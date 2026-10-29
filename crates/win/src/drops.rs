//! The map drops panel's data: NiaDB's monster list, each monster's confirmed drops and the reference drop rates its
//! page shows (meowdb.com, used with the site's permission; the rates are OSMLib's), and the dropped items' icons
//! (maplestory.io's Classic data, like the sale pictures; cache\icons\item). Everything is kept in cache\meowdb and
//! shown from there at once, a copy older than a week too (it is refreshed behind it). A map's monsters missing from
//! it are fetched together; once the panel has shown in a run, while nothing is asked, the site thread gathers every
//! listed monster's drops and rates ahead, one request every two seconds, so a map met later needs no request at all. Before it shows, nothing is fetched.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use maplelens_core::drops::meowdb::{self, Monster};
use maplelens_core::drops::{Drop, DropTable, Listed};
use maplelens_core::mapinfo::MapInfo;
use maplelens_core::notify::item_icon_url;
use maplelens_core::render::RgbaImage;

use crate::maps::cache::{IconCache, cache_dir, download_icon};
use crate::maps::client::in_parallel;
use crate::net::NetError;
use crate::{paths, potions};

/// New monsters are rare, and players confirm drops slowly: both are read again weekly, behind the copy shown.
const FRESH: Duration = Duration::from_secs(7 * 24 * 3600);
/// Between two requests gathering ahead (a family-run fansite): every monster's drops and page in about a quarter of
/// an hour, once a week.
const REQUEST_GAP: Duration = Duration::from_secs(2);
/// A monster whose drops could not be fetched is not tried again before this.
const RETRY: Duration = Duration::from_secs(5 * 60);

/// What the panel asks the site thread for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Want {
    /// The panel's first ask of the run: the list, if missing or over a week old, and the work behind it may start.
    List,
    /// A map's monsters (the site's IDs) not known yet: fetched together.
    Drops(Vec<u32>),
}

#[derive(Default)]
struct Known {
    monsters: Vec<Monster>,
    /// By the site's monster ID.
    drops: HashMap<u32, Vec<Drop>>,
    /// The reference rates on each monster's page (by the site's monster ID), by the game's item ID.
    rates: HashMap<u32, Vec<(u32, f64)>>,
}

pub struct DropBook {
    known: Arc<Mutex<Known>>,
    /// Counts what the site thread has learnt: each answer redraws the panel.
    generation: Arc<AtomicU64>,
    wanted: Option<Sender<Want>>,
    asked: RefCell<HashSet<Option<u32>>>,
    icons: RefCell<IconCache>,
    icons_asked: RefCell<HashSet<u32>>,
    icons_wanted: Option<Sender<u32>>,
    /// Icons downloaded so far: each one redraws the panel.
    icons_ready: Arc<AtomicUsize>,
}

