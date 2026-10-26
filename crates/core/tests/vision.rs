//! Tooltip presence, width and title reading on real, native-resolution captures of the game.
//!
//! Each fixture's top-left pixel is the tooltip's top-left corner, as the pipeline captures it.

mod common;

use common::{fixture, fixture_from};
use maplelens_core::config;
use maplelens_core::image::RgbImage;
use maplelens_core::vision::reader::{NoOcr, TextReader, title_atlas};
use maplelens_core::vision::tooltip::{measure_width, tooltip_present};

fn reader() -> TextReader {
    TextReader::new(title_atlas(), Box::new(NoOcr))
}

/// Tooltips over very different backdrops (sky, equipment window, character, shop sign), plus one with
/// our own price card directly under it.
fn cases() -> Vec<(&'static str, RgbImage, &'static str)> {
    vec![
        ("emerald over a busy backdrop", fixture("tooltip_native_over_window.png"), "Emerald Ore"),
        ("emerald over sky", fixture("tooltip_native_over_sky.png"), "Emerald Ore"),
        ("octopus leg", fixture("tooltip_native_octopus_leg.png"), "Octopus Leg"),
        ("silver ore", fixture("tooltip_native_silver_ore.png"), "Silver Ore"),
        ("squishy with our card below", fixture_from("tooltip_squishy_liquid.png", 31, 33), "Squishy Liquid"),
    ]
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn tooltip_is_detected() {
    for (case, image, _) in cases() {
        assert!(tooltip_present(&image.crop(0, 0, 450, 46)), "{case}");
    }
}

#[test]
fn plain_frames_have_no_tooltip() {
    assert!(!tooltip_present(&RgbImage::filled(450, 46, [200, 200, 200])));
    assert!(!tooltip_present(&RgbImage::new(450, 46)));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn titles_read_exactly() {
    // Over a busy backdrop (shop sign text and windows behind the translucent box) the blue fill is no longer
    // reliable, so the pipeline trusts a recognised title instead; only the reading is asserted for it.
    let busy = ("mushroom spore, busy backdrop", fixture("tooltip_mushroom_spore_busy_backdrop.png"), "Mushroom Spore");
    let mut reader = reader();
    for (case, image, title) in cases().into_iter().chain([busy]) {
        assert_eq!(reader.read_title(&image.crop(0, 0, 450, 46), 450, 1), title, "{case}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn long_name_wraps_onto_second_title_line() {
    // 'Ink-and-Wash Painting / Name Tag Ring': the one-line read cuts the name off, the two-line read keeps it.
    let image = fixture("tooltip_cash_two_line_title_ink_wash_ring.png");
    let mut reader = reader();
    assert_eq!(reader.read_title(&image, 379, 1), "Ink-and-Wash Painting");
    assert_eq!(reader.read_title(&image, 379, 2), "Ink-and-Wash Painting Name Tag Ring");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn equipment_compare_reads_both_titles() {
    // Two equipment tooltips side by side: the first title must not leak into the second or vice versa.
    let strip = fixture("compare_zamadar_garnier_strip.png");
    let mut reader = reader();
    assert_eq!(reader.read_title(&strip, config::TOOLTIP_TITLE_WIDTH as usize, 1), "Beginner's Triangular Zamadar");
    let second = strip.crop(config::TOOLTIP_COMPARE_OFFSET_X as usize, 0, strip.width, strip.height);
    assert_eq!(reader.read_title(&second, second.width, 1), "Beginner's Garnier");
}

#[test]
fn empty_region_reads_nothing() {
    // A capture clipped at the screen edge leaves a zero-width compare strip.
    let mut reader = reader();
    let clipped = RgbImage::new(config::TOOLTIP_COMPARE_OFFSET_X as usize - 3, 120);
    let second = clipped.crop(config::TOOLTIP_COMPARE_OFFSET_X as usize, 0, clipped.width, clipped.height);
    assert_eq!(second.width, 0);
    assert_eq!(reader.read_title(&second, second.width, 1), "");
    assert_eq!(reader.read_title(&RgbImage::new(0, 0), 450, 1), "");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn equipment_names_read_through_the_kitty_hat() {
    // 'tty' touch in this font; the reader still finds the rest of the name and marks the unread run.
    let mut reader = reader();
    let text =
        reader.read_title(&fixture("tooltip_equip_kitty_camping_hat.png"), config::TOOLTIP_TITLE_WIDTH as usize, 1);
    assert!(text.ends_with("Camping Hat"), "{text}");
    let pants = reader.read_title(&fixture("tooltip_equip_shop_pants.png"), config::TOOLTIP_TITLE_WIDTH as usize, 1);
    assert_eq!(pants, "Lined Hip-Hop Pants");
}

const WIDTHS: [i32; 2] = [370, 450];

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn wide_tooltips_are_measured_as_450() {
    for name in [
        "tooltip_native_over_sky.png",
        "tooltip_native_over_window.png",
        "tooltip_native_octopus_leg.png",
        "tooltip_native_silver_ore.png",
    ] {
        assert_eq!(measure_width(&fixture(name).crop(0, 0, 10_000, 120), &WIDTHS), Some(450), "{name}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn equipment_tooltips_are_measured_as_370() {
    // Real shop captures. The item itself (a Cash Shop hat, not in the price list) is unknown to the catalog,
    // which is exactly why the width has to come from the screen.
    for name in ["tooltip_equip_kitty_camping_hat.png", "tooltip_equip_shop_pants.png"] {
        assert_eq!(measure_width(&fixture(name), &WIDTHS), Some(370), "{name}");
    }
}

#[test]
fn width_is_unknown_on_a_plain_frame() {
    assert_eq!(measure_width(&RgbImage::filled(743, 120, [120, 120, 120]), &WIDTHS), None);
}

/// The Python implementation's answers on every fixture (tests/golden/python.json) are the parity target.
#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn matches_the_python_implementation_on_every_fixture() {
    let golden: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(common::repo().join("tests/golden/python.json")).expect("golden file"),
    )
    .expect("json");
    for (name, expected) in golden["fixtures"].as_object().expect("fixtures") {
        let image = fixture(name);
        assert_eq!(
            tooltip_present(&image.crop(0, 0, 450, 46)),
            expected["present"].as_bool().unwrap(),
            "present {name}"
        );
        let width = measure_width(&image.crop(0, 0, image.width, 120), &config::TOOLTIP_WIDTHS);
        assert_eq!(width.map(i64::from), expected["width"].as_i64(), "width {name}");
        if let Some(tops) = expected.get("candidate_tops") {
            let tops: Vec<usize> = tops.as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
            assert_eq!(maplelens_core::vision::locate::candidate_tops(&image), tops, "candidate_tops {name}");
        }
        let window = maplelens_core::vision::info_window::find_window(&image.to_gray(), 1.0);
        let expected_window =
            expected["info_window"].as_array().map(|a| (a[0].as_i64().unwrap() as i32, a[1].as_i64().unwrap() as i32));
        assert_eq!(window, expected_window, "info_window {name}");
    }
}

/// Enhanced equipment has an orange title (measured 230,164,56 on a 1080p stream); its luminance is under the
/// text threshold, so the title once read as nothing. The same fixture recoloured must read the same.
#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn an_orange_title_reads_like_a_white_one() {
    let mut image = fixture("tooltip_native_silver_ore.png");
    for y in 0..40 {
        for x in 20..image.width {
            let [r, g, b] = image.pixel(x, y);
            if r.min(g).min(b) > 200 {
                image.set_pixel(x, y, [230, 164, 56]);
            }
        }
    }
    assert_eq!(reader().read_title(&image, 370, 1), "Silver Ore");
}

/// The translucent fill over dark scenery (measured on 1080p streams: (18,19,60) over a castle wall, (34,21,60)
/// over brown brick) is still the tooltip's fill; dark greys and other hues are not.
#[test]
fn dark_fill_counts_but_dark_scenery_does_not() {
    let band = |rgb: [u8; 3]| {
        let mut image = RgbImage::new(450, 120);
        for y in 0..120 {
            for x in 0..450 {
                image.set_pixel(x, y, rgb);
            }
        }
        tooltip_present(&image)
    };
    for fill in [[18, 19, 60], [34, 21, 60], [8, 6, 55], [26, 18, 55]] {
        assert!(band(fill), "{fill:?}");
    }
    for scenery in [[0, 0, 0], [60, 60, 60], [30, 30, 45], [60, 20, 20], [20, 50, 30], [40, 40, 120]] {
        assert!(!band(scenery), "{scenery:?}");
    }
}
