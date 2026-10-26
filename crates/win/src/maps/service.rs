//! Non-blocking map lookups: `get` answers at once from what is known; a background thread fetches the rest (one
//! request chain at a time, the newest hover first), caches it, then downloads the sprites.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};

use maplelens_core::config;
use maplelens_core::mapinfo::resolve::{Candidate, pick};
use maplelens_core::mapinfo::{Location, MapInfo};
use maplelens_core::render::RgbaImage;

use super::cache::{IconCache, MapStore, download_icon, icon_path};
use super::client::in_parallel;
use crate::net::NetError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Loading,
    Found,
    /// No map is named like the tooltip.
    Missing,
    /// Network trouble; retried after MAP_RETRY_S.
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapResult {
    pub status: Status,
    pub info: Option<Arc<MapInfo>>,
    /// Sprites on disk so far: a changed count means the card should be redrawn.
    pub icons: usize,
    retry_at: f64,
}

impl MapResult {
    const LOADING: Self = Self { status: Status::Loading, info: None, icons: 0, retry_at: 0.0 };

    fn new(status: Status, info: Option<Arc<MapInfo>>, icons: usize) -> Self {
        Self { status, info, icons, retry_at: 0.0 }
    }
}

/// How a background download of one map ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefetched {
    Done,
    /// Listed but not served (some maps are): skipped, and no sign the server is struggling.
    NotFound,
    Failed,
}

pub type Find = Box<dyn Fn(&Location) -> Result<Option<u32>, NetError> + Send + Sync>;
pub type Fetch = Box<dyn Fn(u32) -> Result<MapInfo, NetError> + Send + Sync>;
pub type FetchIcon = Box<dyn Fn(&str, u32) -> Result<Vec<u8>, NetError> + Send + Sync>;
pub type Clock = Box<dyn Fn() -> f64 + Send + Sync>;

#[derive(Default)]
struct State {
    results: HashMap<Location, MapResult>,
    queued: HashSet<Location>,
    /// Newest last: the newest hover is the one being looked at.
    queue: Vec<Location>,
    wanted: HashMap<Location, f64>,
}

pub struct MapService {
    state: Mutex<State>,
    wake: Condvar,
    icons: Mutex<IconCache>,
    store: Mutex<MapStore>,
    /// Every map (once known): a reading is resolved against it before any search request.
    catalog: Mutex<Vec<Candidate>>,
    find: Find,
    fetch: Fetch,
    fetch_icon: FetchIcon,
    clock: Clock,
}

