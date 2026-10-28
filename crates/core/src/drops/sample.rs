//! A hand-written field and drop table for tests and sample cards. Illustrative numbers only.

use std::collections::HashMap;

use super::{Drop, DropTable, Listed, meowdb};
use crate::mapinfo::{MapInfo, Mob};

pub fn map() -> MapInfo {
    let mob = |id, name: &str, level, count| Mob { id, name: name.into(), level, exp: level as u64 * 3, hp: 80, count };
    MapInfo {
        id: 104_040_001,
        name: "Henesys Hunting Ground II".into(),
        street: "Victoria Road".into(),
        is_town: false,
        mobs: vec![mob(1_210_102, "Orange Mushroom", 8, 12), mob(210_100, "Slime", 6, 8), mob(130_100, "Stump", 4, 5)],
        npcs: Vec::new(),
    }
}

/// The sample field's drops, credited like NiaDB's (the site's IDs are its own: 9, 7 and 6).
pub fn table() -> DropTable {
    let drop = |item, name: &str, chance| Drop { item, name: name.into(), chance: Some(chance) };
    let lists = [
        (
            1_210_102,
            9,
            vec![
                drop(4_000_001, "Orange Mushroom Cap", 0.38),
                drop(2_000_001, "Orange Potion", 0.12),
                drop(1_002_043, "Bronze Koif", 0.015),
                drop(2_040_001, "Scroll for Helmet for DEF 60%", 0.004),
                drop(2_040_805, "Scroll for Gloves for ATT 60%", 0.0003),
            ],
        ),
        (
            210_100,
            7,
            vec![
                drop(4_000_004, "Slime Bubble", 0.4),
                drop(2_000_000, "Red Potion", 0.15),
                drop(1_040_002, "Blue One-lined T-Shirt", 0.012),
                drop(2_040_702, "Scroll for Shoes for DEX 60%", 0.003),
            ],
        ),
        (
            130_100,
            6,
            vec![
                drop(4_000_003, "Tree Branch", 0.45),
                drop(4_000_018, "Firewood", 0.1),
                drop(1_322_005, "Wooden Club", 0.02),
                drop(2_040_301, "Scroll for Earring for INT 60%", 0.00005),
            ],
        ),
    ];
    let mut table = DropTable::new(&meowdb::panel_credit());
    for (mob, site_id, drops) in lists {
        table.insert(mob, Listed { drops, credit: meowdb::credit(site_id) });
    }
    table
}

/// Market prices for some of the sample drops.
pub fn prices() -> HashMap<u32, f64> {
    HashMap::from([
        (4_000_001, 9.0),
        (2_000_001, 45.0),
        (1_002_043, 3_200.0),
        (2_040_001, 48_000.0),
        (2_040_805, 2_400_000.0),
        (4_000_004, 6.0),
        (2_000_000, 20.0),
        (1_040_002, 1_500.0),
        (2_040_702, 85_000.0),
        (4_000_003, 4.0),
        (1_322_005, 900.0),
        (2_040_301, 310_000.0),
    ])
}
