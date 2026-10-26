//! maplestory.io: map search, map, monster and NPC data, and their sprites (public, no key).

use maplelens_core::config;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use maplelens_core::mapinfo::data::{assemble, parse_map, parse_mob, parse_npc, parse_search};
use maplelens_core::mapinfo::resolve::{Candidate, pick, search_terms};
use maplelens_core::mapinfo::{Location, MapInfo, Mob, Npc};

use crate::net::{NetError, agent};

/// One request per monster and NPC: sequentially a map takes seconds.
pub const PARALLEL: usize = 8;
/// More than the data set has maps (about 1,100 in the classic era): the whole list in one request. A round
/// number: the CDN answered 5000 in 20 s once and 2000 from cache in 50 ms.
const CATALOG_COUNT: u32 = 2000;
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// The server answers 503 now and then (and times out under load): such answers are asked again before they count.
const ATTEMPTS: u64 = 3;
const RETRY_PAUSE_MS: u64 = 400;

fn get(path: &str) -> Result<Vec<u8>, NetError> {
    let mut last = NetError(String::new());
    for attempt in 1..=ATTEMPTS {
        match get_once(path) {
            Ok(bytes) => return Ok(bytes),
            // A missing page stays missing; anything else may be the server's moment.
            Err((error, true)) => return Err(error),
            Err((error, false)) => last = error,
        }
        std::thread::sleep(std::time::Duration::from_millis(RETRY_PAUSE_MS * attempt));
    }
    Err(last)
}

/// One request; the flag says the answer is final (a client error, not worth asking again).
fn get_once(path: &str) -> Result<Vec<u8>, (NetError, bool)> {
    let url = format!("{}/{path}", config::MAP_API);
    let mut response = agent(config::MAP_REQUEST_TIMEOUT_S)
        .get(&url)
        .call()
        .map_err(|e| (NetError(format!("{e} for {path}")), false))?;
    match response.status().as_u16() {
        200 => response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(|e| (NetError(format!("{e} for {path}")), false)),
        status => Err((NetError(format!("HTTP {status} for {path}")), (400..500).contains(&status) && status != 429)),
    }
}

fn encode(term: &str) -> String {
    term.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Map id for a tooltip reading: search by the whole name, then by its words (one may be misread), and pick from
/// everything found so far.
pub fn find_map_id(location: &Location) -> Result<Option<u32>, NetError> {
    let mut found: Vec<Candidate> = Vec::new();
    for term in search_terms(location) {
        let results = parse_search(&get(&format!("map?searchFor={}&count=100", encode(&term)))?).map_err(NetError)?;
        for candidate in results {
            if !found.iter().any(|c| c.id == candidate.id) {
                found.push(candidate);
            }
        }
        if let Some(id) = pick(location, &found) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// PNG sprite of a monster ("mob") or NPC ("npc").
pub fn fetch_icon(kind: &str, id: u32) -> Result<Vec<u8>, NetError> {
    get(&format!("{kind}/{id}/icon"))
}

/// Every map's id, name and street: one request, so a tooltip reading resolves locally instead of by searches.
pub fn fetch_catalog() -> Result<Vec<Candidate>, NetError> {
    parse_search(&get(&format!("map?count={CATALOG_COUNT}"))?).map_err(NetError)
}

/// Monsters and NPCs recur across maps (a Snail lives on a dozen): each is requested once per run. Only answers
/// the server gave are kept, so a network failure is asked again.
#[derive(Default)]
struct Seen {
    mobs: HashMap<u32, Option<Mob>>,
    npcs: HashMap<u32, Option<Npc>>,
}

fn seen() -> &'static Mutex<Seen> {
    static SEEN: OnceLock<Mutex<Seen>> = OnceLock::new();
    SEEN.get_or_init(Mutex::default)
}

/// A monster (None when the data set has no stats for it: event-only ids), or the network's failure.
fn mob(id: u32, count: u32) -> Result<Option<Mob>, NetError> {
    let known = seen().lock().unwrap_or_else(|e| e.into_inner()).mobs.get(&id).cloned();
    let mob = match known {
        Some(mob) => mob,
        None => {
            let mob = parse_mob(&get(&format!("mob/{id}"))?, id, 0);
            seen().lock().unwrap_or_else(|e| e.into_inner()).mobs.insert(id, mob.clone());
            mob
        }
    };
    Ok(mob.map(|m| Mob { count, ..m }))
}

fn npc(id: u32) -> Result<Option<Npc>, NetError> {
    let known = seen().lock().unwrap_or_else(|e| e.into_inner()).npcs.get(&id).cloned();
    match known {
        Some(npc) => Ok(npc),
        None => {
            let npc = parse_npc(&get(&format!("npc/{id}"))?, id);
            seen().lock().unwrap_or_else(|e| e.into_inner()).npcs.insert(id, npc.clone());
            Ok(npc)
        }
    }
}

/// The map with its monsters and NPCs; stats and roles are separate requests, made a few at a time. One that
/// still fails after its retries fails the map: a card (and a disk cache entry) missing a monster is worse than a
/// lookup made again later.
pub fn fetch_map(id: u32) -> Result<MapInfo, NetError> {
    let outline = parse_map(&get(&format!("map/{id}"))?).map_err(NetError)?;
    let mobs = in_parallel(&outline.mobs, |&(id, count)| mob(id, count)).into_iter().collect::<Result<Vec<_>, _>>()?;
    let npcs = in_parallel(&outline.npcs, |&id| npc(id)).into_iter().collect::<Result<Vec<_>, _>>()?;
    Ok(assemble(id, outline, mobs.into_iter().flatten().collect(), npcs.into_iter().flatten().collect()))
}

/// `f` over `items`, PARALLEL at a time (a worker takes the next item as soon as it is free, so one slow request
/// holds up nothing); the results in the items' order.
pub fn in_parallel<T: Sync, U: Send>(items: &[T], f: impl Fn(&T) -> U + Sync) -> Vec<U> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<U>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..PARALLEL.min(items.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { return };
                    *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(f(item));
                }
            });
        }
    });
    slots.into_iter().filter_map(|slot| slot.into_inner().unwrap_or_else(|e| e.into_inner())).collect()
}