impl MapService {
    pub fn new(
        icons: IconCache,
        store: MapStore,
        (find, fetch, fetch_icon): (Find, Fetch, FetchIcon),
        clock: Clock,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            icons: Mutex::new(icons),
            store: Mutex::new(store),
            catalog: Mutex::new(Vec::new()),
            find,
            fetch,
            fetch_icon,
            clock,
        })
    }

    pub fn set_catalog(&self, catalog: Vec<Candidate>) {
        *self.catalog.lock().unwrap_or_else(|e| e.into_inner()) = catalog;
    }

    pub fn store(&self) -> std::sync::MutexGuard<'_, MapStore> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Make sure map `id` and its sprites are on disk (the background download's job).
    pub fn prefetch(&self, id: u32) -> Prefetched {
        let cached = self.store().load(id);
        let info = match cached {
            Some(info) => info,
            None => match (self.fetch)(id) {
                Ok(info) => {
                    self.store().save(&info);
                    info
                }
                Err(error) => {
                    crate::debug!("prefetch of map {id} failed: {error}");
                    return if error.is_not_found() { Prefetched::NotFound } else { Prefetched::Failed };
                }
            },
        };
        // A sprite the server does not have (404) is not a failed map; the hover asks for it again.
        self.fetch_missing_icons(&info, |_| {});
        Prefetched::Done
    }

    pub fn spawn_worker(self: &Arc<Self>) {
        let service = self.clone();
        let _ = std::thread::Builder::new().name("map-fetch".into()).spawn(move || service.work());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What is known about `location` now; queues a lookup when there is none (or a failed one is due again).
    pub fn get(&self, location: &Location) -> MapResult {
        let now = (self.clock)();
        let mut state = self.lock();
        state.wanted.insert(location.clone(), now);
        let result = state.results.get(location).cloned().unwrap_or(MapResult::LOADING);
        let due = result.status == Status::Loading || (result.status == Status::Failed && now >= result.retry_at);
        if due && state.queued.insert(location.clone()) {
            state.queue.push(location.clone());
            self.wake.notify_one();
        }
        result
    }

    /// A sprite from memory or disk (never the network).
    pub fn icon(&self, kind: &str, id: u32) -> Option<Arc<RgbaImage>> {
        self.icons.lock().unwrap_or_else(|e| e.into_inner()).get(kind, id)
    }

    /// Lookups waiting for the worker.
    #[cfg(test)]
    pub fn queued(&self) -> usize {
        self.lock().queue.len()
    }

    fn work(&self) {
        loop {
            let location = {
                let mut state = self.lock();
                while state.queue.is_empty() {
                    state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                let location = state.queue.pop().expect("not empty");
                let asked = state.wanted.get(&location).copied().unwrap_or(0.0);
                if (self.clock)() - asked > config::MAP_STALE_S {
                    // The pointer moved on while it waited (OCR variants pile up): not worth a request.
                    state.queued.remove(&location);
                    continue;
                }
                location
            };
            self.refresh(&location);
        }
    }

    /// Look `location` up now, then fetch its sprites (the worker's job; tests call it directly).
    pub fn refresh(&self, location: &Location) {
        let result = match self.lookup(location) {
            Ok(None) => MapResult::new(Status::Missing, None, 0),
            Ok(Some(info)) => MapResult::new(Status::Found, Some(Arc::new(info)), 0),
            Err(error) => {
                crate::warn!("map lookup for {} : {} failed: {error}", location.street, location.name);
                MapResult { retry_at: (self.clock)() + config::MAP_RETRY_S, ..MapResult::new(Status::Failed, None, 0) }
            }
        };
        crate::debug!("map lookup {location:?} -> {:?}", result.status);
        let info = result.info.clone();
        self.put(location, result);
        if let Some(info) = info {
            self.download_icons(location, info);
        }
    }

    /// From the disk cache when seen before, else from the network (and cached).
    fn lookup(&self, location: &Location) -> Result<Option<MapInfo>, NetError> {
        let known = self.store.lock().unwrap_or_else(|e| e.into_inner()).find(location);
        let local = || pick(location, self.catalog.lock().unwrap_or_else(|e| e.into_inner()).iter());
        let id = match known.or_else(local) {
            Some(id) => {
                self.store.lock().unwrap_or_else(|e| e.into_inner()).remember(location, id);
                id
            }
            None => match (self.find)(location)? {
                Some(id) => {
                    self.store.lock().unwrap_or_else(|e| e.into_inner()).remember(location, id);
                    id
                }
                None => return Ok(None),
            },
        };
        if let Some(info) = self.store.lock().unwrap_or_else(|e| e.into_inner()).load(id) {
            return Ok(Some(info));
        }
        let info = (self.fetch)(id)?;
        self.store.lock().unwrap_or_else(|e| e.into_inner()).save(&info);
        Ok(Some(info))
    }

    fn put(&self, location: &Location, result: MapResult) {
        let mut state = self.lock();
        state.results.insert(location.clone(), result);
        state.queued.remove(location);
    }

    fn download_icons(&self, location: &Location, info: Arc<MapInfo>) {
        let progress = |ready| self.put(location, MapResult::new(Status::Found, Some(info.clone()), ready));
        let (ready, _) = self.fetch_missing_icons(&info, progress);
        progress(ready);
    }

    /// Download the sprites of `info` that are not on disk, in parallel (a map has a dozen; one after the other
    /// they took seconds). `progress` gets the count on disk so far; returns (on disk, wanted).
    fn fetch_missing_icons(&self, info: &MapInfo, progress: impl Fn(usize) + Sync) -> (usize, usize) {
        let wanted: Vec<(&str, u32)> =
            info.mobs.iter().map(|m| ("mob", m.id)).chain(info.npcs.iter().map(|n| ("npc", n.id))).collect();
        let folder = self.icons.lock().unwrap_or_else(|e| e.into_inner()).folder().to_path_buf();
        let missing: Vec<(&str, u32)> =
            wanted.iter().copied().filter(|&(kind, id)| !icon_path(&folder, kind, id).exists()).collect();
        let ready = std::sync::atomic::AtomicUsize::new(wanted.len() - missing.len());
        progress(ready.load(std::sync::atomic::Ordering::Relaxed));
        // No lock is held while downloading: the UI keeps reading sprites meanwhile.
        in_parallel(&missing, |&(kind, id)| {
            if download_icon(&folder, kind, id, &self.fetch_icon) {
                progress(ready.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1);
            }
        });
        (ready.into_inner(), wanted.len())
    }
}
