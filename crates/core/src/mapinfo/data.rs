//! maplestory.io responses and the on-disk map cache, as plain data. The cache files keep the 0.1.x layout, so
//! maps the Python version already looked up are reused.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::resolve::Candidate;
use super::{MapInfo, Mob, Npc};

/// Bump when MapInfo changes shape: older files are then ignored and fetched again.
const STORE_VERSION: u64 = 2;
/// The data set carries old seasonal events; their NPCs go last.
const SEASONAL: [&str; 9] =
    ["event", "thanksgiving", "easter", "christmas", "halloween", "valentine", "holiday", "santa", "pie advocate"];

fn parse(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("bad JSON: {e}"))
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(|s| s.trim().to_string())
}

fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

/// `map?searchFor=...` results.
pub fn parse_search(bytes: &[u8]) -> Result<Vec<Candidate>, String> {
    let raw = parse(bytes)?;
    let list = raw.as_array().ok_or("search result is not a list")?;
    Ok(list
        .iter()
        .filter_map(|c| {
            Some(Candidate {
                id: number(c, "id")? as u32,
                name: text(c, "name")?,
                street: text(c, "streetName").unwrap_or_default(),
            })
        })
        .collect())
}

/// `map/{id}` before its monsters and NPCs are looked up one by one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapOutline {
    pub name: String,
    pub street: String,
    pub is_town: bool,
    /// (monster id, spawn points), most spawns first (ties in map order).
    pub mobs: Vec<(u32, u32)>,
    /// NPC ids in map order, each once.
    pub npcs: Vec<u32>,
}

pub fn parse_map(bytes: &[u8]) -> Result<MapOutline, String> {
    let raw = parse(bytes)?;
    let ids = |key: &str| -> Result<Vec<u32>, String> {
        let list = raw.get(key).and_then(Value::as_array).ok_or(format!("map without {key}"))?;
        list.iter().map(|e| number(e, "id").map(|id| id as u32).ok_or(format!("{key} entry without id"))).collect()
    };
    let mut mobs: Vec<(u32, u32)> = Vec::new();
    for id in ids("mobs")? {
        match mobs.iter_mut().find(|(m, _)| *m == id) {
            Some((_, count)) => *count += 1,
            None => mobs.push((id, 1)),
        }
    }
    mobs.sort_by_key(|&(_, count)| std::cmp::Reverse(count)); // stable: ties keep map order
    let mut npcs: Vec<u32> = Vec::new();
    for id in ids("npcs")? {
        if !npcs.contains(&id) {
            npcs.push(id);
        }
    }
    Ok(MapOutline {
        name: text(&raw, "name").ok_or("map without name")?,
        street: text(&raw, "streetName").unwrap_or_default(),
        is_town: raw.get("isTown").and_then(Value::as_bool).unwrap_or(false),
        mobs,
        npcs,
    })
}

/// `mob/{id}`; None for entries without stats (event-only ids exist in some data sets).
pub fn parse_mob(bytes: &[u8], id: u32, count: u32) -> Option<Mob> {
    let raw = parse(bytes).ok()?;
    let meta = raw.get("meta")?;
    Some(Mob {
        id,
        name: text(&raw, "name")?,
        level: number(meta, "level")? as u32,
        exp: number(meta, "exp")?,
        hp: number(meta, "maxHP")?,
        count,
    })
}

/// `npc/{id}`; None for nameless ones.
pub fn parse_npc(bytes: &[u8], id: u32) -> Option<Npc> {
    let raw = parse(bytes).ok()?;
    let name = text(&raw, "name").filter(|n| !n.is_empty())?;
    Some(Npc { id, name, role: text(&raw, "function").unwrap_or_default() })
}

/// Useful NPCs first: ones with a role, seasonal-event ones last.
fn npc_rank(npc: &Npc) -> (bool, bool) {
    let role = npc.role.to_lowercase();
    (SEASONAL.iter().any(|w| role.contains(w)), npc.role.is_empty())
}

