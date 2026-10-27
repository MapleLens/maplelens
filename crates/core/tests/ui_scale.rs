//! The UI scale, measured off the status bar so "UI Size: Ideal Ratio" works (the UI keeps its size whatever
//! the window), and player-crafted items, whose title sits under a crafter-name line.

mod common;

use std::sync::Arc;

use common::fixture;
use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect, UiScale};
use maplelens_core::image::RgbImage;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::models::Item;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::vision::hud::{Bar, find_bar};
use maplelens_core::vision::reader::{NoOcr, TextReader, title_atlas};

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn bar_size_in_each_ui_mode() {
    for (name, width, height) in [
        ("status_bar_normal_1920x1200.png", 1250, 106),
        ("status_bar_ideal_ratio_1920x1200.png", 800, 68),
        // Kerning City Pharmacy: the map is smaller than the window and the black around it joins the outline.
        ("status_bar_beside_black_ideal_ratio_1920x1200.png", 800, 68),
    ] {
        let strip = fixture(name);
        let bar = find_bar(&strip).unwrap_or_else(|| panic!("{name}: no bar"));
        assert_eq!((bar.width, strip.height - bar.top), (width, height), "{name}");
        assert_eq!(bar.x, (strip.width - width) / 2, "{name}");
    }
}

/// The bottom 130 rows of a 1920x1200 client in Ideal Ratio, drawn: the bar's outline (columns 560 and 1359, top row
/// 62) around a light inside, with the black the game draws around a small map, or scenery, to its sides and above.
fn drawn_bar(black_left: bool, black_right: bool, black_above: bool) -> RgbImage {
    let (left, right, top) = (560, 1359, 62);
    let mut strip = RgbImage::filled(1920, 130, [150, 160, 120]);
    for y in 0..strip.height {
        for x in 0..strip.width {
            let black = match x {
                x if x < left => black_left,
                x if x > right => black_right,
                _ => y < top && black_above,
            };
            let outline = (left..=right).contains(&x) && y >= top && (y == top || x == left || x == right);
            if black || outline {
                strip.set_pixel(x, y, [0, 0, 0]);
            } else if (left..=right).contains(&x) && y > top {
                strip.set_pixel(x, y, [200, 200, 210]);
            }
        }
    }
    strip
}

