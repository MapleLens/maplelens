mod common;

use common::{fixture, fonts, local_name, now};
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::info::InfoDetector;
use maplelens_core::placement::place_beside_window;
use maplelens_core::ranking::{Rank, RankResult, Status};
use maplelens_core::render::rank_card::{CARD_WIDTH, render_rank_card};
use maplelens_core::vision::Mask;
use maplelens_core::vision::info_window;
use maplelens_core::vision::reader::{NoOcr, Ocr, OcrAnswer, TextReader, name_atlas};

/// Where the Character Info window sits in that screenshot.
const WINDOW: Rect = Rect::new(41, 68, 493, 280);
const SHOT: &str = "character_info_masterball.png";

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn finds_the_window_by_its_title() {
    let (x, y) = info_window::find_window(&fixture(SHOT).to_gray(), 1.0).expect("found");
    assert!((x - WINDOW.x).abs() <= 1 && (y - WINDOW.y).abs() <= 1);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn no_window_where_there_is_none() {
    let shot = fixture(SHOT);
    assert_eq!(info_window::find_window(&shot.crop(0, 330, shot.width, shot.height).to_gray(), 1.0), None);
}

/// The window as the game draws it in "Ideal Ratio" (UI scale 0.64), from a 1920x1200 capture; the player's name
/// is pixelated. The window's top-left in the crop:
const IDEAL_RATIO_WINDOW: (i32, i32) = (20, 14);
const IDEAL_RATIO_SHOT: &str = "character_info_ideal_ratio.png";

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn finds_the_window_at_ideal_ratio() {
    // The small UI has its own lettering: the 1x title shrunk to 0.64 scored 0.79 and the rank never showed.
    let (x, y) = info_window::find_window(&fixture(IDEAL_RATIO_SHOT).to_gray(), 0.64).expect("found");
    assert!((x - IDEAL_RATIO_WINDOW.0).abs() <= 1 && (y - IDEAL_RATIO_WINDOW.1).abs() <= 1, "{x},{y}");
    let window = Rect::new(x, y, 0, 0);
    let shot = fixture(IDEAL_RATIO_SHOT);
    let area = info_window::title_box(window, 0.64);
    let crop = shot.crop(area.x as usize, area.y as usize, area.right() as usize, area.bottom() as usize).to_gray();
    assert!(info_window::is_window(&crop, 0.64));
    let below = shot.crop(0, 60, shot.width, shot.height).to_gray();
    assert_eq!(info_window::find_window(&below, 0.64), None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn title_check_follows_the_window() {
    let shot = fixture(SHOT);
    let area = info_window::title_box(WINDOW, 1.0);
    let crop = shot.crop(area.x as usize, area.y as usize, area.right() as usize, area.bottom() as usize).to_gray();
    assert!(info_window::is_window(&crop, 1.0));
    let mut mirrored = crop.clone();
    for y in 0..crop.height {
        for x in 0..crop.width {
            mirrored.data[y * crop.width + x] = crop.get(crop.width - 1 - x, y);
        }
    }
    assert!(!info_window::is_window(&mirrored, 1.0));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn name_plate_is_read() {
    let shot = fixture(SHOT);
    let b = info_window::name_box(WINDOW, 1.0);
    let plate = shot.crop(b.x as usize, b.y as usize, b.right() as usize, b.bottom() as usize);
    let mut reader = TextReader::new(name_atlas(), Box::new(NoOcr));
    let text = reader.read_mask(&info_window::name_mask(&plate));
    assert_eq!(info_window::clean_name(&text).as_deref(), Some("masterball"));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn detector_reports_the_window_and_name() {
    let shot = fixture(SHOT);
    let bounds = Rect::new(0, 0, shot.width as i32, 1200); // reference height: UI scale 1
    let mut screen = FrameScreen { frame: &shot, origin: Point::default(), cursor: None };
    let mut detector = InfoDetector::new(TextReader::new(name_atlas(), Box::new(NoOcr)));
    let first = detector.step(&mut screen, bounds).expect("window found");
    assert_eq!(first.name, None, "one read is not trusted yet");
    assert_eq!((first.rect.x, first.rect.y), (WINDOW.x, WINDOW.y));
    // Found again in place, without a search; the second read agrees.
    let info = detector.step(&mut screen, bounds).expect("still there");
    assert_eq!(info.rect, first.rect);
    assert_eq!(info.name.as_deref(), Some("masterball"));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn one_odd_read_does_not_change_the_name() {
    // A window still appearing or being dragged reads as junk for a poll; the settled name stays.
    let shot = fixture(SHOT);
    let mut smudged = shot.clone();
    let plate = info_window::name_box(WINDOW, 1.0);
    for y in plate.y as usize..plate.bottom() as usize {
        for x in (plate.x as usize..plate.right() as usize).step_by(3) {
            let i = (y * smudged.width + x) * 3;
            smudged.data[i..i + 3].copy_from_slice(&[255, 255, 255]);
        }
    }
    let bounds = Rect::new(0, 0, shot.width as i32, 1200);
    let mut reader = TextReader::new(name_atlas(), Box::new(NoOcr));
    let plate = smudged.crop(plate.x as usize, plate.y as usize, plate.right() as usize, plate.bottom() as usize);
    assert_ne!(reader.read_mask(&info_window::name_mask(&plate)), "masterball", "the smudge changes the read");
    let mut detector = InfoDetector::new(TextReader::new(name_atlas(), Box::new(NoOcr)));
    let mut name = |frame| {
        let mut screen = FrameScreen { frame, origin: Point::default(), cursor: None };
        detector.step(&mut screen, bounds).and_then(|i| i.name)
    };
    assert_eq!(name(&shot), None);
    assert_eq!(name(&shot).as_deref(), Some("masterball"));
    assert_eq!(name(&smudged).as_deref(), Some("masterball"));
    assert_eq!(name(&shot).as_deref(), Some("masterball"));
}

#[test]
fn clean_name_rejects_read_junk() {
    assert_eq!(info_window::clean_name("master ball").as_deref(), Some("masterball"));
    assert_eq!(info_window::clean_name(""), None);
    assert_eq!(info_window::clean_name("a-b!"), None);
    assert_eq!(info_window::clean_name("mast?rball"), None);
}

#[test]
fn card_sits_left_of_the_window_else_right_else_nowhere() {
    let bounds = Rect::new(0, 0, 1280, 720);
    assert_eq!(place_beside_window(Rect::new(400, 100, 493, 280), (230, 238), 4, bounds), Some((166, 100)));
    assert_eq!(place_beside_window(Rect::new(41, 68, 493, 280), (230, 238), 4, bounds), Some((538, 68)));
    assert_eq!(place_beside_window(Rect::new(41, 68, 1200, 280), (230, 238), 4, bounds), None);
    assert_eq!(place_beside_window(Rect::new(400, 600, 493, 280), (230, 238), 4, bounds).map(|p| p.1), Some(720 - 238));
}

#[test]
fn card_renders_every_state() {
    for status in [Status::Loading, Status::Missing, Status::Failed] {
        let card = render_rank_card(fonts(), &RankResult::of(status), now(), 1.0);
        assert!(card.width == CARD_WIDTH as usize && (70..130).contains(&card.height), "{status:?}: {}", card.height);
    }
}

#[test]
fn ranked_card_shows_more_than_a_notice() {
    let rank = Rank {
        name: "masterball".into(),
        level: Some(27),
        job: Some("Thief".into()),
        fame: Some(3),
        overall_rank: Some(29516),
        job_rank: Some(824),
        fame_rank: Some(21312),
        botting: false,
        last_checked: Some("2026-10-22T10:16:24Z".into()),
        profile_url: None,
    };
    let botter = Rank { name: "b".into(), botting: true, last_checked: None, ..rank.clone() };
    let found = |r: Rank| {
        render_rank_card(fonts(), &RankResult { status: Status::Found, rank: Some(r), expires_at: 0.0 }, now(), 1.0)
    };
    let ranked = found(rank);
    assert!(ranked.height > render_rank_card(fonts(), &RankResult::of(Status::Missing), now(), 1.0).height);
    assert!(found(botter).height < ranked.height);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn window_is_found_when_the_game_is_drawn_smaller() {
    let scale = 0.9;
    let small = fixture(SHOT).resized(scale);
    let (x, y) = info_window::find_window(&small.to_gray(), scale).expect("found");
    assert!(
        (x as f32 - WINDOW.x as f32 * scale).abs() <= 2.0 && (y as f32 - WINDOW.y as f32 * scale).abs() <= 2.0,
        "{x},{y}"
    );
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn window_is_found_when_the_game_is_drawn_bigger() {
    let scale = 1.2;
    let big = fixture(SHOT).resized(scale);
    let (x, y) = info_window::find_window(&big.to_gray(), scale).expect("found");
    assert!(
        (x as f32 - WINDOW.x as f32 * scale).abs() <= 2.0 && (y as f32 - WINDOW.y as f32 * scale).abs() <= 2.0,
        "{x},{y}"
    );
    let card = render_rank_card(fonts(), &RankResult::LOADING, now(), scale);
    assert_eq!(card.width, (CARD_WIDTH * scale).round() as usize);
}

/// An OCR engine that reads one fixed text whatever it is shown.
struct Reads(&'static str);

impl Ocr for Reads {
    fn recognize(&mut self, _mask: &Mask) -> OcrAnswer {
        OcrAnswer::Text(self.0.to_string())
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn small_ui_names_never_go_through_ocr_or_the_atlas_when_the_font_knows_them() {
    // Letters learnt by the 1x atlas from the small pixel font misread every later name (22 names: 20 right by OCR
    // alone, 9 with learnt letters); the exact pixel font needs neither.
    let shot = fixture("character_info_ideal_ratio_owner.png");
    let bounds = Rect::new(0, 0, shot.width as i32, shot.height as i32);
    let mut screen = FrameScreen { frame: &shot, origin: Point::default(), cursor: None };
    let mut detector = InfoDetector::new(TextReader::new(name_atlas(), Box::new(Reads("Wrong"))));
    detector.step_at_scale(&mut screen, bounds, 0.64);
    let info = detector.step_at_scale(&mut screen, bounds, 0.64).expect("window found");
    let owner = local_name("owner");
    assert_eq!(info.name.as_deref(), Some(owner.as_str()));
    assert_eq!(detector.reader_mut().confirm(&owner, &owner), 0, "the 1x atlas learns nothing");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_plate_with_no_letters_names_no_one() {
    // The Ideal Ratio fixture's name is pixelated away: whatever OCR says, there is nothing to read.
    let shot = fixture(IDEAL_RATIO_SHOT);
    let bounds = Rect::new(0, 0, shot.width as i32, shot.height as i32);
    let mut screen = FrameScreen { frame: &shot, origin: Point::default(), cursor: None };
    let mut detector = InfoDetector::new(TextReader::new(name_atlas(), Box::new(Reads("Tester"))));
    detector.step_at_scale(&mut screen, bounds, 0.64);
    assert_eq!(detector.step_at_scale(&mut screen, bounds, 0.64).expect("window found").name, None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn small_ui_names_are_read_exactly_from_the_pixel_font() {
    // The player's own window at Ideal Ratio: every letter is in the bundled font, so no OCR is needed (NoOcr).
    let shot = fixture("character_info_ideal_ratio_owner.png");
    let bounds = Rect::new(0, 0, shot.width as i32, shot.height as i32);
    let mut screen = FrameScreen { frame: &shot, origin: Point::default(), cursor: None };
    let mut detector = InfoDetector::new(TextReader::new(name_atlas(), Box::new(NoOcr)));
    detector.step_at_scale(&mut screen, bounds, 0.64);
    let info = detector.step_at_scale(&mut screen, bounds, 0.64).expect("window found");
    assert_eq!(info.name, Some(local_name("owner")));
}

/// A 1920x1200 game frame at Ideal Ratio: a real Ideal Ratio capture tiled over it, the window pasted at `at`.
fn ideal_ratio_frame(at: Option<(usize, usize)>) -> maplelens_core::image::RgbImage {
    let tile = fixture("shop_ideal_ratio_open.png");
    let (w, h) = (1920, 1200);
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&tile.pixel(x % tile.width, y % tile.height));
        }
    }
    let mut frame = maplelens_core::image::RgbImage { width: w, height: h, data };
    if let Some((ax, ay)) = at {
        let window = fixture(IDEAL_RATIO_SHOT);
        for y in 0..window.height {
            for x in 0..window.width {
                let i = ((ay + y) * w + ax + x) * 3;
                frame.data[i..i + 3].copy_from_slice(&window.pixel(x, y));
            }
        }
    }
    frame
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_ideal_ratio_window_is_found_in_a_whole_game_frame() {
    // The whole-frame search shrinks the frame and the title together; a title capture too small to shrink (or one
    // the coarse pass loses) made it slow or blind.
    let (ax, ay) = (700, 500);
    let found = info_window::find_window(&ideal_ratio_frame(Some((ax, ay))).to_gray(), 0.64);
    let expected = ((ax as i32) + IDEAL_RATIO_WINDOW.0, (ay as i32) + IDEAL_RATIO_WINDOW.1);
    assert!(found.is_some_and(|(x, y)| (x - expected.0).abs() <= 1 && (y - expected.1).abs() <= 1), "{found:?}");
    assert_eq!(info_window::find_window(&ideal_ratio_frame(None).to_gray(), 0.64), None);
}

/// The search scans every second coarse position, so a window must be found wherever it sits, not only at the
/// positions the scan lands on: the same capture shifted by 0..5 px each way, at both UI scales.
#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_window_is_found_at_every_alignment() {
    for (shot, scale, at) in [(SHOT, 1.0, (WINDOW.x, WINDOW.y)), (IDEAL_RATIO_SHOT, 0.64, IDEAL_RATIO_WINDOW)] {
        let shot = fixture(shot);
        for dy in 0..5 {
            for dx in 0..5 {
                let mut frame = maplelens_core::image::RgbImage::new(shot.width + 8, shot.height + 8);
                for y in 0..shot.height {
                    for x in 0..shot.width {
                        frame.set_pixel(x + dx, y + dy, shot.pixel(x, y));
                    }
                }
                let found = info_window::find_window(&frame.to_gray(), scale);
                let want = (at.0 + dx as i32, at.1 + dy as i32);
                assert!(
                    found.is_some_and(|(x, y)| (x - want.0).abs() <= 1 && (y - want.1).abs() <= 1),
                    "scale {scale} shifted ({dx}, {dy}): {found:?}"
                );
            }
        }
    }
}
