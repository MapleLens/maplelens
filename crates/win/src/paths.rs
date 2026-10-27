//! Where MapleLens keeps its files (one per-user folder: no admin rights, nothing next to the game), the wall
//! clock the cards' "3h ago" ages are measured against, and the steady clock the detector threads share.

use std::path::PathBuf;

/// %LOCALAPPDATA%\MapleLens: settings, the cached price snapshot and learnt glyphs.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("USERPROFILE")
            .map_or_else(|| PathBuf::from("."), |home| PathBuf::from(home).join("AppData").join("Local"))
    });
    base.join("MapleLens")
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}

/// The cached price snapshot. Per user rather than next to the exe: started at sign-in the working directory
/// is System32, and the exe may live somewhere read-only.
pub fn snapshot_file() -> PathBuf {
    data_dir().join("list.json")
}

/// Letters the glyph atlas learnt on this machine, per font.
pub fn learnt_glyphs(font: &str) -> PathBuf {
    data_dir().join(format!("glyphs-{font}.json"))
}

/// The exe's folder: where players expect the log file they are asked to send.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(PathBuf::from)
}

/// Write `contents` to `path` through a temporary file, so a crash never leaves half a file.
pub fn write_atomic(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Seconds since the Unix epoch, now.
pub fn now_unix() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Seconds since the first call, on a steady clock: the threads that vote on the UI scale (`UiScale`) must all
/// time their readings on this one.
pub fn uptime() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}
