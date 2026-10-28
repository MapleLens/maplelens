//! The browser's search (no mouse) on whole frames of a live session, one per kind of tooltip it used to miss or
//! misread: over a busy backdrop, inside another tooltip, a long title in a wide tooltip, and just after the player
//! switched UI Size.

mod common;

use std::sync::Arc;

use common::fixture;
use maplelens_core::geometry::Rect;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::models::Item;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::{Hover, HoverDetector};
use maplelens_core::vision::reader::{NoOcr, TextReader, title_atlas};

/// The items of these frames, and their look-alikes the misreads named.
const ITEMS: &[(&str, &str)] = &[
    ("Blue Snail Shell", "etc:04000000"),
    ("Snail Shell", "etc:04000019"),
    ("One-Handed Sword Attack Scroll: Greater", "use:02043000"),
    ("Shoes Speed Scroll: Greater", "use:02071000"),
];

fn search(name: &str, scale: f32) -> Option<Hover> {
    let frame = fixture(name);
    let catalog = Catalog::new(
        ITEMS.iter().map(|(n, id)| Item { item_id: (*id).into(), price: Some(100.0), sold: 10, ..Item::named(n) }),
        None,
    );
    let mut detector = HoverDetector::new(Arc::new(catalog), TextReader::new(title_atlas(), Box::new(NoOcr)));
    let bounds = Rect::new(0, 0, frame.width as i32, frame.height as i32);
    let mut screen = FrameScreen { frame: &frame, origin: Default::default(), cursor: None };
    detector.step_at_scale(&mut screen, bounds, scale, 0.0)
}

fn item_of(hover: Option<Hover>) -> Option<String> {
    hover.and_then(|h| h.item.map(|i| i.name.clone()))
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_tooltip_over_inventory_icons_is_found_by_its_bullet() {
    // No fill band or border survives over the icons; only the title's bullet does.
    assert_eq!(item_of(search("live_busy_backdrop_1200.png", 1.0)).as_deref(), Some("Blue Snail Shell"));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_spot_inside_a_tooltip_does_not_read_the_end_of_its_title() {
    // A border-like edge inside the tooltip, 46 px right of its corner, read "Snail Shell" off "Blue Snail Shell".
    assert_eq!(item_of(search("live_spot_inside_768.png", 0.64)).as_deref(), Some("Blue Snail Shell"));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_long_title_in_a_wide_tooltip_is_read_to_its_end() {
    let item = item_of(search("live_wide_title_1080.png", 0.9));
    assert_eq!(item.as_deref(), Some("One-Handed Sword Attack Scroll: Greater"));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn just_after_a_ui_size_switch_the_other_scale_is_tried() {
    // Drawn at UI scale 1 while the vote still says Ideal Ratio's 0.64.
    let hover = search("live_ui_size_switched_1200.png", 0.64).expect("found at the other scale");
    assert_eq!(hover.item.map(|i| i.name.clone()).as_deref(), Some("Shoes Speed Scroll: Greater"));
    assert!((hover.scale - 1.0).abs() < 0.01, "{}", hover.scale);
}
