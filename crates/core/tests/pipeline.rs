//! The hover pipeline end to end on fixture "screens", with and without a mouse position, and the glyph
//! atlas learning letters from reads the catalog confirms.

mod common;

use std::sync::Arc;

use common::fixture;
use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::RgbImage;
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::models::Item;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::{HoverDetector, looks_like_title, tooltip_area};
use maplelens_core::vision::Mask;
use maplelens_core::vision::find::find_tooltips;
use maplelens_core::vision::glyphs::Atlas;
use maplelens_core::vision::reader::{NoOcr, Ocr, OcrAnswer, TextReader, title_atlas, title_mask};
use maplelens_core::vision::samples::{Font, SAMPLES};

fn item(name: &str, id: &str) -> Item {
    Item { item_id: id.into(), category: Some("Etc".into()), price: Some(100.0), sold: 10, ..Item::named(name) }
}

fn detector(names: &[(&str, &str)]) -> HoverDetector {
    let catalog = Catalog::new(names.iter().map(|(n, id)| item(n, id)), None);
    HoverDetector::new(Arc::new(catalog), TextReader::new(title_atlas(), Box::new(NoOcr)))
}

/// `tooltip` pasted at `at` into a 1920x1200 game frame (reference resolution: UI scale 1) over a busy
/// backdrop (the Character Info screenshot, tiled).
fn game_frame(tooltip: &RgbImage, at: (usize, usize)) -> RgbImage {
    let backdrop = fixture("character_info_masterball.png");
    let mut frame = RgbImage::new(1920, 1200);
    for y in 0..frame.height {
        for x in 0..frame.width {
            frame.set_pixel(x, y, backdrop.pixel(x % backdrop.width, y % backdrop.height));
        }
    }
    for y in 0..tooltip.height.min(frame.height - at.1) {
        for x in 0..tooltip.width.min(frame.width - at.0) {
            frame.set_pixel(at.0 + x, at.1 + y, tooltip.pixel(x, y));
        }
    }
    frame
}

const BOUNDS: Rect = Rect::new(0, 0, 1920, 1200);

