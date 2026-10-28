//! The healing and buff items' icons for the training reader, so it can tell which potion each quickslot holds:
//! maplestory.io's list and icons (its Classic data, the source of the sold items' pictures), kept in
//! %LOCALAPPDATA%\MapleLens\cache\potions so only the first start downloads them.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime};

use maplelens_core::training::potions;

use crate::net::{NetError, agent};
use crate::paths;

/// One request's time limit (maplestory.io is sometimes slow).
const TIMEOUT_S: u64 = 20;
/// The list (~100 KB) and an icon (~1 KB) are small; anything far bigger is not them.
const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// The list is asked for again after this long: new items are rare.
const LIST_FRESH: Duration = Duration::from_secs(7 * 24 * 3600);

/// An icon for the reader: (id, name, PNG).
pub type PotionIcon = (u32, String, Vec<u8>);

fn dir() -> PathBuf {
    paths::data_dir().join("cache").join("potions")
}

fn fetch(url: &str) -> Result<Vec<u8>, NetError> {
    let mut response = agent(TIMEOUT_S).get(url).call().map_err(|e| NetError(format!("{e}")))?;
    match response.status().as_u16() {
        200 => response.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| NetError(format!("{e}"))),
        status => Err(NetError(format!("HTTP {status}"))),
    }
}

/// The cached copy at `path` (when `fresh` says it still is), else downloaded from `url` and cached.
fn cached(path: &std::path::Path, url: &str, fresh: impl Fn(SystemTime) -> bool) -> Result<Vec<u8>, NetError> {
    let fresh_copy = std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(fresh);
    if fresh_copy && let Ok(bytes) = std::fs::read(path) {
        return Ok(bytes);
    }
    match fetch(url) {
        Ok(bytes) => {
            if let Err(error) = paths::write_atomic(path, &bytes) {
                crate::debug!("could not cache {}: {error}", path.display());
            }
            Ok(bytes)
        }
        // Offline: an old copy is better than none.
        Err(error) => std::fs::read(path).map_err(|_| error),
    }
}

/// Send every potion's icon to the reader, from the cache or downloaded, on a thread of its own (the first start
/// downloads ~65 icons; the reader keeps reading the EXP meanwhile).
pub fn spawn() -> Receiver<PotionIcon> {
    let (tx, rx) = mpsc::channel();
    let started = std::thread::Builder::new().name("potion-icons".into()).spawn(move || send_all(&tx));
    if let Err(error) = started {
        crate::warn!("could not start the potion icon download: {error}");
    }
    rx
}

fn send_all(tx: &Sender<PotionIcon>) {
    let dir = dir();
    let list =
        match cached(&dir.join("list.json"), &potions::list_url(), |m| m.elapsed().is_ok_and(|age| age < LIST_FRESH)) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => return crate::warn!("potion list unavailable ({error}): potions are not counted"),
        };
    let mut missing = 0;
    for (id, name) in potions::candidates(&list) {
        match cached(&dir.join(format!("{id}.png")), &potions::icon_url(id), |_| true) {
            Ok(png) => {
                if tx.send((id, name, png)).is_err() {
                    return; // the reader has stopped
                }
            }
            Err(_) => missing += 1,
        }
    }
    if missing > 0 {
        crate::warn!("{missing} potion icons could not be downloaded: those potions are not counted");
    }
}
