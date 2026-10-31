//! Reading tooltip titles on real captures that once went wrong: scenery beside a shifted tooltip, a border broken
//! into dashes, and screens under the pointer that are not tooltips at all. Measured on 792 captures of real play
//! (crates/win/src/reading_bench.rs), the app path shows no card for the wrong item.

mod common;

use std::sync::Arc;

use common::fixture;
use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::market::models::Item;
use maplelens_core::market::sample::sample_items;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::vision::Mask;
use maplelens_core::vision::reader::{NoOcr, Ocr, OcrAnswer, TextReader, find_bullet, title_atlas};

const WIDTH: usize = config::TOOLTIP_TITLE_WIDTH as usize;

fn catalog(names: &[&str]) -> Arc<Catalog> {
    let item = |name: &&str| Item { name: name.to_string(), ..sample_items()[0].clone() };
    Arc::new(Catalog::new(names.iter().map(item), None))
}

/// An OCR engine that reads one fixed text whatever it is shown: Windows OCR reads shop lists and quest logs too.
struct Reads(&'static str);

impl Ocr for Reads {
    fn recognize(&mut self, _mask: &Mask) -> OcrAnswer {
        OcrAnswer::Text(self.0.to_string())
    }
}

/// What the hover detector makes of `name` captured at the usual tooltip spot.
fn hover(name: &str, catalog: Arc<Catalog>, ocr: &'static str) -> Option<String> {
    let image = fixture(name);
    let mut detector = HoverDetector::new(catalog, TextReader::new(title_atlas(), Box::new(Reads(ocr))));
    let mut screen = FrameScreen {
        frame: &image,
        origin: Point { x: 0, y: 0 },
        cursor: Some(Point { x: -config::TOOLTIP_OFFSET.0, y: -config::TOOLTIP_OFFSET.1 }),
    };
    detector
        .step_at_scale(&mut screen, Rect::new(0, 0, 1920, 1200), 1.0, 0.0)
        .and_then(|h| h.item)
        .map(|i| i.name.clone())
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_title_starts_after_its_bullet_even_when_the_tooltip_is_off_its_usual_spot() {
    // The tooltip sits 10 px right of and 6 px below where tooltips usually start; bright scenery fills the gap
    // and read as four junk letters, which the catalog then took for "Supreme Sniper Potion".
    let image = fixture("tooltip_shifted_sniper_potion.png");
    assert!(find_bullet(&image, 0).is_some());
    let mut reader = TextReader::new(title_atlas(), Box::new(NoOcr));
    assert_eq!(reader.read_title(&image, WIDTH, 1), "Sniper Potion");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_border_broken_into_dashes_does_not_move_the_baseline() {
    // The dashed top border was the busiest row; the letters' lower half fell below the bar and every glyph was
    // read 4 px too high ("????????").
    let image = fixture("tooltip_dashed_border_firewood.png");
    let mut reader = TextReader::new(title_atlas(), Box::new(NoOcr));
    assert_eq!(reader.read_title(&image, WIDTH, 1), "Firewood");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn screens_that_are_not_tooltips_show_no_card_whatever_ocr_reads() {
    let items = catalog(&["Firewood", "Squishy Liquid", "Stirge Wing", "Salad", "Tourist T", "Mithril Ore"]);
    for (name, ocr) in [
        ("no_tooltip_shop_list.png", "Firewood"),
        ("no_tooltip_quest_log.png", "Stirge Wing"),
        ("no_tooltip_shop_list_blue_icon.png", "Tourist T"),
    ] {
        assert_eq!(hover(name, items.clone(), ocr), None, "{name}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_light_blue_item_icon_is_not_a_title_bullet() {
    // The Tourist T icon's light-blue patch sits on the shop list's light background, not a tooltip's dark fill.
    assert_eq!(find_bullet(&fixture("no_tooltip_shop_list_blue_icon.png"), 0), None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_real_tooltip_still_gets_its_card() {
    let items = catalog(&["Sniper Potion", "Supreme Sniper Potion", "Firewood"]);
    assert_eq!(hover("tooltip_shifted_sniper_potion.png", items.clone(), "").as_deref(), Some("Sniper Potion"));
    assert_eq!(hover("tooltip_dashed_border_firewood.png", items, "").as_deref(), Some("Firewood"));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn ocr_losing_letters_falls_back_to_the_atlas_read() {
    // The atlas read 'SiI?er Ore' (one unknown glyph); OCR, asked to fill it in, read the whole title over the busy
    // backdrop as just 'Ore', which names nothing.
    let items = catalog(&["Silver Ore", "Iron Ore", "Mithril Ore", "Silver Earrings"]);
    let name = "tooltip_ocr_loses_letters_silver_ore.png";
    assert_eq!(hover(name, items, "Ore").as_deref(), Some("Silver Ore"));
}

#[test]
fn a_wildcard_that_fits_two_items_names_neither() {
    let items = catalog(&["AP Reset Scroll", "SP Reset Scroll", "Sniper Potion"]);
    assert!(items.lookup("?P Reset Scroll").is_none(), "AP or SP: a guess");
    assert_eq!(items.lookup("Sniper P?tion").map(|i| i.name.clone()).as_deref(), Some("Sniper Potion"));
}

/// A drawn tooltip: dark fill, title strokes from the bullet to `title_end`, a light border at `border` and a white
/// window behind it from there on (the shop's chat panel).
fn drawn_tooltip(title_end: usize, border: usize) -> maplelens_core::image::RgbImage {
    let mut image = maplelens_core::image::RgbImage::new(config::TOOLTIP_CAPTURE_SIZE.0 as usize, 120);
    for y in 0..image.height {
        for x in 0..image.width {
            let colour = if x >= border { [250, 250, 252] } else { [60, 60, 110] };
            image.set_pixel(x, y, colour);
        }
    }
    // Letters as 2 px strokes 4 px apart, a word gap every 40 px, on the title's rows.
    for x in (24..title_end).filter(|x| x % 4 < 2 && x % 40 > 3) {
        for y in 14..30 {
            image.set_pixel(x, y, [255, 255, 255]);
        }
    }
    image
}

#[test]
fn a_long_title_is_read_past_the_cut_up_to_its_border() {
    let width = |image| maplelens_core::vision::reader::title_mask(&image, WIDTH, 1).unwrap().width;
    // "Gloves Magic Attack Scroll: Intermediate": the game widened the tooltip to ~465 px for it.
    let long = width(drawn_tooltip(460, 466));
    assert!((450 - 18..466 - 18).contains(&long), "{long}"); // to its last letters, short of the border
    // A title ending before the cut is cut where it always was.
    assert_eq!(width(drawn_tooltip(300, 450)), WIDTH - 18);
    // Equipment (370 px, its border at the cut) may have the worn item's tooltip beside it: never read on into it.
    assert_eq!(width(drawn_tooltip(366, 368)), WIDTH - 18);
}