#[test]
fn item_names_are_titles_and_hints_are_not() {
    for text in ["Emerald Ore", "Kitty Camping Hat", "Hunter's Bow"] {
        assert!(looks_like_title(text), "{text}");
    }
    for text in [
        "Double-click to send whispers.",
        "C) oubl*-click: tü send",
        "r •.lfll IVIlP-f: lif:k tri rid b'dhi P.",
        "to",
        "to send",
        "",
        "Em?rald Ore",
    ] {
        assert!(!looks_like_title(text), "{text}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_mouse_says_where_the_tooltip_is() {
    let frame = game_frame(&fixture("tooltip_native_over_sky.png"), (600, 400));
    // Tooltips start at mouse + (1, 32).
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: Some(Point { x: 599, y: 368 }) };
    let mut detector = detector(&[("Emerald Ore", "etc:04020003"), ("Silver Ore", "etc:04010004")]);
    let hover = detector.step(&mut screen, BOUNDS, 0.0).expect("tooltip under the mouse");
    assert_eq!(hover.title, "Emerald Ore");
    assert_eq!(hover.tooltip_size, (450, 178));
    assert!(!hover.compare && hover.found_at.is_none());
    // Mouse elsewhere: nothing.
    let mut away = FrameScreen { frame: &frame, origin: Point::default(), cursor: Some(Point { x: 100, y: 100 }) };
    assert_eq!(detector.step(&mut away, BOUNDS, 0.1), None);
}

#[test]
fn no_room_below_the_mouse_means_no_tooltip_area() {
    assert_eq!(tooltip_area(Point { x: 100, y: 1190 }, BOUNDS, 1.0), None);
    assert_eq!(tooltip_area(Point { x: 1800, y: 100 }, BOUNDS, 1.0), None); // not even the title fits
    assert_eq!(tooltip_area(Point { x: 99, y: 68 }, BOUNDS, 1.0), Some(Rect::new(100, 100, 743, 120)));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn tooltips_are_found_anywhere_without_a_mouse() {
    for (name, at) in [
        ("tooltip_native_over_sky.png", (600, 400)),
        ("tooltip_native_octopus_leg.png", (37, 701)),
        ("tooltip_equip_kitty_camping_hat.png", (1100, 90)),
    ] {
        let frame = game_frame(&fixture(name), at);
        let spots = find_tooltips(&frame, 1.0);
        assert!(
            spots.iter().any(|s| (s.x - at.0 as i32).abs() <= 2 && (s.y - at.1 as i32).abs() <= 2),
            "{name}: expected near {at:?}, found {spots:?}"
        );
    }
    assert!(find_tooltips(&game_frame(&RgbImage::new(1, 1), (0, 0)), 1.0).is_empty());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_browser_path_reads_the_same_item() {
    let frame = game_frame(&fixture("tooltip_native_octopus_leg.png"), (812, 333));
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
    let mut detector = detector(&[("Octopus Leg", "etc:04000006")]);
    let hover = detector.step(&mut screen, BOUNDS, 0.0).expect("found without a mouse");
    assert_eq!(hover.title, "Octopus Leg");
    let at = hover.found_at.expect("search reports where");
    assert!((at.x - 812).abs() <= 2 && (at.y - 333).abs() <= 2, "{at:?}");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn compare_tooltips_are_found_both_and_the_hovered_one_first() {
    // Two equipment tooltips side by side, the worn one TOOLTIP_COMPARE_OFFSET_X to the right.
    let kitty = fixture("tooltip_equip_kitty_camping_hat.png");
    let hat = kitty.crop(0, 0, 372, kitty.height);
    let mut frame = game_frame(&hat, (500, 200));
    let right = 500 + config::TOOLTIP_COMPARE_OFFSET_X as usize;
    for y in 0..hat.height {
        for x in 0..hat.width {
            frame.set_pixel(right + x, 200 + y, hat.pixel(x, y));
        }
    }
    let spots = find_tooltips(&frame, 1.0);
    assert_eq!(spots.len(), 2, "{spots:?}");
    assert!((spots[0].x - 500).abs() <= 2 && (spots[1].x - right as i32).abs() <= 2, "{spots:?}");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn compare_tooltips_over_a_bright_window_are_found_at_their_tops() {
    // A 1366x768 client at Ideal Ratio, an inventory hat hovered over an open store: the fill under both titles lies
    // over the store's light paper, so the band was found only under the job row's box, whose line was taken for the
    // top. The hovered tooltip's "title" was then a stats line, and the browser showed the worn hat's card instead.
    let frame = fixture("compare_ideal_ratio_1366x768.png");
    let scale = 768.0 / 1200.0;
    let spots = find_tooltips(&frame, scale);
    let near = |s: &Point, x: i32| (s.x - x).abs() <= 3 && (s.y - 298).abs() <= 3;
    assert!(spots.len() == 2 && near(&spots[0], 383) && near(&spots[1], 621), "{spots:?}");
    let bounds = Rect::new(0, 0, frame.width as i32, frame.height as i32);
    let mut detector = detector(&[("Brown Bamboo Hat", "eqp:01002073"), ("Black Loosecap", "eqp:01002176")]);
    for now in [0.0, 0.1, 0.2] {
        let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
        let hover = detector.step(&mut screen, bounds, now).expect("found");
        assert_eq!(hover.title, "Brown Bamboo Hat");
    }
}

/// An OCR stand-in that knows one answer.
struct Oracle(&'static str);

impl Ocr for Oracle {
    fn recognize(&mut self, _mask: &Mask) -> OcrAnswer {
        OcrAnswer::Text(self.0.to_string())
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn unknown_letters_are_learnt_from_confirmed_reads() {
    let image = fixture("tooltip_native_octopus_leg.png");
    let mask = title_mask(&image, 450, 1).unwrap();
    let mut reader = TextReader::new(Atlas::empty(18, 6, 7), Box::new(NoOcr));
    let first = reader.read_mask(&mask);
    assert_eq!(first, "??????? ???"); // nothing known yet: every letter is a wildcard
    assert!(reader.confirm(&first, "Octopus Leg") >= 9); // one variant per letter (the two o's may differ)
    assert!(reader.take_learned());
    assert_eq!(reader.read_mask(&mask), "Octopus Leg");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn ocr_answers_fill_in_unknown_letters() {
    let image = fixture("tooltip_native_silver_ore.png");
    let mut reader = TextReader::new(Atlas::empty(18, 6, 7), Box::new(Oracle("'Silver Ore")));
    assert_eq!(reader.read_title(&image, 450, 1), "Silver Ore"); // stray leading quote stripped
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn held_out_reads_rarely_guess_a_wrong_letter() {
    // Every sample read with an atlas built from all the others: letters are right or '?', and a wrong letter is
    // rare. Some letters occur in only one or two other samples, over a busy backdrop; there a noisy glyph can look
    // more like another letter ('i' as 'r'). The catalog absorbs single slips: on 792 real captures no wrong item
    // was shown (crates/win/src/reading_bench.rs).
    let masks: Vec<(usize, Mask)> = SAMPLES
        .iter()
        .enumerate()
        .filter(|(_, s)| s.font == Font::Title)
        .map(|(i, s)| {
            let image = fixture(s.fixture);
            let tooltip = image.crop(s.origin.0, s.origin.1, image.width, image.height);
            (i, title_mask(&tooltip, s.width, s.lines).unwrap())
        })
        .collect();
    let (mut letters, mut wrong) = (0, Vec::new());
    for (i, mask) in &masks {
        let mut others = Atlas::empty(18, 6, 7);
        for (j, m) in &masks {
            if j != i {
                others.learn(m, SAMPLES[*j].text);
            }
        }
        let read = others.read(mask).text;
        let truth = SAMPLES[*i].text;
        // Compare letter by letter where the read has the same shape (a '?' may stand for touching letters).
        if read.chars().count() == truth.chars().count() {
            let slips = read.chars().zip(truth.chars()).filter(|&(r, t)| r != '?' && r != t).count();
            assert!(slips <= 1, "{truth}: read {read}");
            if slips > 0 {
                wrong.push(format!("{truth} -> {read}"));
            }
            letters += truth.chars().filter(|c| !c.is_whitespace()).count();
        } else {
            let known: String = read.chars().filter(|&c| c != '?').collect();
            assert!(known.split(' ').all(|w| truth.contains(w.trim())), "{truth}: read {read}");
        }
    }
    assert!(wrong.len() * 100 <= letters, "more than 1% wrong letters ({letters} letters): {wrong:?}");
}

#[test]
fn atlas_round_trips_through_json() {
    let atlas = title_atlas();
    assert!(atlas.glyphs.len() > 40);
    assert_eq!(Atlas::from_json(&atlas.to_json()).unwrap().glyphs.len(), atlas.glyphs.len());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_browser_path_works_at_other_resolutions() {
    // A 1920x1080 game draws its UI at 0.9x (and 1440p at 1.2x); the frame is searched as captured.
    for (w, h) in [(1920usize, 1080usize), (2560, 1440)] {
        let scale = h as f32 / 1200.0;
        let tooltip = fixture("tooltip_native_octopus_leg.png").resized(scale);
        let mut frame = game_frame(&tooltip, (500, 300));
        frame = frame.crop(0, 0, w.min(frame.width), h.min(frame.height));
        if frame.width < w {
            let mut wide = RgbImage::new(w, h);
            for y in 0..frame.height {
                for x in 0..frame.width {
                    wide.set_pixel(x, y, frame.pixel(x, y));
                }
            }
            for y in 0..tooltip.height {
                for x in 0..tooltip.width {
                    wide.set_pixel(500 + x, 300 + y, tooltip.pixel(x, y));
                }
            }
            frame = wide;
        }
        let spots = find_tooltips(&frame, scale);
        assert!(spots.iter().any(|s| (s.x - 500).abs() <= 3 && (s.y - 300).abs() <= 3), "{w}x{h}: {spots:?}");
        let bounds = Rect::new(0, 0, w as i32, h as i32);
        let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
        let hover = detector(&[("Octopus Leg", "etc:04000006")]).step(&mut screen, bounds, 0.0).expect("found");
        assert_eq!(hover.title, "Octopus Leg", "{w}x{h}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_followed_tooltip_is_checked_in_place_and_lost_or_moved_at_once() {
    let tooltip = fixture("tooltip_native_octopus_leg.png");
    let mut detector = detector(&[("Octopus Leg", "etc:04000006")]);
    let mut look = |frame: &RgbImage, now: f64| {
        let mut screen = FrameScreen { frame, origin: Point::default(), cursor: None };
        detector.step(&mut screen, BOUNDS, now)
    };
    let here = game_frame(&tooltip, (812, 333));
    // The first look searches the frame, the next ones check the spot: all name the same item at the same place.
    for i in 0..4 {
        let hover = look(&here, i as f64 * 0.1).expect("followed");
        assert_eq!(
            (hover.title.as_str(), hover.found_at.map(|p| (p.x / 4, p.y / 4))),
            ("Octopus Leg", Some((203, 83)))
        );
    }
    // Moved: found at its new place on the very next look, not after the next full search.
    let moved = game_frame(&tooltip, (300, 700));
    let hover = look(&moved, 0.5).expect("found again");
    let at = hover.found_at.expect("where");
    assert!((at.x - 300).abs() <= 2 && (at.y - 700).abs() <= 2, "{at:?}");
    // Gone: nothing at once.
    assert!(look(&game_frame(&RgbImage::new(1, 1), (0, 0)), 0.6).is_none());
    // And a tooltip that appears elsewhere while one is followed is found within the full-search period.
    look(&moved, 0.7).expect("back");
    let elsewhere = game_frame(&tooltip, (1100, 300));
    let found = (0..8).find_map(|i| look(&elsewhere, 0.8 + i as f64 * 0.1)).expect("found");
    let at = found.found_at.expect("where");
    assert!((at.x - 1100).abs() <= 2 && (at.y - 300).abs() <= 2, "{at:?}");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_tooltip_clamped_to_the_right_edge_of_the_frame_is_found() {
    // The Cash Shop's tooltip, translucent over the shop and pushed against the client's right edge: its right border
    // is on the last column or past it, so no width can be measured and the fill over wood is too dark to read one.
    let crop = fixture("cash_shop_tooltip_at_right_edge.png");
    let frame = game_frame(&crop, (1520, 800));
    let spots = find_tooltips(&frame, 1.0);
    assert!(spots.iter().any(|s| (s.x - 1551).abs() <= 2 && (s.y - 834).abs() <= 2), "{spots:?}");
}
