//! Background download of the popular regions (Victoria Island by default), so a hover is answered from disk:
//! first the list of every map (kept a day on disk), then each map and its sprites. The lookup worker is a
//! separate thread, so what the player hovers never waits behind this.

use std::sync::Arc;
use std::time::Duration;

use maplelens_core::config;
use maplelens_core::mapinfo::resolve::Candidate;

use super::service::{MapService, Prefetched};
use crate::net::NetError;

/// The run stops after this many maps in a row fail (offline, or the server is struggling).
const GIVE_UP_AFTER: usize = 5;
const CATALOG_ATTEMPTS: usize = 3;
const CATALOG_RETRY_S: u64 = 10;
const REGION_SIZE: u32 = 10_000_000;

/// The list of maps: from disk when fresh, else fetched (the stale copy still serves when that fails).
fn catalog(service: &MapService, fetch: impl Fn() -> Result<Vec<Candidate>, NetError>) -> Option<Vec<Candidate>> {
    let saved = service.store().catalog();
    if let Some((catalog, age)) = &saved
        && *age < config::MAP_CATALOG_MAX_AGE_S
    {
        return Some(catalog.clone());
    }
    for attempt in 1..=CATALOG_ATTEMPTS {
        match fetch() {
            Ok(catalog) => {
                service.store().save_catalog(&catalog);
                return Some(catalog);
            }
            Err(error) => {
                crate::warn!("could not fetch the map list (try {attempt}): {error}");
                std::thread::sleep(Duration::from_secs(CATALOG_RETRY_S));
            }
        }
    }
    saved.map(|(catalog, _)| catalog)
}

/// Ids in `regions` of `catalog` that `is_cached` says are missing, in id order (Henesys first).
pub fn wanted(catalog: &[Candidate], regions: &[u32], is_cached: impl Fn(u32) -> bool) -> Vec<u32> {
    let mut ids: Vec<u32> =
        catalog.iter().map(|c| c.id).filter(|id| regions.contains(&(id / REGION_SIZE)) && !is_cached(*id)).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Resolve against the whole list from now on, then fill the disk cache in the background.
pub fn spawn(service: Arc<MapService>, fetch: impl Fn() -> Result<Vec<Candidate>, NetError> + Send + 'static) {
    let _ = std::thread::Builder::new().name("map-prefetch".into()).spawn(move || {
        let Some(catalog) = catalog(&service, fetch) else { return };
        let ids = wanted(&catalog, &config::MAP_PREFETCH_REGIONS, |id| service.store().has(id));
        service.set_catalog(catalog);
        crate::info!("map prefetch: {} maps to download", ids.len());
        let (mut done, mut failures) = (0, 0);
        for id in &ids {
            match service.prefetch(*id) {
                Prefetched::Done => (done, failures) = (done + 1, 0),
                Prefetched::NotFound => {}
                Prefetched::Failed => {
                    failures += 1;
                    if failures >= GIVE_UP_AFTER {
                        crate::warn!("map prefetch stopped after {done} maps: the server is not answering");
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_secs_f64(config::MAP_PREFETCH_PAUSE_S));
        }
        crate::info!("map prefetch: done, {done} maps downloaded");
    });
}
