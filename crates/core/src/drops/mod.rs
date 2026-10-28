//! Map drops: what the monsters of the map being played drop, coloured by rarity and priced, for the panel beside
//! the EXP card. The map's monsters come from the map helper's data (`mapinfo`); the drop lists from NiaDB
//! (`meowdb`, fetched by the platform), the prices from the market catalog.

pub mod meowdb;
pub mod sample;

use std::collections::HashMap;
use std::sync::Arc;

use crate::mapinfo::MapInfo;
use crate::market::models::Item;

/// How hard a drop is to come by, from the game's loot colours: grey, green, blue, purple, orange.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    VeryRare,
    Legendary,
}

impl Rarity {
    /// By the chance of one kill dropping it: 1 in 10, 100, 1,000 and 10,000 are where players start to call a drop
    /// uncommon, rare and so on.
    pub fn of_chance(chance: f64) -> Self {
        match chance {
            c if c >= 0.1 => Self::Common,
            c if c >= 0.01 => Self::Uncommon,
            c if c >= 0.001 => Self::Rare,
            c if c >= 0.0001 => Self::VeryRare,
            _ => Self::Legendary,
        }
    }

    /// Without a known chance, by market price: the market prices scarcity, so an expensive drop is a rare one.
    pub fn of_price(price: f64) -> Self {
        match price {
            p if p < 1_000.0 => Self::Common,
            p if p < 10_000.0 => Self::Uncommon,
            p if p < 100_000.0 => Self::Rare,
            p if p < 1_000_000.0 => Self::VeryRare,
            _ => Self::Legendary,
        }
    }

    /// The chance when it is known, else the price, else common.
    pub fn of(chance: Option<f64>, price: Option<f64>) -> Self {
        chance.map(Self::of_chance).or_else(|| price.map(Self::of_price)).unwrap_or(Self::Common)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::VeryRare => "Very rare",
            Self::Legendary => "Legendary",
        }
    }
}

/// 0.38 -> "38%", 0.015 -> "1.5%", 0.0003 -> "0.03%": two significant digits at most, as drop lists write them.
pub fn chance_text(chance: f64) -> String {
    let percent = chance * 100.0;
    let decimals = if percent >= 10.0 { 0 } else { (1.0 - percent.log10().floor()).max(1.0) as usize };
    let text = format!("{percent:.decimals$}");
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
    format!("{text}%")
}

/// The line under a hovered drop's name: "Very rare drop · 0.03% a kill · Orange Mushroom", "Rare by price · Slime"
/// when only the price tells its rarity, "Dropped by Stump" when neither is known.
pub fn note(drop: &Shown, mob: &str) -> String {
    match (drop.chance, drop.price) {
        (Some(chance), _) => format!("{} drop · {} a kill · {mob}", drop.rarity.name(), chance_text(chance)),
        (None, Some(_)) => format!("{} by price · {mob}", drop.rarity.name()),
        (None, None) => format!("Dropped by {mob}"),
    }
}

/// One item a monster drops.
#[derive(Clone, Debug, PartialEq)]
pub struct Drop {
    /// The game's item ID.
    pub item: u32,
    pub name: String,
    /// Per kill, 0..1, when the source knows it.
    pub chance: Option<f64>,
}

/// One monster's drops and the credit its source asks for ("NiaDB · meowdb.com/msclassic/monsters/9").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listed {
    pub drops: Vec<Drop>,
    pub credit: String,
}

/// What monsters drop, by the game's monster ID, and the credit for the whole list (shown under the panel).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DropTable {
    pub credit: String,
    mobs: HashMap<u32, Listed>,
}

impl DropTable {
    pub fn new(credit: &str) -> Self {
        Self { credit: credit.to_string(), mobs: HashMap::new() }
    }

    pub fn insert(&mut self, mob: u32, listed: Listed) {
        self.mobs.insert(mob, listed);
    }

    pub fn of(&self, mob: u32) -> Option<&Listed> {
        self.mobs.get(&mob)
    }

    pub fn is_empty(&self) -> bool {
        self.mobs.is_empty()
    }
}

/// The market's items by game item ID: a drop's price, and its price card when it is hovered.
pub fn items_by_id<'a>(items: impl IntoIterator<Item = &'a Arc<Item>>) -> HashMap<u32, Arc<Item>> {
    items.into_iter().filter_map(|item| Some((u32::try_from(item.game_id()?).ok()?, item.clone()))).collect()
}

/// Worth a slot in the list: uncommon or better (by chance, else a price of 1k and up). The rest, potions and monster
/// parts, show on the monster's page.
pub fn is_worthy(drop: &Shown) -> bool {
    drop.rarity >= Rarity::Uncommon
}

