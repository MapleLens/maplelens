//! Catalog lookup, number formatting, insights and snapshot parsing.

mod common;

use maplelens_core::config;
use maplelens_core::market::catalog::{Catalog, clean_title, ratio};
use maplelens_core::market::format::{format_ago, format_mesos, group_thousands};
use maplelens_core::market::insights::{self, Pace};
use maplelens_core::market::models::{Craft, CraftPart, Day, Item, NpcShop};
use maplelens_core::market::snapshot::{SnapshotError, parse_snapshot};
use maplelens_core::market::time::parse_iso;
use maplelens_core::pipeline::hover::tooltip_size;

fn make(name: &str, sold: i64) -> Item {
    Item { category: Some("Etc".into()), price: Some(100.0), sold, ..Item::named(name) }
}

fn catalog() -> Catalog {
    Catalog::new([make("Emerald Ore", 10), make("Tree Branch", 10), make("Red Snail Shell", 10)], None)
}

fn found(catalog: &Catalog, text: &str) -> Option<String> {
    catalog.lookup(text).map(|i| i.name.clone())
}

#[test]
fn exact_match_ignores_case_and_punctuation() {
    assert_eq!(found(&catalog(), "emerald  ORE!").as_deref(), Some("Emerald Ore"));
}

#[test]
fn tolerates_misreads() {
    assert_eq!(found(&catalog(), "Emerald 0re").as_deref(), Some("Emerald Ore"));
    assert_eq!(found(&catalog(), "Dragon Scale"), None);
    assert_eq!(found(&catalog(), "  "), None);
}

#[test]
fn name_collision_keeps_item_with_most_sales() {
    let catalog = Catalog::new([make("Iron Ore", 5), make("Iron Ore", 900), make("Iron Ore", 50)], None);
    assert_eq!(catalog.lookup("Iron Ore").map(|i| i.sold), Some(900));
    assert_eq!(catalog.len(), 1);
}

#[test]
fn equipment_is_recognised_by_item_id_not_category() {
    let cash =
        |name: &str, id: &str| Item { item_id: id.into(), category: Some("Cash Shop".into()), ..Item::named(name) };
    let (pants, coupon, hat) = (
        cash("Lined Hip-Hop Pants", "basil:01067001"),
        cash("Selective 5-slot Coupon", "cash:05680001"),
        cash("Bunny Earmuffs", "cash:01007136"),
    );
    assert!(pants.is_equip() && hat.is_equip() && !coupon.is_equip());
    assert_eq!(tooltip_size(Some(&pants), None), config::tooltip_size_for(Some("Equip")));
    assert_eq!(tooltip_size(Some(&hat), None), config::tooltip_size_for(Some("Equip")));
    assert_eq!(tooltip_size(Some(&coupon), None), config::tooltip_size_for(Some("Cash Shop")));
}

#[test]
fn trailing_read_junk_is_ignored() {
    assert_eq!(found(&catalog(), "Emerald Ore ii").as_deref(), Some("Emerald Ore"));
    assert_eq!(clean_title("Kitty Camping Hat ii"), "Kitty Camping Hat");
    assert_eq!(clean_title("Hat Accuracy Scroll: Greater"), "Hat Accuracy Scroll: Greater");
}

#[test]
fn tooltip_size_uses_the_measured_width() {
    // Unknown item, narrow tooltip measured: equipment size, never the wide default that misplaced the card.
    assert_eq!(tooltip_size(None, Some(370)), (370, config::tooltip_size_for(Some("Equip")).1));
    assert_eq!(tooltip_size(None, Some(450)).0, 450);
    assert_eq!(tooltip_size(None, None), config::tooltip_size_for(Some("Etc")));
}

#[test]
fn confused_glyphs_still_match() {
    let catalog = Catalog::new([make("Stiff Feather", 10), make("Tree Branch", 10)], None);
    assert_eq!(found(&catalog, "Stitt Feather").as_deref(), Some("Stiff Feather")); // 'ff' read as 'tt'
    assert_eq!(found(&catalog, "Stiff Feather").as_deref(), Some("Stiff Feather"));
}

