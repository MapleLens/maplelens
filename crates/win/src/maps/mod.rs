//! The map helper: hover a location on the world map and a card lists its monsters and NPCs (maplestory.io data,
//! classic era). Its own detection thread and lookup service; the card lives in `app::map`.

pub mod cache;
pub mod client;
pub mod prefetch;
pub mod service;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use maplelens_core::config;
use maplelens_core::geometry::UiScale;
use maplelens_core::image::encode_png;
use maplelens_core::mapinfo::Location;
use maplelens_core::pipeline::map::{MapHover, MapScanner};

use crate::capture::GdiScreen;
use crate::workers::GameWatch;
use crate::{game, ocr};

/// The real service: maplestory.io behind the disk caches.
pub fn service() -> Arc<service::MapService> {
    let start = Instant::now();
    let service = service::MapService::new(
        cache::IconCache::new(cache::cache_dir().join("icons")),
        cache::MapStore::new(cache::cache_dir().join("maps")),
        (Box::new(client::find_map_id), Box::new(client::fetch_map), Box::new(client::fetch_icon)),
        Box::new(move || start.elapsed().as_secs_f64()),
    );
    service.spawn_worker();
    prefetch::spawn(service.clone(), client::fetch_catalog);
    service
}

/// What the UI tells the scanner: the overlay toggle, whether an item tooltip is up, and the measured UI scale.
pub struct ScanFlags {
    pub enabled: Arc<AtomicBool>,
    pub item_showing: Arc<AtomicBool>,
    pub ui: Arc<UiScale>,
}

/// World-map tooltip -> `Some(MapHover)` on `events` (None while there is none). `rejects` are readings the UI
/// found no map for.
pub fn spawn_scanner(events: Sender<Option<MapHover>>, rejects: Receiver<Location>, flags: ScanFlags, debug: bool) {
    std::thread::Builder::new()
        .name("map-pipeline".into())
        .spawn(move || {
            let mut scanner = MapScanner::new(Box::new(ocr::CachedLines::new()));
            scanner.keep_captures = debug;
            let (mut screen, mut watch) = (GdiScreen::new(), GameWatch::default());
            let start = Instant::now();
            loop {
                let now = start.elapsed().as_secs_f64();
                while let Ok(location) = rejects.try_recv() {
                    scanner.reject(&location, now);
                }
                let wait =
                    if flags.enabled.load(Ordering::Relaxed) { watch.front() } else { Err(config::POLL_BACKGROUND_S) };
                let (event, delay) = match wait {
                    Err(delay) => (None, delay), // never draw over other apps
                    Ok(_) if flags.item_showing.load(Ordering::Relaxed) => {
                        // Item tooltips (equips above all) are full of text that is not a map title.
                        scanner.stand_down();
                        (None, config::MAP_POLL_S)
                    }
                    Ok(hwnd) => match game::client_rect(hwnd) {
                        Some(bounds) => {
                            let scale = flags.ui.of(bounds);
                            let event = scanner.step(&mut screen, bounds, scale, now);
                            let busy = event.is_some() || scanner.is_active(now);
                            (event, if busy { config::MAP_POLL_S } else { config::MAP_IDLE_POLL_S })
                        }
                        None => (None, config::POLL_BACKGROUND_S),
                    },
                };
                if let Some(image) = scanner.capture.take()
                    && std::fs::create_dir_all("debug").is_ok()
                {
                    // Debug aid: the scan window whenever it held tooltip-like colours (source of test fixtures).
                    let _ = std::fs::write("debug/map_scan.png", encode_png(&image));
                }
                if events.send(event).is_err() {
                    return; // the app has quit
                }
                std::thread::sleep(Duration::from_secs_f64(delay));
            }
        })
        .expect("spawning the map thread");
}