#[test]
fn black_around_a_small_map_is_not_part_of_the_bar() {
    // The black runs into the outline, so its top row is dark from edge to edge: the bar ends where the black does.
    for (black_left, black_right, black_above) in
        [(true, true, false), (true, true, true), (true, false, false), (false, true, true), (false, false, false)]
    {
        let bar = find_bar(&drawn_bar(black_left, black_right, black_above));
        let case = (black_left, black_right, black_above);
        assert_eq!(bar, Some(Bar { x: 560, top: 62, width: 800 }), "black left, right, above: {case:?}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn real_bars_measure_the_same_with_black_around_them() {
    // The game's own bar insides at both UI sizes, with everything outside the outline painted black (beside it, and
    // above it too, as in a map shorter than the window with the chat box closed).
    for name in ["status_bar_normal_1920x1200.png", "status_bar_ideal_ratio_1920x1200.png"] {
        let strip = fixture(name);
        let bar = find_bar(&strip).unwrap_or_else(|| panic!("{name}: no bar"));
        for black_above in [false, true] {
            let mut painted = strip.clone();
            for y in 0..strip.height {
                for x in 0..strip.width {
                    let beside = x < bar.x || x >= bar.x + bar.width;
                    if (y >= bar.top && beside) || (y < bar.top && (black_above || beside)) {
                        painted.set_pixel(x, y, [0, 0, 0]);
                    }
                }
            }
            assert_eq!(find_bar(&painted), Some(bar), "{name}, black above: {black_above}");
        }
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_tooltip_in_a_small_map_is_found_at_the_bars_scale() {
    // Kerning City Pharmacy in Ideal Ratio, as the browser gets it (no mouse): the black around the map had made the
    // whole width read as the bar (scale 1.53), and no tooltip was found at that size.
    let frame = fixture("frame_ideal_ratio_pharmacy_tree_branch.png");
    let bounds = Rect::new(0, 0, frame.width as i32, frame.height as i32);
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
    let branch = Item { category: Some("Etc".into()), price: Some(5.0), ..Item::named("Tree Branch") };
    let mut detector =
        HoverDetector::new(Arc::new(Catalog::new([branch], None)), TextReader::new(title_atlas(), Box::new(NoOcr)));
    let last = config::SCALE_VOTE_MIN_READINGS - 1;
    for t in 0..last {
        detector.step(&mut screen, bounds, t as f64);
    }
    let hover = detector.step(&mut screen, bounds, last as f64).expect("tooltip found at the measured scale");
    assert_eq!(hover.title, "Tree Branch");
    assert!((detector.ui_scale().of(bounds) - 0.64).abs() < 0.005, "{}", detector.ui_scale().of(bounds));
    let at = hover.found_at.expect("search reports where");
    assert!((at.y - 475).abs() <= 2 && (533..=546).contains(&at.x), "{at:?}");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn ideal_ratio_scale_matches_the_bar_height_too() {
    let bar = find_bar(&fixture("status_bar_ideal_ratio_1920x1200.png")).unwrap();
    let scale = bar.width as f32 / config::STATUS_BAR_WIDTH as f32;
    assert!((scale - 68.0 / 106.0).abs() <= 0.005, "{scale}");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn menu_over_the_outline_is_no_measurement() {
    // A game menu cut into the top outline: better no answer than a wrong width (the last one is kept).
    assert_eq!(find_bar(&fixture("status_bar_under_game_menu_1920x1200.png")), None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn no_bar_on_scenery() {
    let strip = fixture("status_bar_normal_1920x1200.png");
    assert_eq!(find_bar(&strip.crop(0, 0, strip.width, 20)), None); // above the bar
}

#[test]
fn scale_follows_window_height_until_measured_and_resets_on_resize() {
    let client = Rect::new(0, 0, 2560, 1440);
    let ui = UiScale::default();
    assert_eq!(ui.of(client), 1.2);
    ui.set(client, 0.62, 0.0); // "Ideal Ratio": the UI keeps its size whatever the window
    ui.set(client, 0.62, 1.0);
    assert_eq!(ui.of(client), 1.2, "two readings do not decide yet");
    ui.set(client, 0.62, 2.0);
    assert_eq!(ui.of(client), 0.62);
    ui.set(client, 1.2, 3.0); // one frame with a menu over the outline
    assert_eq!(ui.of(client), 0.62);
    assert_eq!(ui.of(Rect::new(0, 0, 1920, 1200)), 1.0); // another window size: measure again
}

#[test]
fn a_title_bar_guess_is_the_same_client() {
    // The browser cycles its guess at the window's title bar (0-47 px), which changes the height it passes.
    let ui = UiScale::default();
    for (t, inset) in [0, 31, 39, 47, 0].into_iter().enumerate() {
        ui.set(Rect::new(0, inset, 1920, 1080 - inset), 0.577, t as f64);
    }
    assert_eq!(ui.of(Rect::new(0, 39, 1920, 1041)), 0.577);
    assert_eq!(ui.of(Rect::new(0, 0, 1920, 1080)), 0.577);
}

#[test]
fn a_ratio_switch_on_the_same_client_takes_over_within_seconds() {
    let client = Rect::new(0, 0, 1920, 1080);
    let ui = UiScale::default();
    for t in 0..10 {
        ui.set(client, 0.9, t as f64);
    }
    let switched = (10..20).find(|&t| {
        ui.set(client, 0.577, t as f64);
        ui.of(client) == 0.577
    });
    let after = switched.expect("Ideal Ratio took over") - 10;
    assert!((2..=5).contains(&after), "{after} s");
}

fn paste(frame: &mut RgbImage, image: &RgbImage, at: (usize, usize)) {
    for y in 0..image.height.min(frame.height - at.1) {
        for x in 0..image.width.min(frame.width - at.0) {
            frame.set_pixel(at.0 + x, at.1 + y, image.pixel(x, y));
        }
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn ideal_ratio_game_reads_tooltips_at_the_bars_scale() {
    // 1920x1200 in Ideal Ratio: everything is drawn at 0.64, which the window height (1.0) cannot tell.
    let bar = fixture("status_bar_ideal_ratio_1920x1200.png");
    let mut frame = RgbImage::filled(1920, 1200, [30, 40, 60]);
    paste(&mut frame, &bar, (0, 1200 - bar.height));
    let scale = 800.0 / 1250.0;
    paste(&mut frame, &fixture("tooltip_native_octopus_leg.png").crop(0, 0, 452, 180).resized(scale), (700, 400));
    let bounds = Rect::new(0, 0, 1920, 1200);
    // Tooltips start at mouse + (1, 32), scaled.
    let cursor = Point { x: 700 - 1, y: 400 - (32.0 * scale).round() as i32 };
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: Some(cursor) };
    let leg = Item { category: Some("Etc".into()), price: Some(5.0), ..Item::named("Octopus Leg") };
    let mut detector =
        HoverDetector::new(Arc::new(Catalog::new([leg], None)), TextReader::new(title_atlas(), Box::new(NoOcr)));
    // The bar is read once a second and settles after a few readings.
    let last = config::SCALE_VOTE_MIN_READINGS - 1;
    for t in 0..last {
        detector.step(&mut screen, bounds, t as f64);
    }
    let hover = detector.step(&mut screen, bounds, last as f64).expect("tooltip read at the measured scale");
    assert_eq!(hover.title, "Octopus Leg");
    assert!((hover.scale - scale).abs() < 0.005, "{}", hover.scale);
    assert!((detector.ui_scale().of(bounds) - scale).abs() < 0.005);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn crafted_item_title_sits_below_the_crafter_line() {
    // "bTang's" prints on its own line above "Great Red Helmet", so the usual title row misses the name.
    let image = fixture("tooltip_equip_crafted_great_red_helmet.png");
    let helmet = Item {
        item_id: "basil:01002022".into(),
        category: Some("Equip".into()),
        price: Some(9000.0),
        ..Item::named("Great Red Helmet")
    };
    let mut frame = RgbImage::filled(1920, 1200, [30, 40, 60]);
    paste(&mut frame, &image, (300, 300));
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: Some(Point { x: 299, y: 268 }) };
    let catalog = Arc::new(Catalog::new([helmet], None));
    let mut detector = HoverDetector::new(catalog, TextReader::new(title_atlas(), Box::new(NoOcr)));
    let hover = detector.step_at_scale(&mut screen, Rect::new(0, 0, 1920, 1200), 1.0, 0.0).expect("crafted item");
    assert_eq!(hover.title, "Great Red Helmet");
    assert!(hover.crafted);
    // The crafter line makes the tooltip taller: the card is placed against the real height.
    assert_eq!(hover.tooltip_size.1, config::tooltip_size_for(Some("Equip")).1 + config::TOOLTIP_CRAFTED_SHIFT);
}

#[test]
fn a_black_border_is_not_a_bar() {
    assert_eq!(find_bar(&RgbImage::new(2560, 240)), None);
}
