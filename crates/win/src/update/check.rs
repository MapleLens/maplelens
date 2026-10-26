//! Looking for a newer release: GitHub names the candidate, the signed manifest is what makes it believable.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use serde_json::Value;

use super::manifest::{asset_name, manifest_name, verify_manifest};
use super::version::{format_version, parse_version};
use super::{CHECK_S, MAX_API_BYTES, MAX_MANIFEST_BYTES, REPO, RETRY_S, Update, UpdateError, fail};

/// `latest` never returns a draft or a pre-release.
fn api_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

pub fn download_url(version: &str, name: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/v{version}/{name}")
}

pub fn release_page(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/v{version}")
}

/// (url, size limit) -> bytes
pub type Fetch = dyn Fn(&str, u64) -> Result<Vec<u8>, UpdateError> + Send + Sync;

/// None when up to date. Download URLs are built here from the version, never taken from server-sent text.
pub fn find_update(current: &str, fetch: &Fetch, public_key: &str) -> Result<Option<Update>, UpdateError> {
    let release: Value = serde_json::from_slice(&fetch(&api_url(), MAX_API_BYTES)?)
        .map_err(|_| UpdateError("GitHub sent an unexpected answer".into()))?;
    let tag = release["tag_name"].as_str().ok_or_else(|| UpdateError("GitHub sent an unexpected answer".into()))?;
    let latest = parse_version(tag)?;
    let flagged = |key: &str| release[key].as_bool().unwrap_or(false);
    if flagged("draft") || flagged("prerelease") || latest <= parse_version(current)? {
        return Ok(None);
    }
    let version = format_version(latest);
    // Releases from before the in-app updater (0.1.3 and older) have no signed manifest.
    let unsigned =
        |error: UpdateError| UpdateError(format!("version {version} cannot be installed from here ({error})"));
    let signed = fetch(&download_url(&version, &manifest_name(&version)), MAX_MANIFEST_BYTES).map_err(unsigned)?;
    let signature =
        fetch(&download_url(&version, &(manifest_name(&version) + ".sig")), MAX_MANIFEST_BYTES).map_err(unsigned)?;
    let manifest = verify_manifest(&signed, &signature, public_key)?;
    if manifest.version != version {
        // A validly signed manifest of another release must not pass as this one.
        return fail("the update manifest is for a different version");
    }
    let url = download_url(&version, &asset_name(&version));
    Ok(Some(Update { page_url: release_page(&version), version, manifest, url }))
}

/// How a check went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckResult {
    Found(Option<Update>),
    Failed(String),
}

enum Wake {
    Now,
    Stop,
}

/// Checks at start-up and then daily (while `auto_check` is on); `check_now` asks for a check at once.
pub struct UpdateChecker {
    pub events: Receiver<CheckResult>,
    wake: Sender<Wake>,
    auto_check: Arc<AtomicBool>,
}

impl UpdateChecker {
    /// `public_key` is the one release manifests must be signed with (`update::PUBLIC_KEY`).
    pub fn spawn(current: &str, auto_check: bool, fetch: Box<Fetch>, public_key: &str) -> Self {
        let (events_tx, events) = channel();
        let (wake, wake_rx) = channel();
        let auto = Arc::new(AtomicBool::new(auto_check));
        let (current, shared, key) = (current.to_string(), auto.clone(), public_key.to_string());
        std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || run(&current, &key, &shared, fetch.as_ref(), &wake_rx, &events_tx))
            .expect("spawning the update thread");
        Self { events, wake, auto_check: auto }
    }

    pub fn set_auto_check(&self, on: bool) {
        self.auto_check.store(on, Ordering::Relaxed);
    }

    pub fn check_now(&self) {
        let _ = self.wake.send(Wake::Now);
    }
}

impl Drop for UpdateChecker {
    fn drop(&mut self) {
        let _ = self.wake.send(Wake::Stop);
    }
}

fn run(
    current: &str,
    key: &str,
    auto: &AtomicBool,
    fetch: &Fetch,
    wake: &Receiver<Wake>,
    events: &Sender<CheckResult>,
) {
    let mut wait = if auto.load(Ordering::Relaxed) { Some(Duration::ZERO) } else { None };
    loop {
        let manual = match wait {
            Some(delay) => match wake.recv_timeout(delay) {
                Ok(Wake::Now) => true,
                Ok(Wake::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => false,
            },
            None => match wake.recv() {
                Ok(Wake::Now) => true,
                _ => return,
            },
        };
        if !(manual || auto.load(Ordering::Relaxed)) {
            wait = None;
            continue;
        }
        let result = match find_update(current, fetch, key) {
            Ok(update) => {
                wait = Some(Duration::from_secs(CHECK_S));
                CheckResult::Found(update)
            }
            Err(error) => {
                crate::warn!("update check failed: {error}");
                wait = Some(Duration::from_secs(RETRY_S));
                CheckResult::Failed(error.0)
            }
        };
        if events.send(result).is_err() {
            return;
        }
        if !auto.load(Ordering::Relaxed) {
            wait = None;
        }
    }
}
