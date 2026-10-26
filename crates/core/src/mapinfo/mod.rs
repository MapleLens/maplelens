//! The map helper's data: what a world-map tooltip names, and the monsters and NPCs of that map (maplestory.io,
//! classic-era data). Network and disk live in the platform crates; parsing, matching and voting are here.

pub mod data;
pub mod resolve;
pub mod vote;

/// A world-map tooltip title, "Victoria Road : Henesys" (street "" when OCR lost the colon).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub street: String,
    pub name: String,
}

impl Location {
    pub fn new(street: &str, name: &str) -> Self {
        Self { street: street.into(), name: name.into() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mob {
    pub id: u32,
    pub name: String,
    pub level: u32,
    pub exp: u64,
    pub hp: u64,
    /// Spawn points of this monster on the map.
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Npc {
    pub id: u32,
    pub name: String,
    /// "Bowman Instructor", "" when it only talks.
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapInfo {
    pub id: u32,
    pub name: String,
    pub street: String,
    pub is_town: bool,
    /// Most spawns first.
    pub mobs: Vec<Mob>,
    pub npcs: Vec<Npc>,
}

impl MapInfo {
    pub fn spawns(&self) -> u32 {
        self.mobs.iter().map(|m| m.count).sum()
    }

    pub fn levels(&self) -> Option<(u32, u32)> {
        let low = self.mobs.iter().map(|m| m.level).min()?;
        Some((low, self.mobs.iter().map(|m| m.level).max()?))
    }
}