/// Monster IDs the game keeps for event-only monsters. The classic-era data still places some on ordinary maps (a
/// level 20 "King Slime", 9500168, on Henesys Hunting Ground I) where they do not spawn: the map helper's card listed
/// it, and the drops panel, matching by name, showed the real King Slime's drops for it.
const EVENT_MOBS: std::ops::RangeInclusive<u32> = 9_500_000..=9_599_999;

pub fn is_event_mob(id: u32) -> bool {
    EVENT_MOBS.contains(&id)
}

pub fn assemble(id: u32, outline: MapOutline, mut mobs: Vec<Mob>, mut npcs: Vec<Npc>) -> MapInfo {
    mobs.retain(|m| !is_event_mob(m.id));
    npcs.sort_by_key(npc_rank);
    MapInfo { id, name: outline.name, street: outline.street, is_town: outline.is_town, mobs, npcs }
}

pub fn map_to_json(info: &MapInfo) -> String {
    let mobs: Vec<Value> = info
        .mobs
        .iter()
        .map(|m| json!({"id": m.id, "name": m.name, "level": m.level, "exp": m.exp, "hp": m.hp, "count": m.count}))
        .collect();
    let npcs: Vec<Value> = info.npcs.iter().map(|n| json!({"id": n.id, "name": n.name, "role": n.role})).collect();
    json!({"version": STORE_VERSION, "id": info.id, "name": info.name, "street": info.street,
           "is_town": info.is_town, "mobs": mobs, "npcs": npcs})
    .to_string()
}

/// A cached map; None for another version or a damaged file (it is then fetched again).
pub fn map_from_json(file: &str) -> Option<MapInfo> {
    let raw: Value = serde_json::from_str(file).ok()?;
    if number(&raw, "version")? != STORE_VERSION {
        return None;
    }
    let mobs = raw.get("mobs")?.as_array()?.iter().map(|m| {
        Some(Mob {
            id: number(m, "id")? as u32,
            name: text(m, "name")?,
            level: number(m, "level")? as u32,
            exp: number(m, "exp")?,
            hp: number(m, "hp")?,
            count: number(m, "count")? as u32,
        })
    });
    let npcs = raw
        .get("npcs")?
        .as_array()?
        .iter()
        .map(|n| Some(Npc { id: number(n, "id")? as u32, name: text(n, "name")?, role: text(n, "role")? }));
    Some(MapInfo {
        id: number(&raw, "id")? as u32,
        name: text(&raw, "name")?,
        street: text(&raw, "street")?,
        is_town: raw.get("is_town")?.as_bool()?,
        // Maps cached before event monsters were left out lose them here.
        mobs: mobs.collect::<Option<Vec<Mob>>>()?.into_iter().filter(|m| !is_event_mob(m.id)).collect(),
        npcs: npcs.collect::<Option<_>>()?,
    })
}

/// `index.json`: title reading key -> map id.
pub fn index_from_json(file: &str) -> HashMap<String, u32> {
    let Ok(Value::Object(raw)) = serde_json::from_str::<Value>(file) else { return HashMap::new() };
    raw.into_iter().filter_map(|(k, v)| Some((k, v.as_u64()? as u32))).collect()
}

pub fn index_to_json(index: &HashMap<String, u32>) -> String {
    serde_json::to_string(index).unwrap_or_default()
}

/// `catalog.json`: every map's id, name and street, as `parse_search` reads them.
pub fn catalog_to_json(catalog: &[Candidate]) -> String {
    let list: Vec<Value> =
        catalog.iter().map(|c| json!({"id": c.id, "name": c.name, "streetName": c.street})).collect();
    Value::Array(list).to_string()
}

pub fn catalog_from_json(file: &str) -> Vec<Candidate> {
    parse_search(file.as_bytes()).unwrap_or_default()
}
