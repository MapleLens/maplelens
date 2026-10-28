//! HTTPS fetching with TLS verification, a host allow-list that also covers every redirect, and size caps.

use std::io::Read;
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

/// The whole download, in memory; its byte count and hash must match the signed manifest exactly. Never staged in a
/// file: %TEMP% is writable by every program the player runs, and the elevated app would have unpacked whatever one
/// of them swapped in there after the hash was checked.
pub fn download(url: &str, size: u64, sha256: &str, progress: Progress<'_>) -> Result<Vec<u8>, UpdateError> {
    let mut reader = open(url)?.into_body().into_reader();
    stream(&mut reader, size, sha256, progress)
}

/// Read `reader` to its end, checking it against the expected size and SHA-256 as it goes.
pub fn stream(reader: &mut dyn Read, size: u64, sha256: &str, progress: Progress<'_>) -> Result<Vec<u8>, UpdateError> {
    let io = |e: std::io::Error| UpdateError(format!("the download broke off: {e}"));
    // `size` is the signed manifest's, at most MAX_ZIP_BYTES.
    let mut data = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    let (mut digest, mut buffer) = (Sha256::new(), vec![0u8; CHUNK]);
    loop {
        let n = reader.read(&mut buffer).map_err(io)?;
        if n == 0 {
            break;
        }
        if (data.len() + n) as u64 > size {
            return fail("the download is larger than the signed manifest says");
        }
        digest.update(&buffer[..n]);
        data.extend_from_slice(&buffer[..n]);
        progress(data.len() as u64, size);
    }
    let hex: String = digest.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if data.len() as u64 != size || hex != sha256 {
        return fail("the download does not match its checksum");
    }
    Ok(data)
}