#[test]
fn folding_does_not_make_distinct_names_collide_on_exact_match() {
    let catalog = Catalog::new([make("Stiff Feather", 1), make("Stitt Feather", 2)], None);
    assert_eq!(catalog.lookup("Stiff Feather").map(|i| i.sold), Some(1));
}

#[test]
fn unknown_glyphs_are_wildcards() {
    let catalog = Catalog::new(
        [make("Kitty Camping Hat", 1), make("Camping Hat", 1), make("Octopus Leg", 1), make("Octopus Ink", 1)],
        None,
    );
    // '?' stands for one to three letters the glyph atlas has not learnt yet.
    assert_eq!(found(&catalog, "?i? Camping Hat").as_deref(), Some("Kitty Camping Hat"));
    assert_eq!(found(&catalog, "O?topus Leg").as_deref(), Some("Octopus Leg"));
    // Too little known to be sure of anything.
    assert_eq!(found(&catalog, "??? ?? ?"), None);
}

#[test]
fn ratio_is_rapidfuzz_ratio() {
    assert_eq!(ratio("abc", "abc"), 100.0);
    assert!((ratio("emeraldore", "emeraldore") - 100.0).abs() < 1e-9);
    assert!((ratio("stittfeather", "stifffeather") - 2.0 * 10.0 / 24.0 * 100.0).abs() < 1e-9);
}

/// The Python implementation's fuzzy answers (tests/golden/python.json).
#[test]
fn fuzzy_matches_the_python_implementation() {
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(common::repo().join("tests/golden/python.json")).unwrap())
            .unwrap();
    let names = ["Emerald Ore", "Stiff Feather", "Tree Branch", "Red Snail Shell", "Beginner's Garnier", "Octopus Leg"];
    let catalog = Catalog::new(names.map(|n| Item { price: Some(1.0), ..Item::named(n) }), None);
    for (query, expected) in golden["fuzzy"].as_object().unwrap() {
        assert_eq!(found(&catalog, query).as_deref(), expected.as_str(), "{query}");
    }
    for (amount, expected) in golden["format_mesos"].as_object().unwrap() {
        assert_eq!(format_mesos(Some(amount.parse().unwrap())), expected.as_str().unwrap(), "{amount}");
    }
}

#[test]
fn format_mesos_table() {
    let cases: &[(Option<f64>, &str)] = &[
        (None, "?"),
        (Some(6.0), "6"),
        (Some(9.5), "9.5"),
        (Some(119.25), "119"),
        (Some(999.0), "999"),
        (Some(1000.0), "1k"),
        (Some(1320.0), "1.32k"),
        (Some(10500.0), "10.5k"),
        (Some(105000.0), "105k"),
        (Some(150000.0), "150k"),
        (Some(145000.0), "145k"),
        (Some(159999.0), "160k"),
        (Some(80000.0), "80k"),
        (Some(250000.0), "250k"),
        (Some(199999.0), "200k"),
        (Some(100000.0), "100k"),
        (Some(150e6), "150m"),
        (Some(250e9), "250b"),
        (Some(999_999.0), "1m"),
        (Some(2_300_000.0), "2.3m"),
        (Some(1.5e9), "1.5b"),
    ];
    for (amount, expected) in cases {
        assert_eq!(format_mesos(*amount), *expected, "{amount:?}");
    }
}

#[test]
fn format_ago_table() {
    let now = parse_iso("2026-10-22T12:00:00Z").unwrap();
    assert_eq!(format_ago(None, now), "—");
    assert_eq!(format_ago(Some("2026-10-22T11:59:30Z"), now), "just now");
    assert_eq!(format_ago(Some("2026-10-22T11:15:00Z"), now), "45m ago");
    assert_eq!(format_ago(Some("2026-10-22T09:00:00Z"), now), "3h ago");
    assert_eq!(format_ago(Some("2026-10-19T12:00:00Z"), now), "3d ago");
    assert_eq!(format_ago(Some("2026-10-22T09:00:00+00:00"), now), "3h ago");
    assert_eq!(format_ago(Some("2026-10-22T12:00:00+03:00"), now), "3h ago");
}