/// A drop as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub item: u32,
    pub name: String,
    pub chance: Option<f64>,
    pub price: Option<f64>,
    pub rarity: Rarity,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MobDrops {
    pub id: u32,
    pub name: String,
    pub level: u32,
    pub hp: u64,
    pub exp: u64,
    /// Spawn points on the map.
    pub count: u32,
    /// Rarest first; among equals the dearer first.
    pub drops: Vec<Shown>,
    /// Where its drop list comes from, as the source asks to be credited.
    pub credit: String,
}

impl MobDrops {
    /// How many of its drops are worth showing at a glance: they come first (rarest first).
    pub fn worthy(&self) -> usize {
        self.drops.iter().take_while(|d| is_worthy(d)).count()
    }

    /// Its dearest drop's price.
    pub fn best_price(&self) -> Option<f64> {
        self.drops.iter().filter_map(|d| d.price).max_by(f64::total_cmp)
    }
}

/// The panel's content for one map.
#[derive(Clone, Debug, PartialEq)]
pub struct MapDrops {
    pub map: String,
    pub levels: Option<(u32, u32)>,
    /// The map's monsters with known drops, most spawns first.
    pub mobs: Vec<MobDrops>,
    /// Where the drop lists come from.
    pub credit: String,
}

impl MapDrops {
    /// The dearest drop on the map (the rarer of two at one price).
    pub fn best(&self) -> Option<&Shown> {
        let priced = self.mobs.iter().flat_map(|m| &m.drops).filter(|d| d.price.is_some());
        priced.max_by(|a, b| a.price.unwrap_or(0.0).total_cmp(&b.price.unwrap_or(0.0)).then(a.rarity.cmp(&b.rarity)))
    }

    /// The map's worthy drops, each item once, as (monster, drop) of the first monster listing it: rarest first, then
    /// dearest.
    pub fn rare(&self) -> Vec<(usize, usize)> {
        let mut seen = std::collections::HashSet::new();
        let mut rare: Vec<(usize, usize)> = self
            .mobs
            .iter()
            .enumerate()
            .flat_map(|(m, mob)| (0..mob.worthy()).map(move |d| (m, d)))
            .filter(|&(m, d)| seen.insert(self.mobs[m].drops[d].item))
            .collect();
        let shown = |&(m, d): &(usize, usize)| &self.mobs[m].drops[d];
        rare.sort_by(|a, b| {
            let (a, b) = (shown(a), shown(b));
            let dearer = b.price.unwrap_or(-1.0).total_cmp(&a.price.unwrap_or(-1.0));
            b.rarity.cmp(&a.rarity).then(dearer)
        });
        rare
    }

    /// The map's `n` dearest worthy drops (the rarer of two at one price; unpriced ones last).
    pub fn top(&self, n: usize) -> Vec<(usize, usize)> {
        let mut top = self.rare();
        let shown = |&(m, d): &(usize, usize)| &self.mobs[m].drops[d];
        top.sort_by(|a, b| {
            let (a, b) = (shown(a), shown(b));
            b.price.unwrap_or(-1.0).total_cmp(&a.price.unwrap_or(-1.0)).then(b.rarity.cmp(&a.rarity))
        });
        top.truncate(n);
        top
    }

    /// The names of the map's monsters that drop `item`.
    pub fn droppers(&self, item: u32) -> Vec<&str> {
        self.mobs.iter().filter(|m| m.drops.iter().any(|d| d.item == item)).map(|m| m.name.as_str()).collect()
    }
}

