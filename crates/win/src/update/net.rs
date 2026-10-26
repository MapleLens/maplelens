//! HTTPS fetching with TLS verification, a host allow-list that also covers every redirect, and size caps.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::{TIMEOUT_S, UpdateError, fail};

/// GitHub serves release downloads through a redirect to its asset CDN.
pub const ALLOWED_HOSTS: [&str; 4] =
    ["api.github.com", "github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];
const MAX_REDIRECTS: usize = 5;
const CHUNK: usize = 1 << 16;
const USER_AGENT: &str = concat!("MapleLens/", env!("CARGO_PKG_VERSION"));

/// (bytes done, bytes expected)
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

/// The host of an https URL, if it is one of the allowed ones.
pub fn check_url(url: &str) -> Result<(), UpdateError> {
    let host = url
        .strip_prefix("https://")
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .map(|authority| authority.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or(""));
    match host {
        Some(host) if ALLOWED_HOSTS.contains(&host) => Ok(()),
        _ => fail(format!("refusing to fetch {url}")),
    }
}

/// Open `url`, following redirects by hand so every hop is checked against the allow-list.
fn open(url: &str) -> Result<ureq::http::Response<ureq::Body>, UpdateError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_S)))
        .user_agent(USER_AGENT)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        check_url(&url)?;
        let response = agent
            .get(&url)
            .header("Accept", "application/octet-stream, application/json")
            .call()
            .map_err(|e| UpdateError(format!("could not reach the update server: {e}")))?;
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            let location = response.headers().get("Location").and_then(|v| v.to_str().ok());
            url = location.ok_or_else(|| UpdateError("a redirect without a target".into()))?.to_string();
            continue;
        }
        if status != 200 {
            return fail(format!("the update server answered HTTP {status}"));
        }
        return Ok(response);
    }
    fail("too many redirects")
}

pub fn get_bytes(url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let mut data = Vec::new();
    open(url)?
        .into_body()
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|e| UpdateError(format!("the download broke off: {e}")))?;
    if data.len() as u64 > limit {
        return fail("the server sent more data than expected");
    }
    Ok(data)
}

/// Stream into `dest`; the byte count and hash must match the signed manifest exactly or it is deleted.
pub fn download(url: &str, dest: &Path, size: u64, sha256: &str, progress: Progress<'_>) -> Result<(), UpdateError> {
    let result = (|| {
        let mut reader = open(url)?.into_body().into_reader();
        stream(&mut reader, dest, size, sha256, progress)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

/// Copy `reader` to `dest`, checking it against the expected size and SHA-256 as it goes.
pub fn stream(
    reader: &mut dyn Read,
    dest: &Path,
    size: u64,
    sha256: &str,
    progress: Progress<'_>,
) -> Result<(), UpdateError> {
    let io = |e: std::io::Error| UpdateError(format!("the download broke off: {e}"));
    let mut output = std::fs::File::create(dest).map_err(io)?;
    let (mut digest, mut done, mut buffer) = (Sha256::new(), 0u64, vec![0u8; CHUNK]);
    loop {
        let n = reader.read(&mut buffer).map_err(io)?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > size {
            return fail("the download is larger than the signed manifest says");
        }
        digest.update(&buffer[..n]);
        output.write_all(&buffer[..n]).map_err(io)?;
        progress(done, size);
    }
    let hex: String = digest.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != size || hex != sha256 {
        return fail("the download does not match its checksum");
    }
    Ok(())
}