#[test]
fn thousands_are_grouped() {
    assert_eq!(group_thousands(0), "0");
    assert_eq!(group_thousands(999), "999");
    assert_eq!(group_thousands(1000), "1,000");
    assert_eq!(group_thousands(29516), "29,516");
    assert_eq!(group_thousands(-1234567), "-1,234,567");
}

fn item() -> Item {
    Item::named("X")
}

#[test]
fn insight_numbers() {
    let premium = insights::ask_premium(&Item { price: Some(100.0), ask_median: Some(150.0), ..item() }).unwrap();
    assert!((premium - 0.5).abs() < 1e-9);
    assert_eq!(insights::ask_premium(&Item { price: None, ask_median: Some(150.0), ..item() }), None);
    assert!((insights::range_position(Some(100.0), Some(10.0), Some(1000.0)).unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(insights::range_position(Some(1.0), Some(10.0), Some(1000.0)), Some(0.0));
    assert_eq!(insights::range_position(Some(5000.0), Some(10.0), Some(1000.0)), Some(1.0));
    assert_eq!(insights::range_position(Some(100.0), Some(10.0), Some(10.0)), None);
}

#[test]
fn market_pace() {
    let pace = |s: f64, low: bool| insights::market_pace(&Item { sell_through: Some(s), low_data: low, ..item() });
    assert_eq!(pace(0.6, false), Some(Pace::Fast));
    assert_eq!(pace(0.05, false), Some(Pace::Slow));
    assert_eq!(pace(0.3, false), None);
    assert_eq!(pace(0.6, true), None);
}

#[test]
fn npc_floor_share_and_craft_margin() {
    let npc = Item {
        price: Some(12000.0),
        npc: Some(NpcShop { buy: None, sell: Some(6000.0), shop: None, sellers: 0 }),
        ..item()
    };
    assert!((insights::npc_floor_share(&npc).unwrap() - 0.5).abs() < 1e-9);
    let craft = Craft {
        discipline: "Woodcrafting".into(),
        level: Some(1),
        qty: 500,
        cost: Some(400.0),
        per_item: Some(0.8),
        parts: vec![CraftPart { name: "Wood".into(), count: 1, each: Some(299.5) }],
    };
    let crafted = Item { price: Some(0.5), craft: Some(craft), ..item() };
    assert!((insights::craft_margin(&crafted, None).unwrap() + 0.375).abs() < 1e-9);
}

#[test]
fn volume_bars_scale_to_the_busiest_day() {
    let days = [50, 100, 25]
        .iter()
        .enumerate()
        .map(|(i, &s)| Day { date: format!("2026-10-{}", 15 + i), median: 10.0, low: 9.0, high: 11.0, sold: s })
        .collect();
    assert_eq!(insights::volume_bars(&Item { history: days, ..item() }), vec![0.5, 1.0, 0.25]);
    assert!(insights::volume_bars(&item()).is_empty());
}

const RAW: &str = r#"{
    "name": "Mithril Pole Arm", "category": "Weapons", "level": 30, "classes": ["Warrior"], "unitBasis": "item",
    "price": 12000, "priceBasis": "24h", "priceSales": 43, "p25": 9999, "p75": 14499, "low": 6500, "high": 99999,
    "priceAll": 14943.5, "trend": -0.018, "sold": 716, "lowData": false, "askMedian": 15000, "asks": 2388,
    "sellThrough": 0.231, "listingShops": 305, "mesosTraded": 12694326, "topSale": 99999,
    "last3": {"avg": 26332.6, "n": 3}, "lastSold": "2026-10-21T23:10:47Z", "firstSold": "2026-10-07T14:37:38Z",
    "history": [
        {"date": "2026-10-15", "median": 12999, "low": 10000, "high": 20000, "sold": 35},
        {"date": "2026-10-16", "median": null}
    ],
    "obtain": ["npc", "mob"],
    "npc": {"buy": 26000, "sell": 6500, "sellers": ["River (Perion)"], "shop": "River (Perion)"},
    "craft": {"discipline": "Woodcrafting", "level": 1, "qty": 500, "cost": 400, "perItem": 0.8,
              "parts": [{"name": "Processed Wood", "count": 1, "each": 299.5}]},
    "cash": {"nx": 5000, "onSale": true, "crateOdds": null}
}"#;

#[test]
fn loads_every_field_we_use() {
    let snapshot =
        parse_snapshot(&format!(r#"{{"items": [{RAW}], "generated_at": "2026-10-21T23:45:49+00:00"}}"#)).unwrap();
    let item = &snapshot.items[0];
    assert_eq!(snapshot.generated_at.as_deref(), Some("2026-10-21T23:45:49+00:00"));
    assert_eq!(
        (item.name.as_str(), item.category.as_deref(), item.level, item.classes.clone()),
        ("Mithril Pole Arm", Some("Weapons"), Some(30), vec!["Warrior".to_string()])
    );
    assert_eq!(
        (item.price, item.price_basis.as_deref(), item.price_sales, item.p25, item.p75),
        (Some(12000.0), Some("24h"), 43, Some(9999.0), Some(14499.0))
    );
    assert_eq!(
        (item.low, item.high, item.ask_median, item.asks, item.listing_shops),
        (Some(6500.0), Some(99999.0), Some(15000.0), 2388, 305)
    );
    assert_eq!(item.last3_avg, Some(26332.6));
    assert_eq!(item.medians(), vec![12999.0]); // days without a median are dropped
    let npc = item.npc.as_ref().unwrap();
    assert_eq!((npc.sell, npc.sellers), (Some(6500.0), 1));
    let craft = item.craft.as_ref().unwrap();
    assert_eq!((craft.per_item, craft.parts[0].name.as_str()), (Some(0.8), "Processed Wood"));
    let cash = item.cash.as_ref().unwrap();
    assert_eq!((cash.nx, cash.on_sale), (Some(5000), true));
    assert_eq!(item.obtain, vec!["npc".to_string(), "mob".to_string()]);
}

#[test]
fn unsold_item_has_safe_defaults() {
    let snapshot = parse_snapshot(r#"{"items": [{"name": "Unsold", "price": null}]}"#).unwrap();
    let item = &snapshot.items[0];
    assert!(item.price.is_none() && item.sold == 0 && item.history.is_empty() && item.npc.is_none());
}

#[test]
fn wrong_documents_are_rejected() {
    assert!(matches!(parse_snapshot("{}"), Err(SnapshotError::Invalid(_))));
    assert!(matches!(parse_snapshot("<html>Cloudflare challenge</html>"), Err(SnapshotError::Invalid(_))));
    assert!(matches!(parse_snapshot(r#"{"items": [{"name": null}]}"#), Err(SnapshotError::Incompatible(_))));
}

#[test]
fn a_gender_mark_is_ignored_unless_it_is_part_of_the_name() {
    let item = |name: &str| Item { name: name.into(), ..maplelens_core::market::sample::sample_items()[0].clone() };
    let catalog =
        Catalog::new([item("Undershirt"), item("Natalie's Fashion Box (M)"), item("Natalie's Fashion Box (F)")], None);
    let name = |read: &str| catalog.lookup(read).map(|i| i.name.clone());
    // The mark in front of a name the catalog lists without it, however the parentheses were read.
    for read in ["Undershirt (M)", "Undershirt lM?", "Undershirt IMI", "Undershirt IF)"] {
        assert_eq!(name(read).as_deref(), Some("Undershirt"), "{read}");
    }
    // Two items that differ only by the mark stay apart.
    assert_eq!(name("Natalie's Fashion Box (M)").as_deref(), Some("Natalie's Fashion Box (M)"));
    assert_eq!(name("Natalie's Fashion Box (F)").as_deref(), Some("Natalie's Fashion Box (F)"));
}

#[test]
fn an_enhancement_mark_is_ignored() {
    let item = |name: &str| Item { name: name.into(), ..maplelens_core::market::sample::sample_items()[0].clone() };
    let catalog = Catalog::new([item("Steel Pow"), item("Maple Claw")], None);
    let name = |read: &str| catalog.lookup(read).map(|i| i.name.clone());
    // As the game draws it and as the stream's compressed pixels read: the mark is never part of the name.
    for read in ["Steel Pow (+3)", "Steel Pow 1+3)", "Steel Pow (+12", "Steel Pow"] {
        assert_eq!(name(read).as_deref(), Some("Steel Pow"), "{read}");
    }
}

#[test]
fn a_misread_that_fits_two_items_equally_names_neither() {
    let item = |name: &str| Item { name: name.into(), ..maplelens_core::market::sample::sample_items()[0].clone() };
    let catalog = Catalog::new(
        [item("Hat HP Scroll: Greater"), item("Hat MP Scroll: Greater"), item("Dark Calaf"), item("Dark Calas")],
        None,
    );
    let name = |read: &str| catalog.lookup(read).map(|i| i.name.clone());
    // One letter lost where two names differ: any pick would be a guess.
    assert_eq!(name("Hat P Scroll: Greater"), None);
    assert_eq!(name("Dark Cala"), None);
    // One letter off a single name is still that name.
    assert_eq!(name("Hat HP Scrol: Greater").as_deref(), Some("Hat HP Scroll: Greater"));
    assert_eq!(name("Dark Calaf").as_deref(), Some("Dark Calaf"));
}

#[test]
fn numbers_in_names_are_never_fuzzy() {
    let item = |name: &str| Item { name: name.into(), ..maplelens_core::market::sample::sample_items()[0].clone() };
    let catalog =
        Catalog::new([item("Leaf Points (1,000) Exchange Coupon"), item("Leaf Points (10,000) Exchange Coupon")], None);
    let name = |read: &str| catalog.lookup(read).map(|i| i.name.clone());
    assert_eq!(name("Leaf Points (10,000) Exchange Coupon").as_deref(), Some("Leaf Points (10,000) Exchange Coupon"));
    // A lost or extra zero is another coupon's name, not a misread of this one.
    assert_eq!(name("Leaf Points (10,00) Exchange Coupon"), None);
    assert_eq!(name("Leaf Points (1,0000) Exchange Coupon"), None);
}

#[test]
fn a_lost_gender_letter_names_neither_variant() {
    let item = |name: &str| Item { name: name.into(), ..maplelens_core::market::sample::sample_items()[0].clone() };
    let catalog = Catalog::new([item("Red Cloth Pants"), item("Red Cloth Pants (F)"), item("Undershirt")], None);
    let name = |read: &str| catalog.lookup(read).map(|i| i.name.clone());
    assert_eq!(name("Red Cloth Pants ()"), None);
    assert_eq!(name("Red Cloth Pants (F)").as_deref(), Some("Red Cloth Pants (F)"));
    assert_eq!(name("Red Cloth Pants").as_deref(), Some("Red Cloth Pants"));
    // An item with no variants still reads through a gender mark.
    assert_eq!(name("Undershirt (M)").as_deref(), Some("Undershirt"));
}

#[test]
fn several_look_alike_misreads_in_one_name_still_find_it() {
    // "Iron Ore" as Windows OCR and Tesseract read it: two letters swapped for look-alikes.
    let catalog = Catalog::new(
        [make("Iron Ore", 10), make("Iron Axe", 10), make("Mithril Ore", 10), make("Emerald Ore", 10)],
        None,
    );
    assert_eq!(found(&catalog, "Iran Qre").as_deref(), Some("Iron Ore"));
    assert_eq!(found(&catalog, "lron 0re").as_deref(), Some("Iron Ore"));
    // Wrong letters that are not look-alikes cost full price: this is nothing, not Iron Axe.
    assert_eq!(found(&catalog, "Iron Qxz"), None);
}
