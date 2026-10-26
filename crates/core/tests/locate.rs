//! Finding a tooltip the game slid up to fit on screen.
//!
//! Both fixtures are strips whose left edge is the tooltip's left edge; the tooltip starts below some UI
//! that belongs to something else, and its bottom is the bottom of the screen.

mod common;

use std::sync::Arc;

use common::fixture;
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::RgbImage;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::models::Item;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::vision::locate::candidate_tops;
use maplelens_core::vision::reader::{NoOcr, TextReader, title_atlas};

const CASES: [(&str, usize); 2] = [("strip_pushed_up_cash_ring.png", 26), ("strip_pushed_up_cash_ring_b.png", 7)];
const RING: &str = "Ink-and-Wash Painting Name Tag Ring";

fn ring_detector() -> HoverDetector {
    let ring = Item { category: Some("Cash Shop".into()), price: Some(1_899_999.0), sold: 57, ..Item::named(RING) };
    HoverDetector::new(Arc::new(Catalog::new([ring], None)), TextReader::new(title_atlas(), Box::new(NoOcr)))
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn finds_the_pushed_up_tooltip() {
    for (name, top) in CASES {
        assert_eq!(candidate_tops(&fixture(name)), vec![top], "{name}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn reads_the_long_title_at_the_found_top() {
    let strip = fixture(CASES[0].0);
    let tops = candidate_tops(&strip);
    let mut reader = TextReader::new(title_atlas(), Box::new(NoOcr));
    assert_eq!(reader.read_title(&strip.crop(0, tops[0], strip.width, tops[0] + 120), 370, 2), RING);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn plain_backdrop_has_no_tooltip() {
    let sky = fixture("tooltip_native_over_sky.png");
    assert!(candidate_tops(&sky.crop(0, 300, sky.width, sky.height)).is_empty());
}

/// The whole path: mouse low on screen, the long-named ring's tooltip slid up -> Hover with its real height.
#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn pipeline_finds_the_pushed_up_item() {
    let screen_image = fixture(CASES[0].0); // tooltip left edge = screen x 0
    let bounds = Rect::new(0, 0, screen_image.width as i32, screen_image.height as i32);
    // Tooltips start at mouse + (1, 32).
    let mut screen =
        FrameScreen { frame: &screen_image, origin: Point::default(), cursor: Some(Point { x: -1, y: 400 }) };
    let mut detector = ring_detector();
    let hover = detector.step_at_scale(&mut screen, bounds, 1.0, 0.0).expect("the pushed-up ring is found");
    assert_eq!(hover.item.as_ref().map(|i| i.name.as_str()), Some(RING));
    // From the found top to the bottom of the screen.
    assert_eq!(hover.pushed_up_height, Some(screen_image.height as i32 - 26));
}

/// Mouse 300 px above the bottom: a 178 px tooltip fits below it, a 490 px ring does not and is pushed up.
#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn pipeline_looks_for_a_pushed_tooltip_even_when_a_short_one_would_fit() {
    let strip = fixture(CASES[0].0);
    let mut tall = RgbImage::filled(strip.width, strip.height + 600, [20, 30, 40]);
    for y in 0..strip.height {
        for x in 0..strip.width {
            tall.set_pixel(x, y + 600, strip.pixel(x, y));
        }
    }
    let height = tall.height as i32;
    let bounds = Rect::new(0, 0, tall.width as i32, height);
    let mut screen =
        FrameScreen { frame: &tall, origin: Point::default(), cursor: Some(Point { x: -1, y: height - 300 - 32 }) };
    let hover = ring_detector().step_at_scale(&mut screen, bounds, 1.0, 0.0).expect("found");
    assert!(hover.pushed_up_height.is_some());
}
