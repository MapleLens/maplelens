//! Safe self-update: find a newer signed release, download it, verify it, swap the exe.
//!
//! Never weaken the chain: only a manifest signed by `PUBLIC_KEY` is trusted, download URLs are built from the
//! verified version (never from server text), and the zip's size and SHA-256 must match that manifest. Only an
//! installed exe swaps itself; a development build opens the release page instead.

pub mod check;
pub mod install;
pub mod manifest;
pub mod net;
pub mod release;
pub mod version;

use std::fmt;

use manifest::Manifest;

pub const REPO: &str = "MapleLens/maplelens";
/// Ed25519 public key (base64, raw) that release manifests must be signed with. Rotating it strands every
/// installed copy, so keep the private key (CI secret UPDATE_SIGNING_KEY) safe; `maplelens --make-update-key`
/// makes it (see `release`).
pub const PUBLIC_KEY: &str = "Qa1pyznR0jsj3Vdhk5i/OrBzkfsCdipbQ1zavbranMs=";
pub const CHECK_S: u64 = 24 * 3600;
/// A failed check (offline, rate limited) is retried sooner than the daily one.
pub const RETRY_S: u64 = 15 * 60;
pub const TIMEOUT_S: u64 = 30;
/// Hard size caps so a hostile or broken server cannot fill the disk or memory.
pub const MAX_API_BYTES: u64 = 1 << 20;
pub const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
pub const MAX_ZIP_BYTES: u64 = 300 << 20;
/// The new exe has this long to answer `--version` (a cold disk and an antivirus scan can be slow).
pub const SELF_TEST_S: u64 = 90;

/// Anything that makes an update untrustworthy or impossible; the message is shown to the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateError(pub String);

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UpdateError {}

pub fn fail<T>(message: impl Into<String>) -> Result<T, UpdateError> {
    Err(UpdateError(message.into()))
}

/// A newer release, vouched for by its signed manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub version: String,
    pub manifest: Manifest,
    /// Where the zip comes from.
    pub url: String,
    /// The human release page, for builds that cannot update themselves.
    pub page_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Failed,
}

/// What the update machinery tells the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateState {
    pub phase: Phase,
    /// Kept through Downloading and Failed so the player can retry.
    pub update: Option<Update>,
    /// Error text, or download progress such as "42%".
    pub detail: String,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self { phase: Phase::Idle, update: None, detail: String::new() }
    }
}

impl UpdateState {
    pub fn new(phase: Phase, update: Option<Update>, detail: impl Into<String>) -> Self {
        Self { phase, update, detail: detail.into() }
    }

    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Checking | Phase::Downloading)
    }

    /// (status line, Check now enabled, Update now enabled) for the settings window.
    pub fn describe(&self) -> (String, bool, bool) {
        let version = self.update.as_ref().map_or("", |u| u.version.as_str());
        let text = match self.phase {
            Phase::Idle => String::new(),
            Phase::Checking => "Checking for updates...".into(),
            Phase::UpToDate => "You have the latest version.".into(),
            Phase::Available => format!("Version {version} is available."),
            Phase::Downloading => format!("Downloading version {version}... {}", self.detail).trim().to_string(),
            Phase::Failed if self.update.is_some() => format!("Update failed: {}", self.detail),
            Phase::Failed => format!("Could not check: {}", self.detail),
        };
        (text, !self.busy(), self.update.is_some() && !self.busy())
    }
}

#[cfg(test)]
mod tests;
