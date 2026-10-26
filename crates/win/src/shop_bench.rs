//! The Store Permit watcher on a folder of captured shop frames (`shop_YYYYMMDD_HHMMSS.png`, as the Python version's
//! `scripts/shop_watch.py` saves them): what the Rust watcher reports, line for line like the Python replay, to
//! compare the two. `MAPLELENS_SHOP_FRAMES=<folder> cargo test --release -p maplelens shop_replay -- --ignored
//! --nocapture`. The watcher measures the UI scale off the shop window itself. `MAPLELENS_SHOP_REPEAT=3` looks at each
//! frame three times (frames a minute apart, like `D:\MapleLensCaptures`).

use std::time::Instant;

use maplelens_core::image::decode_png;
use maplelens_core::shop::ShopEvent;
use maplelens_core::shop::watcher::ShopWatcher;

use std::sync::Arc;

use crate::{paths, refresh, shop};

/// `20261024_025123` -> (seconds since midnight, a monotonic stamp for the watcher's clock).
fn stamp(name: &str) -> Option<(u64, f64)> {
    let digits = name.strip_prefix("shop_")?.split('.').next()?;
    let (date, time) = digits.split_once('_')?;
    let part = |range: std::ops::Range<usize>| time.get(range)?.parse::<u64>().ok();
    let seconds = part(0..2)? * 3600 + part(2..4)? * 60 + part(4..6)?;
    let day: u64 = date.get(6..8)?.parse().ok()?;
    Some((seconds, (day * 86_400 + seconds) as f64))
}

#[test]
#[ignore = "needs MAPLELENS_SHOP_FRAMES and Windows OCR"]
fn shop_replay() {
    let Ok(folder) = std::env::var("MAPLELENS_SHOP_FRAMES") else { return };
    let repeat: usize = std::env::var("MAPLELENS_SHOP_REPEAT").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut files: Vec<_> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.path()).collect();
    files.sort();
    let mut watcher = ShopWatcher::new(shop::reader());
    if std::env::var("MAPLELENS_SHOP_NO_CATALOG").is_err()
        && let Ok(catalog) = refresh::load_catalog(&paths::snapshot_file())
    {
        watcher.set_catalog(Arc::new(catalog));
    }
    let (mut frames, mut total_ms, mut worst_ms) = (0usize, 0.0f64, 0.0f64);
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Some((seconds, now)) = stamp(&name) else { continue };
        if !name.ends_with(".png") {
            continue;
        }
        let Ok(frame) = decode_png(&std::fs::read(&path).unwrap()) else { continue };
        let start = Instant::now();
        // Captures a minute apart are each looked at a few times, as the app's one-a-second polls would: a part is
        // only believed once two looks agree.
        let mut events = Vec::new();
        for k in 0..repeat {
            events.extend(watcher.update(&frame, now + k as f64));
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        (frames, total_ms, worst_ms) = (frames + 1, total_ms + ms, worst_ms.max(ms));
        let context = watcher.context();
        for event in events {
            let after = if matches!(event, ShopEvent::Sale { .. }) {
                format!("   [store {:?}, balance {:?}]", context.owner, context.balance)
            } else {
                String::new()
            };
            println!("{:02}:{:02}:{:02} {}{after}", seconds / 3600, seconds / 60 % 60, seconds % 60, event.describe());
        }
    }
    eprintln!("{frames} frames, {:.0} ms mean, {worst_ms:.0} ms worst", total_ms / frames.max(1) as f64);
}
