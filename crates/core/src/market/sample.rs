//! Built-in demo items so the overlay works before any price data is available. Illustrative numbers only.

use super::models::{Day, Item, NpcShop};

fn week(medians: [f64; 7], sold: [i64; 7]) -> Vec<Day> {
    medians
        .iter()
        .zip(sold)
        .enumerate()
        .map(|(i, (&m, s))| Day {
            date: format!("2026-10-{}", 15 + i),
            median: m,
            low: m * 0.9,
            high: m * 1.15,
            sold: s,
        })
        .collect()
}

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

pub fn sample_items() -> Vec<Item> {
    vec![
        Item {
            category: s("Etc"),
            price: Some(100.0),
            price_basis: s("24h"),
            price_sales: 61,
            p25: Some(99.0),
            p75: Some(119.0),
            low: Some(40.0),
            high: Some(900.0),
            price_all: Some(121.0),
            last3_avg: Some(104.0),
            trend: Some(0.042),
            sold: 1000,
            top_sale: Some(900.0),
            mesos_traded: 151_000.0,
            last_sold: s("2026-10-21T20:06:20Z"),
            first_sold: s("2026-10-07T10:00:00Z"),
            ask_median: Some(118.0),
            asks: 640,
            listing_shops: 96,
            sell_through: Some(0.31),
            npc: Some(NpcShop { buy: None, sell: Some(2.0), shop: None, sellers: 0 }),
            history: week([92.0, 95.0, 99.0, 97.0, 103.0, 98.0, 100.0], [120, 90, 140, 110, 160, 130, 150]),
            obtain: vec!["mob".into()],
            ..Item::named("Emerald Ore")
        },
        Item {
            category: s("Etc"),
            price: Some(20.0),
            price_basis: s("24h"),
            price_sales: 90,
            p25: Some(19.0),
            p75: Some(25.0),
            low: Some(5.0),
            high: Some(60.0),
            price_all: Some(24.0),
            trend: Some(-0.03),
            sold: 1208,
            ask_median: Some(30.0),
            asks: 900,
            listing_shops: 140,
            sell_through: Some(0.12),
            last_sold: s("2026-10-21T12:46:30Z"),
            first_sold: s("2026-10-07T10:00:00Z"),
            history: week([22.0, 21.0, 21.0, 20.0, 20.0, 19.0, 20.0], [80, 95, 70, 85, 110, 90, 100]),
            ..Item::named("Tree Branch")
        },
    ]
}