impl DropBook {
    pub fn new() -> Self {
        let (known, generation) = (Arc::new(Mutex::new(Known::default())), Arc::new(AtomicU64::new(0)));
        let folder = cache_dir().join("icons");
        let icons_ready = Arc::new(AtomicUsize::new(0));
        Self {
            wanted: spawn_site(known.clone(), generation.clone()),
            known,
            generation,
            asked: RefCell::default(),
            icons_wanted: spawn_icons(folder.clone(), icons_ready.clone()),
            icons: RefCell::new(IconCache::new(folder)),
            icons_asked: RefCell::default(),
            icons_ready,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// What `info`'s monsters drop, as far as it is known; the monsters not known yet are asked for together (once:
    /// the site thread tries a failure again while gathering).
    pub fn table_for(&self, info: &MapInfo) -> DropTable {
        // The first ask of the run, whatever is on disk: the site thread starts its work with it (the list's weekly
        // refresh, gathering ahead).
        if self.asked.borrow_mut().insert(None) {
            self.send(Want::List);
        }
        let known = lock(&self.known);
        let mut table = DropTable::new(&meowdb::panel_credit());
        if known.monsters.is_empty() {
            return table;
        }
        let mut missing = Vec::new();
        for mob in &info.mobs {
            let Some(monster) = meowdb::find(&known.monsters, mob) else { continue };
            match known.drops.get(&monster.id) {
                Some(drops) => {
                    let rates = known.rates.get(&monster.id).map_or(&[][..], Vec::as_slice);
                    let drops = meowdb::with_rates(drops.clone(), rates);
                    table.insert(mob.id, Listed { drops, credit: meowdb::credit(monster.id) });
                }
                None if self.asked.borrow_mut().insert(Some(monster.id)) => missing.push(monster.id),
                None => {}
            }
        }
        if !missing.is_empty() {
            self.send(Want::Drops(missing));
        }
        table
    }

    fn send(&self, want: Want) {
        if let Some(wanted) = &self.wanted {
            let _ = wanted.send(want);
        }
    }

    /// An item's icon from memory or disk; one not there yet is asked for once (`icons_ready` counts the arrivals).
    pub fn icon(&self, id: u32) -> Option<Arc<RgbaImage>> {
        let icon = self.icons.borrow_mut().get("item", id);
        if icon.is_none()
            && self.icons_asked.borrow_mut().insert(id)
            && let Some(wanted) = &self.icons_wanted
        {
            let _ = wanted.send(id);
        }
        icon
    }

    pub fn icons_ready(&self) -> usize {
        self.icons_ready.load(Ordering::Relaxed)
    }
}

fn lock(known: &Mutex<Known>) -> MutexGuard<'_, Known> {
    known.lock().unwrap_or_else(|e| e.into_inner())
}

fn spawn_site(known: Arc<Mutex<Known>>, generation: Arc<AtomicU64>) -> Option<Sender<Want>> {
    let (tx, rx) = mpsc::channel();
    let site =
        Site { known, generation, last_request: None, failed: HashMap::new(), gathered: HashSet::new(), in_use: false };
    match std::thread::Builder::new().name("map-drops".into()).spawn(move || site.run(&rx)) {
        Ok(_) => Some(tx),
        Err(error) => {
            crate::warn!("could not start the map drops thread: {error}");
            None
        }
    }
}

/// The site thread: what is on disk first, then the panel's wants, and between them the gathering ahead.
struct Site {
    known: Arc<Mutex<Known>>,
    generation: Arc<AtomicU64>,
    last_request: Option<Instant>,
    /// Monsters whose drops failed to arrive, and when.
    failed: HashMap<u32, Instant>,
    /// Monsters fetched (or found fresh on disk) in this run: gathering is done with them.
    gathered: HashSet<u32>,
    /// The panel has asked for something in this run. Until then nothing is fetched: most runs never show it (map
    /// drops off, never on a field), and every new install gathering at its first start would flood the site.
    in_use: bool,
}

impl Site {
    fn run(mut self, wants: &Receiver<Want>) {
        self.load_disk();
        loop {
            match wants.recv_timeout(REQUEST_GAP) {
                Ok(Want::List) => {
                    self.in_use = true;
                    self.ensure_list();
                }
                Ok(Want::Drops(ids)) => {
                    self.in_use = true;
                    self.fetch_together(&ids);
                }
                Err(RecvTimeoutError::Timeout) if self.in_use => self.gather_one(),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// The list and every monster's drops and rates as last saved, whatever their age: shown at once. A monster is
    /// gathered when both are fresh (drops saved before the rates were read get them at the next gathering).
    fn load_disk(&mut self) {
        let monsters = std::fs::read_to_string(dir().join("monsters.json"))
            .map(|t| meowdb::monsters_from_json(&t))
            .unwrap_or_default();
        let (drops, fresh_drops) = read_folder("drops", |t| meowdb::parse_drops(t).ok());
        let (rates, fresh_rates) = read_folder("rates", |t| Some(meowdb::rates_from_json(t)));
        self.gathered.extend(fresh_drops.intersection(&fresh_rates));
        let (listed, rated) = (drops.len(), rates.len());
        crate::info!("map drops: {} monsters, {listed} drop lists and {rated} rate lists on disk", monsters.len());
        *lock(&self.known) = Known { monsters, drops, rates };
        self.learnt();
    }

    fn learnt(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// The monster list, fetched when there is none or it is over a week old (the old one stays shown meanwhile).
    fn ensure_list(&mut self) {
        let path = dir().join("monsters.json");
        if !lock(&self.known).monsters.is_empty() && is_fresh(&path) {
            return;
        }
        let fetched = self.fetch(&meowdb::monsters_url()).and_then(|page| {
            let monsters = meowdb::monsters_from_page(&String::from_utf8_lossy(&page));
            if monsters.is_empty() { Err(NetError("no monsters on the page".into())) } else { Ok(monsters) }
        });
        match fetched {
            Ok(monsters) => {
                crate::info!("map drops: {} monsters listed on NiaDB", monsters.len());
                save(&path, meowdb::monsters_to_json(&monsters).as_bytes());
                lock(&self.known).monsters = monsters;
                self.learnt();
            }
            Err(error) => crate::warn!("map drops: the monster list is unavailable ({error})"),
        }
    }

    /// A map's monsters at once: the player is waiting for them.
    fn fetch_together(&mut self, ids: &[u32]) {
        let results = in_parallel(ids, |&id| (id, fetch_monster(id, Duration::ZERO)));
        self.last_request = Some(Instant::now());
        for (id, result) in results {
            self.store(id, result);
        }
    }

    /// One monster not gathered yet (missing, or saved over a week ago), between the panel's wants.
    fn gather_one(&mut self) {
        let next =
            {
                let known = lock(&self.known);
                known.monsters.iter().map(|m| m.id).find(|id| {
                    !self.gathered.contains(id) && self.failed.get(id).is_none_or(|at| at.elapsed() >= RETRY)
                })
            };
        if let Some(id) = next {
            if let Some(wait) = self.last_request.and_then(|at| REQUEST_GAP.checked_sub(at.elapsed())) {
                std::thread::sleep(wait);
            }
            self.last_request = Some(Instant::now());
            let result = fetch_monster(id, REQUEST_GAP);
            self.last_request = Some(Instant::now());
            self.store(id, result);
        }
    }

    fn store(&mut self, id: u32, result: Result<Fetched, NetError>) {
        match result {
            Ok(Fetched { drops, rates }) => {
                self.failed.remove(&id);
                self.gathered.insert(id);
                let mut known = lock(&self.known);
                known.drops.insert(id, drops);
                if let Some(rates) = rates {
                    known.rates.insert(id, rates);
                }
                drop(known);
                self.learnt();
            }
            Err(error) => {
                crate::warn!("map drops: monster {id} unavailable ({error})");
                self.failed.insert(id, Instant::now());
            }
        }
    }

    fn fetch(&mut self, url: &str) -> Result<Vec<u8>, NetError> {
        self.last_request = Some(Instant::now());
        potions::fetch(url)
    }
}

/// A monster's drops, and its page's reference rates when the page answered.
struct Fetched {
    drops: Vec<Drop>,
    rates: Option<Vec<(u32, f64)>>,
}

/// One monster's drops from the site, saved as it answered, then (`gap` later) the rates on its page, saved as read
/// (not the page). The drops count without the rates: a missing page is asked again at the next gathering.
fn fetch_monster(id: u32, gap: Duration) -> Result<Fetched, NetError> {
    let body = potions::fetch(&meowdb::drops_url(id))?;
    let drops = meowdb::parse_drops(&String::from_utf8_lossy(&body)).map_err(NetError)?;
    save(&dir().join("drops").join(format!("{id}.json")), &body);
    std::thread::sleep(gap);
    let rates = match potions::fetch(&meowdb::monster_url(id)) {
        Ok(page) => {
            let rates = meowdb::rates_from_page(&String::from_utf8_lossy(&page));
            save(&dir().join("rates").join(format!("{id}.json")), meowdb::rates_to_json(&rates).as_bytes());
            Some(rates)
        }
        Err(error) => {
            crate::warn!("map drops: the rates on monster {id}'s page are unavailable ({error})");
            None
        }
    };
    Ok(Fetched { drops, rates })
}

/// Every `<monster id>.json` in a cache folder, read by `parse`, and the IDs of those saved within the week.
fn read_folder<T>(name: &str, parse: impl Fn(&str) -> Option<T>) -> (HashMap<u32, T>, HashSet<u32>) {
    let (mut read, mut fresh) = (HashMap::new(), HashSet::new());
    for entry in std::fs::read_dir(dir().join(name)).into_iter().flatten().flatten() {
        let path = entry.path();
        let id = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok());
        let parsed = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t));
        if let (Some(id), Some(value)) = (id, parsed) {
            if is_fresh(&path) {
                fresh.insert(id);
            }
            read.insert(id, value);
        }
    }
    (read, fresh)
}

fn dir() -> PathBuf {
    cache_dir().join("meowdb")
}

fn is_fresh(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.elapsed().ok())
        .is_some_and(|age| age < FRESH)
}

fn save(path: &Path, bytes: &[u8]) {
    if let Err(error) = paths::write_atomic(path, bytes) {
        crate::debug!("could not cache {}: {error}", path.display());
    }
}

/// Icons arrive a map's worth at a time: each batch is downloaded in parallel.
fn spawn_icons(folder: PathBuf, ready: Arc<AtomicUsize>) -> Option<Sender<u32>> {
    let (tx, rx) = mpsc::channel::<u32>();
    let started = std::thread::Builder::new().name("drop-icons".into()).spawn(move || {
        while let Ok(first) = rx.recv() {
            let batch: Vec<u32> = std::iter::once(first).chain(rx.try_iter()).collect();
            in_parallel(&batch, |&id| {
                if download_icon(&folder, "item", id, |_, id| potions::fetch(&item_icon_url(u64::from(id)))) {
                    ready.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    match started {
        Ok(_) => Some(tx),
        Err(error) => {
            crate::warn!("could not start the drop icon download: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use maplelens_core::drops::meowdb;
    use maplelens_core::mapinfo::Mob;

    /// NiaDB's real monster list and one monster's drops and page, through the parsers (three requests):
    /// `cargo test -p maplelens live_meowdb -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the network"]
    fn live_meowdb() {
        let page = crate::potions::fetch(&meowdb::monsters_url()).expect("the monster page");
        let monsters = meowdb::monsters_from_page(&String::from_utf8_lossy(&page));
        println!("{} monsters", monsters.len());
        assert!(monsters.len() > 100, "the list is read off the page");
        let mob = Mob { id: 1_210_102, name: "Orange Mushroom".into(), level: 8, exp: 15, hp: 80, count: 1 };
        let monster = meowdb::find(&monsters, &mob).expect("the Orange Mushroom is listed");
        std::thread::sleep(super::REQUEST_GAP);
        let body = crate::potions::fetch(&meowdb::drops_url(monster.id)).expect("its drops");
        let drops = meowdb::parse_drops(&String::from_utf8_lossy(&body)).expect("readable");
        println!("{} (site id {}): {:?}", monster.name, monster.id, drops.iter().map(|d| &d.name).collect::<Vec<_>>());
        assert!(drops.iter().any(|d| d.name == "Orange Mushroom Cap"));
        std::thread::sleep(super::REQUEST_GAP);
        let page = crate::potions::fetch(&meowdb::monster_url(monster.id)).expect("its page");
        let rates = meowdb::rates_from_page(&String::from_utf8_lossy(&page));
        let drops = meowdb::with_rates(drops, &rates);
        for drop in drops.iter().filter(|d| d.rate.is_some()) {
            println!("  {} {:?}", drop.name, drop.rate.map(maplelens_core::drops::one_in_text));
        }
        assert!(!rates.is_empty(), "the page has its reference rates");
        assert!(drops.iter().any(|d| d.name == "Orange Mushroom Cap" && d.rate.is_some_and(|r| r > 0.1)));
    }
}