/// What `info`'s monsters drop, or None in a town or when no monster's drops are known (there is nothing to show).
pub fn assemble(info: &MapInfo, table: &DropTable, price: impl Fn(u32) -> Option<f64>) -> Option<MapDrops> {
    if info.is_town {
        return None;
    }
    let mobs: Vec<MobDrops> = info
        .mobs
        .iter()
        .filter_map(|mob| {
            let listed = table.of(mob.id)?;
            let mut drops: Vec<Shown> = listed
                .drops
                .iter()
                .map(|d| {
                    let price = price(d.item);
                    Shown {
                        item: d.item,
                        name: d.name.clone(),
                        chance: d.chance,
                        price,
                        rarity: Rarity::of(d.chance, price),
                    }
                })
                .collect();
            drops.sort_by(|a, b| {
                let dearer = b.price.unwrap_or(-1.0).total_cmp(&a.price.unwrap_or(-1.0));
                b.rarity.cmp(&a.rarity).then(dearer).then(a.name.cmp(&b.name))
            });
            (!drops.is_empty()).then(|| MobDrops {
                id: mob.id,
                name: mob.name.clone(),
                level: mob.level,
                hp: mob.hp,
                exp: mob.exp,
                count: mob.count,
                drops,
                credit: listed.credit.clone(),
            })
        })
        .collect();
    (!mobs.is_empty()).then(|| MapDrops {
        map: info.name.clone(),
        levels: info.levels(),
        mobs,
        credit: table.credit.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapinfo::Mob;

    fn mob(id: u32, name: &str, level: u32, count: u32) -> Mob {
        Mob { id, name: name.into(), level, exp: 10, hp: 50, count }
    }

    fn field(mobs: Vec<Mob>) -> MapInfo {
        MapInfo { id: 1, name: "Field".into(), street: "Road".into(), is_town: false, mobs, npcs: Vec::new() }
    }

    #[test]
    fn rarity_follows_the_chance_before_the_price() {
        assert_eq!(Rarity::of_chance(0.4), Rarity::Common);
        assert_eq!(Rarity::of_chance(0.015), Rarity::Uncommon);
        assert_eq!(Rarity::of_chance(0.004), Rarity::Rare);
        assert_eq!(Rarity::of_chance(0.0003), Rarity::VeryRare);
        assert_eq!(Rarity::of_chance(0.00005), Rarity::Legendary);
        assert_eq!(Rarity::of_price(2_400_000.0), Rarity::Legendary);
        assert_eq!(Rarity::of_price(45.0), Rarity::Common);
        // A cheap item that rarely drops is rare all the same.
        assert_eq!(Rarity::of(Some(0.0003), Some(45.0)), Rarity::VeryRare);
        assert_eq!(Rarity::of(None, Some(48_000.0)), Rarity::Rare);
        assert_eq!(Rarity::of(None, None), Rarity::Common);
    }

    #[test]
    fn chances_read_as_drop_lists_write_them() {
        let texts: Vec<String> = [0.38, 0.1, 0.015, 0.01, 0.004, 0.0003, 0.00005, 1.0].map(chance_text).into();
        assert_eq!(texts, ["38%", "10%", "1.5%", "1%", "0.4%", "0.03%", "0.005%", "100%"]);
        let drop =
            Shown { item: 1, name: "Scroll".into(), chance: Some(0.0003), price: None, rarity: Rarity::VeryRare };
        assert_eq!(note(&drop, "Orange Mushroom"), "Very rare drop · 0.03% a kill · Orange Mushroom");
        let priced = Shown { chance: None, price: Some(85_000.0), rarity: Rarity::Rare, ..drop };
        assert_eq!(note(&priced, "Slime"), "Rare by price · Slime");
        assert_eq!(note(&Shown { price: None, ..priced }, "Stump"), "Dropped by Stump");
    }

    #[test]
    fn drops_go_rarest_first_and_mobs_without_drops_are_left_out() {
        let mut table = DropTable::new("Source");
        let drops = vec![
            Drop { item: 10, name: "Cap".into(), chance: Some(0.4) },
            Drop { item: 11, name: "Scroll".into(), chance: Some(0.0003) },
            Drop { item: 12, name: "Coif".into(), chance: Some(0.015) },
        ];
        table.insert(1, Listed { drops, credit: "Source · page 1".into() });
        let info = field(vec![mob(1, "Orange Mushroom", 8, 12), mob(2, "Unknown", 9, 3)]);
        let price = |id| HashMap::from([(10, 9.0), (11, 2_400_000.0)]).get(&id).copied();
        let drops = assemble(&info, &table, price).expect("a mob with drops");
        assert_eq!(drops.mobs.len(), 1);
        let names: Vec<&str> = drops.mobs[0].drops.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Scroll", "Coif", "Cap"]);
        assert_eq!(drops.mobs[0].best_price(), Some(2_400_000.0));
        assert_eq!(drops.mobs[0].worthy(), 2, "the cap is common");
        assert_eq!(drops.best().map(|d| d.item), Some(11));
        assert_eq!(drops.levels, Some((8, 9)));
        assert_eq!((drops.credit.as_str(), drops.mobs[0].credit.as_str()), ("Source", "Source · page 1"));
    }

    #[test]
    fn towns_and_maps_without_known_drops_show_nothing() {
        let table = sample::table();
        let mut town = sample::map();
        town.is_town = true;
        assert!(assemble(&town, &table, |_| None).is_none());
        assert!(assemble(&field(vec![mob(42, "Nobody", 1, 1)]), &table, |_| None).is_none());
    }

    #[test]
    fn items_are_keyed_by_the_game_id() {
        let cap = Arc::new(Item { item_id: "basil:04000001".into(), ..Item::named("Orange Mushroom Cap") });
        let unnumbered = Arc::new(Item::named("Demo"));
        let by_id = items_by_id([&cap, &unnumbered]);
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[&4_000_001].name, "Orange Mushroom Cap");
    }
}
