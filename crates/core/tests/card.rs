//! The price card: size, translucency and which sections show.

mod common;

use common::{fonts, now};
use maplelens_core::market::models::{Item, NpcShop};
use maplelens_core::market::sample::sample_items;
use maplelens_core::render::RgbaImage;
use maplelens_core::render::canvas::Canvas;
use maplelens_core::render::card::{CARD_WIDTH, CardSpec, render_card};
use maplelens_core::render::range_bar::{PAD, listing_colour, listing_marker};
use maplelens_core::render::theme;

fn emerald() -> Item {
    sample_items().remove(0)
}

fn card(item: Option<&Item>, expanded: bool, wide: bool, ui_scale: f32) -> RgbaImage {
    let spec = CardSpec {
        item,
        title: "Mystery Item",
        generated_at: Some("2026-10-21T23:45:49+00:00"),
        now: now(),
        expanded,
        ui_scale,
        wide,
        source: "meso.watch",
    };
    render_card(fonts(), &spec)
}

#[test]
fn card_is_translucent_like_the_game_tooltip() {
    let card = card(Some(&emerald()), true, false, 1.0);
    assert!(card.width == (CARD_WIDTH * theme::CARD_SCALE).round() as usize && (300..900).contains(&card.height));
    // A spot in the panel with no content: the fill, as see-through as the game's box (not opaque, not clear).
    assert_eq!(card.pixel(card.width - 30, 40)[3], theme::PANEL_FILL[3]);
    assert!(card.pixel(card.width / 2, card.height / 2)[3] > 100);
}

#[test]
fn unknown_item_gets_a_small_notice() {
    let (unknown, known) = (card(None, true, false, 1.0), card(Some(&emerald()), true, false, 1.0));
    assert!(unknown.height < known.height / 2);
}

#[test]
fn item_without_sales_still_renders() {
    let unsold = Item {
        category: Some("Etc".into()),
        npc: Some(NpcShop { buy: None, sell: Some(5.0), shop: None, sellers: 0 }),
        ..Item::named("Unsold")
    };
    assert!(card(Some(&unsold), true, false, 1.0).height > 60);
}

#[test]
fn glance_card_is_a_compact_version_of_the_full_card() {
    let item = emerald();
    let (glance, full) = (card(Some(&item), false, false, 1.0), card(Some(&item), true, false, 1.0));
    assert_eq!(glance.width, full.width);
    // The glance card is what you see first, so it must stay small.
    assert!((glance.height as f32) < full.height as f32 * 0.65);
}

#[test]
fn card_scales_with_game_resolution() {
    let (full, small) = (card(Some(&emerald()), true, false, 1.0), card(Some(&emerald()), true, false, 0.5));
    assert_eq!(small.width, (full.width as f32 / 2.0).round() as usize);
}

#[test]
fn compare_card_is_horizontal() {
    let item = emerald();
    let (normal, wide) = (card(Some(&item), true, false, 1.0), card(Some(&item), true, true, 1.0));
    assert!(wide.width as f32 > normal.width as f32 * 1.5 && (wide.height as f32) < normal.height as f32 * 0.75);
    // Nothing to expand on a glance card: it stays the compact one.
    assert_eq!(card(Some(&item), false, true, 1.0).width, normal.width);
}

#[test]
fn ask_marker_shows_on_glance_card_and_adds_a_row() {
    let item = emerald();
    let without = Item { ask_median: None, ..item.clone() };
    assert!(card(Some(&item), false, false, 1.0).height > card(Some(&without), false, false, 1.0).height);
}

#[test]
fn ask_marker_label_flips_left_at_the_right_edge() {
    let mut cv = Canvas::new(fonts(), CARD_WIDTH, 80.0, 1.0);
    listing_marker(&mut cv, CARD_WIDTH - PAD, 40.0, 900.0, theme::BULLET);
    let (left, _, right, _) = RgbaImage::from_canvas(&cv).alpha_bbox().expect("something drawn");
    // The label sits left of the arrow.
    assert!(right as f32 <= CARD_WIDTH - PAD + 4.0 && (left as f32) < CARD_WIDTH - 60.0);
}

#[test]
fn ask_marker_turns_amber_only_when_shops_ask_well_above_sales() {
    let item = emerald(); // sells at 100
    assert_eq!(listing_colour(&Item { ask_median: Some(110.0), ..item.clone() }), theme::BULLET);
    assert_eq!(listing_colour(&Item { ask_median: Some(130.0), ..item }), theme::WARN);
}

#[test]
fn a_token_npc_price_is_left_off_the_card() {
    let item = Item { obtain: Vec::new(), ..emerald() };
    // An NPC paying 1 meso for a 100 meso item says nothing.
    let token = Item { npc: Some(NpcShop { buy: None, sell: Some(1.0), shop: None, sellers: 0 }), ..item.clone() };
    let none = Item { npc: None, ..item.clone() };
    assert_eq!(card(Some(&token), true, false, 1.0), card(Some(&none), true, false, 1.0));
    let notable = Item { npc: Some(NpcShop { buy: None, sell: Some(40.0), shop: None, sellers: 0 }), ..item };
    assert!(card(Some(&notable), true, false, 1.0).height > card(Some(&token), true, false, 1.0).height);
}

/// Same layout as the Python card: identical sizes for the demo items (tests/golden/python.json).
#[test]
fn card_sizes_match_the_python_implementation() {
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(common::repo().join("tests/golden/python.json")).unwrap())
            .unwrap();
    for item in sample_items() {
        for expanded in [false, true] {
            for wide in [false, true] {
                let expected = &golden["cards"][format!(
                    "{}|{}|{}",
                    item.name,
                    if expanded { "True" } else { "False" },
                    if wide { "True" } else { "False" }
                )];
                let c = card(Some(&item), expanded, wide, 1.0);
                assert_eq!(
                    [c.width as u64, c.height as u64],
                    [expected[0].as_u64().unwrap(), expected[1].as_u64().unwrap()],
                    "{} {expanded} {wide}",
                    item.name
                );
            }
        }
    }
}

#[test]
fn premultiplied_output_never_exceeds_alpha() {
    let bgra = card(Some(&emerald()), true, true, 1.0).premultiplied_bgra();
    assert!(bgra.as_chunks::<4>().0.iter().all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
}
