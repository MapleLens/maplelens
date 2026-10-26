//! Validated, atomic snapshot refreshes on a worker thread: the cache is only ever replaced by a download that
//! parses and builds a catalog, and any failure keeps the old prices.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use maplelens_core::config;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::snapshot::{SnapshotError, parse_snapshot};

use crate::paths;

/// Load the cached snapshot into a catalog.
pub fn load_catalog(path: &Path) -> Result<Catalog, SnapshotError> {
    let text = std::fs::read_to_string(path).map_err(|_| SnapshotError::NotFound(path.display().to_string()))?;
    let snapshot = parse_snapshot(&text)?;
    Ok(Catalog::new(snapshot.items, snapshot.generated_at))
}

fn modified_s(path: &Path) -> Option<f64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

/// Fetch only when absent or stale, preserving the cache on any failure. True when a new snapshot was saved.
pub fn refresh_snapshot(
    path: &Path,
    now: f64,
    fetch: &dyn Fn() -> Result<String, Box<dyn std::error::Error>>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if let Some(mtime) = modified_s(path)
        && (0.0..config::SNAPSHOT_REFRESH_S as f64).contains(&(now - mtime))
        && load_catalog(path).is_ok()
    {
        return Ok(false);
    }
    let body = fetch()?;
    parse_snapshot(&body)?; // a challenge page or a changed format must never replace good prices
    paths::write_atomic(path, body.as_bytes())?;
    let stamp = UNIX_EPOCH + Duration::from_secs_f64(now.max(0.0));
    std::fs::File::options().write(true).open(path)?.set_modified(stamp)?;
    Ok(true)
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Refresh now, then whenever due; failures retry with doubling backoff. New catalogs go to `updates`.
pub fn spawn(path: PathBuf, updates: Sender<Catalog>) {
    std::thread::Builder::new()
        .name("snapshot-refresh".into())
        .spawn(move || {
            let mut retry = config::SNAPSHOT_RETRY_S as f64;
            loop {
                let fetch = || crate::net::fetch_snapshot().map_err(|e| Box::new(e) as Box<dyn std::error::Error>);
                let delay = match refresh_snapshot(&path, now(), &fetch) {
                    Ok(fresh) => {
                        if fresh {
                            match load_catalog(&path) {
                                Ok(catalog) => {
                                    crate::info!("prices updated: {} items", catalog.len());
                                    if updates.send(catalog).is_err() {
                                        return; // the app has quit
                                    }
                                }
                                Err(error) => crate::warn!("saved snapshot does not load: {error}"),
                            }
                        }
                        retry = config::SNAPSHOT_RETRY_S as f64;
                        let due = modified_s(&path).map_or(0.0, |m| m + config::SNAPSHOT_REFRESH_S as f64 - now());
                        due.clamp(1.0, config::SNAPSHOT_REFRESH_S as f64)
                    }
                    Err(error) => {
                        crate::warn!("snapshot refresh failed, keeping cached prices: {error}");
                        let delay = retry;
                        retry = (retry * 2.0).min(config::SNAPSHOT_REFRESH_S as f64);
                        delay
                    }
                };
                std::thread::sleep(Duration::from_secs_f64(delay));
            }
        })
        .expect("spawning the refresh thread");
}
