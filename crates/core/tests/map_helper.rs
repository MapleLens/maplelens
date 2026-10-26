//! The map helper: reading world-map tooltip titles, matching them to maps, settling noisy reads, the card, and the
//! cache format. Reads that need real OCR are in the Windows crate.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{fixture, fonts};
use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::RgbImage;
use maplelens_core::mapinfo::data::{
    assemble, index_from_json, index_to_json, map_from_json, map_to_json, parse_map, parse_mob, parse_npc, parse_search,
};
use maplelens_core::mapinfo::resolve::{Candidate, key, parse_location, pick, search_terms};
use maplelens_core::mapinfo::vote::Voter;
use maplelens_core::mapinfo::{Location, MapInfo, Mob, Npc};
use maplelens_core::pipeline::Screen;
use maplelens_core::pipeline::hover::looks_like_title;
use maplelens_core::pipeline::map::MapScanner;
use maplelens_core::render::RgbaImage;
use maplelens_core::render::map_card::{SPINNER_FRAMES, render_loading_card, render_map_card};
use maplelens_core::vision::Mask;
use maplelens_core::vision::map_tooltip::{LineOcr, TextLine, find_tooltip, may_hold_tooltip, text_mask};

fn search() -> Vec<Candidate> {
    [
        ("Henesys", 100000000),
        ("Henesys Market", 100000100),
        ("Henesys Hunting Ground I", 104040000),
        ("Henesys Hunting Ground II", 104040001),
    ]
    .into_iter()
    .map(|(name, id)| Candidate { id, name: name.into(), street: "Victoria Road".into() })
    .collect()
}

fn info() -> MapInfo {
    MapInfo {
        id: 104040000,
        name: "Henesys Hunting Ground I".into(),
        street: "Victoria Road".into(),
        is_town: false,
        mobs: vec![
            Mob { id: 100101, name: "Blue Snail".into(), level: 2, exp: 4, hp: 15, count: 9 },
            Mob { id: 130101, name: "Red Snail".into(), level: 4, exp: 8, hp: 45, count: 3 },
        ],
        npcs: vec![Npc { id: 1, name: "Anne".into(), role: "Guide".into() }],
    }
}

fn town(npcs: u32) -> MapInfo {
    MapInfo {
        id: 1,
        name: "Henesys".into(),
        street: "Victoria Road".into(),
        is_town: true,
        mobs: Vec::new(),
        npcs: (0..npcs).map(|i| Npc { id: i, name: format!("Npc {i}"), role: "Role".into() }).collect(),
    }
}

fn no_icons(_: &str, _: u32) -> Option<Arc<RgbaImage>> {
    None
}

// Titles.

#[test]
fn parse_location_reads_street_and_name() {
    assert_eq!(parse_location("Victoria Road : Henesys"), Some(Location::new("Victoria Road", "Henesys")));
    assert_eq!(parse_location("Victoria Road ; Henesys Hunting Ground I").unwrap().name, "Henesys Hunting Ground I");
    assert_eq!(
        parse_location("Victoria Road Henesys Hunting Ground j"),
        Some(Location::new("", "Victoria Road Henesys Hunting Ground j")),
        "without a colon it is a bare name"
    );
    for text in ["", "Snail", "double-click to send whispers", "12:30 : 45", "Cure: 12"] {
        assert_eq!(parse_location(text), None, "{text:?}");
    }
}

#[test]
fn parse_location_drops_ocr_junk_around_the_title() {
    assert_eq!(parse_location("Victoria Road : Henesys Hunting Ground I l/").unwrap().name, "Henesys Hunting Ground I");
    assert_eq!(parse_location("i Warning Street : The Road to the Dungeon").unwrap().street, "Warning Street");
    assert_eq!(parse_location("Victoria Road : 3-Way Road-Split").unwrap().name, "3-Way Road-Split");
    assert_eq!(
        parse_location("Victoria Road \u{fffd} 3-VVay Road-Split .").unwrap().name,
        "Victoria Road 3-VVay Road-Split",
        "a symbol for the colon is dropped"
    );
}

#[test]
fn scroll_names_with_a_colon_stay_item_titles() {
    // PR #20 rejected item titles with a colon; 200+ scrolls are named like this.
    assert!(looks_like_title("Hat Accuracy Scroll: Greater"));
}

// Matching.

#[test]
fn pick_matches_name_and_street_despite_ocr_slips() {
    let search = search();
    assert_eq!(pick(&Location::new("Victoria Road", "Henesys Hunting Ground l"), &search), Some(104040000));
    assert_eq!(pick(&Location::new("Victoria Road", "Henesys"), &search), Some(100000000));
    assert_eq!(pick(&Location::new("", "Victoria Road Henesys Hunting Ground j"), &search), Some(104040000));
    assert_eq!(pick(&Location::new("Victoria Road", "Kerning City Subway"), &search), None);
}

#[test]
fn numbered_neighbours_do_not_confuse_the_match() {
    let search = search();
    assert_eq!(pick(&Location::new("Victoria Road", "Henesys Hunting Ground I"), &search), Some(104040000));
    assert_eq!(pick(&Location::new("Victoria Road", "Henesys Hunting Ground II"), &search), Some(104040001));
    let near_tie = [
        Candidate { id: 1, name: "Ground A".into(), street: "S".into() },
        Candidate { id: 2, name: "Ground B".into(), street: "S".into() },
    ];
    assert_eq!(pick(&Location::new("S", "Ground C"), &near_tie), None, "two equally likely maps: no guess");
}

#[test]
fn look_alike_glyphs_share_a_key_and_search_terms_go_specific_first() {
    assert_eq!(key(&Location::new("Victoria Road", "Ground l")), key(&Location::new("Victoria Road", "Ground I")));
    assert_eq!(
        search_terms(&Location::new("Victoria Road", "Henesys Hunting Ground I")),
        ["Henesys Hunting Ground I", "Henesys", "Hunting", "Ground"]
    );
}

// Finding the tooltip among OCR lines.

fn line(text: &str, x: i32, y: i32, w: i32) -> TextLine {
    TextLine::new(text, x, y, w, 16)
}

#[test]
fn find_tooltip_builds_the_box_around_title_and_description() {
    let lines = [
        line("Victoria Road : Henesys", 60, 20, 200),
        line("A bowman town located in a wide prairie", 10, 48, 300),
        line("where you can choose to become a", 10, 70, 280),
        line("Florina Beach", 400, 120, 200),
    ];
    let found = find_tooltip(&lines).unwrap();
    assert_eq!(found.location, Location::new("Victoria Road", "Henesys"));
    assert!(found.rect.x < 10 && found.rect.right() > 310);
    assert!(found.rect.y < 20 && found.rect.bottom() >= 86 && found.rect.bottom() < 120, "{:?}", found.rect);
}

#[test]
fn find_tooltip_joins_a_split_title_and_ignores_noise() {
    let found = find_tooltip(&[line("Henesys Hunting Ground I", 200, 130, 200), line("Victoria Road", 100, 131, 200)]);
    assert_eq!(found.unwrap().location.name, "Victoria Road Henesys Hunting Ground I");
    assert_eq!(find_tooltip(&[line("x", 0, 0, 5), line("Buy 10 for 5,000", 0, 30, 200)]), None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_scene_without_a_tooltip_is_not_scanned() {
    let scene = fixture("map_tooltip_town.png").crop(0, 0, 300, 150);
    assert!(!may_hold_tooltip(&scene, &text_mask(&scene)));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_sample_tooltips_hold_tooltip_colours_and_text() {
    for (name, (x, y, w, h)) in
        [("map_tooltip_town.png", (150, 150, 400, 150)), ("map_tooltip_hunting_ground.png", (100, 105, 500, 60))]
    {
        let image = fixture(name).crop(x, y, x + w, y + h);
        assert!(may_hold_tooltip(&image, &text_mask(&image)), "{name}");
    }
}

// Settling noisy readings.

#[test]
fn voter_settles_noisy_readings_and_holds_the_answer() {
    let mut voter = Voter::default();
    assert_eq!(voter.add("victoriaroadhenesyshuntinggroundi", "A"), None, "one reading is not agreement");
    assert_eq!(voter.add("victoriaroadhenesyshuntinggroundii", "B"), None);
    let held = voter.add("victoriaroadhenesyshuntinggroundi", "C").unwrap();
    assert_eq!(voter.add("cashshopmenu", "X"), Some(held), "a stray reading changes nothing");
    assert_eq!(voter.add("victoriaroadhenesyshuntinggroundi", "D"), Some(held));
}

#[test]
fn voter_switches_to_a_new_tooltip_once_it_repeats() {
    let mut voter = Voter::default();
    for _ in 0..3 {
        voter.add("henesyshuntinggroundi", "old");
    }
    let answers: Vec<Option<&str>> = (0..6).map(|_| voter.add("theforesteastofhenesys", "new")).collect();
    assert_eq!(answers.last(), Some(&Some("new")));
}

// The scanner, with a stand-in for OCR.

struct FixedLines(Vec<TextLine>);

impl LineOcr for FixedLines {
    fn read_lines(&mut self, _: &Mask) -> Vec<TextLine> {
        self.0.clone()
    }
}

struct Picture {
    image: RgbImage,
    cursor: Point,
}

impl Screen for Picture {
    fn grab(&mut self, area: Rect) -> RgbImage {
        let clip = |v: i32| v.max(0) as usize;
        self.image.crop(clip(area.x), clip(area.y), clip(area.right()), clip(area.bottom()))
    }

    fn cursor(&mut self) -> Option<Point> {
        Some(self.cursor)
    }
}

fn town_screen() -> Picture {
    let sample = fixture("map_tooltip_town.png");
    let mut image = RgbImage::new(1920, 1200);
    for y in 0..sample.height {
        for x in 0..sample.width {
            image.set_pixel(x, y, sample.pixel(x, y));
        }
    }
    Picture { image, cursor: Point { x: 127, y: 128 } } // the fingertip of the hand in the sample
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_scanner_waits_for_agreement_and_reports_the_tooltip_relative_to_the_cursor() {
    let mut screen = town_screen();
    let lines = vec![line("Victoria Road : Henesys", 40, 22, 260)];
    let mut scanner = MapScanner::new(Box::new(FixedLines(lines)));
    let bounds = Rect::new(0, 0, 1920, 1200);
    assert_eq!(scanner.step(&mut screen, bounds, 1.0, 0.0), None, "a reading must repeat before a card may show");
    assert_eq!(scanner.step(&mut screen, bounds, 1.0, 0.1), None);
    let hover = scanner.step(&mut screen, bounds, 1.0, 0.2).expect("three agreeing reads");
    assert_eq!(hover.location, Location::new("Victoria Road", "Henesys"));
    // The scan starts 8 px right of and below the cursor; the box pads the title by 10 / 8 px.
    assert_eq!(hover.offset, (8 + 40 - 10, 8 + 22 - 8));
    assert_eq!(hover.size, (260 + 20, 16 + 16));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_rejected_reading_is_ignored_so_another_can_win() {
    let mut screen = town_screen();
    let here = Location::new("Victoria Road", "Henesys");
    let mut scanner = MapScanner::new(Box::new(FixedLines(vec![line("Victoria Road : Henesys", 40, 22, 260)])));
    let bounds = Rect::new(0, 0, 1920, 1200);
    for t in 0..3 {
        scanner.step(&mut screen, bounds, 1.0, t as f64 * 0.1);
    }
    scanner.reject(&here, 1.0);
    for t in 0..5 {
        assert_eq!(scanner.step(&mut screen, bounds, 1.0, 1.0 + t as f64 * 0.1), None);
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn nothing_is_scanned_next_to_the_edge_of_the_game() {
    let mut screen = town_screen();
    screen.cursor = Point { x: 1900, y: 600 };
    let mut scanner = MapScanner::new(Box::new(FixedLines(vec![line("Victoria Road : Henesys", 0, 0, 200)])));
    for t in 0..4 {
        assert_eq!(scanner.step(&mut screen, Rect::new(0, 0, 1920, 1200), 1.0, t as f64), None);
    }
}

// The card.

#[test]
fn card_renders_with_and_without_sprites_and_scales() {
    let sprite = Arc::new(RgbaImage { width: 60, height: 50, data: [10, 200, 10, 255].repeat(60 * 50) });
    let with_sprites = render_map_card(fonts(), &info(), &|_, _| Some(sprite.clone()), 1.0, None);
    let without = render_map_card(fonts(), &info(), &no_icons, 1.0, None);
    assert_eq!((with_sprites.width, with_sprites.height), (without.width, without.height));
    assert_ne!(with_sprites.data, without.data);
    let double = render_map_card(fonts(), &info(), &no_icons, 2.0, None);
    assert!(double.width.abs_diff(2 * without.width) <= 1);
    assert!((without.height as f32) < 700.0 * 1.5);
}

#[test]
fn card_for_a_town_and_an_empty_map() {
    assert!(render_map_card(fonts(), &town(20), &no_icons, 1.0, None).height > 300);
    let empty = MapInfo { npcs: Vec::new(), name: "Quiet Place".into(), is_town: false, ..town(0) };
    assert!(
        render_map_card(fonts(), &empty, &no_icons, 1.0, None).height
            < render_map_card(fonts(), &town(20), &no_icons, 1.0, None).height
    );
}

#[test]
fn card_lists_everything_and_adds_columns_only_when_the_window_is_too_short() {
    let town = town(25);
    let tall = render_map_card(fonts(), &town, &no_icons, 1.0, Some((2000, 3000)));
    // Nothing is cut off: the same 25 NPCs flow into more columns when the window is short.
    let squeezed = render_map_card(fonts(), &town, &no_icons, 1.0, Some((2000, 800)));
    assert!(squeezed.height <= 800 && tall.height > 800);
    assert!(squeezed.width > tall.width);
    let narrow = render_map_card(fonts(), &town, &no_icons, 1.0, Some((300, 800)));
    assert_eq!(narrow.width, tall.width, "never wider than allowed: one column is all that fits");
}

#[test]
fn loading_card_animates_and_the_failure_notice_differs() {
    let frames: std::collections::HashSet<Vec<u8>> =
        (0..SPINNER_FRAMES).map(|i| render_loading_card(fonts(), "Henesys", i, false, 1.0).data).collect();
    assert_eq!(frames.len(), SPINNER_FRAMES);
    assert_ne!(
        render_loading_card(fonts(), "Henesys", 0, true, 1.0).data,
        render_loading_card(fonts(), "Henesys", 0, false, 1.0).data
    );
}

// maplestory.io responses and the disk cache.

#[test]
fn responses_parse_into_a_map() {
    let found = parse_search(br#"[{"name":"Henesys","streetName":"Victoria Road","id":100000000},{"bad":1}]"#);
    assert_eq!(found.unwrap(), [Candidate { id: 100000000, name: "Henesys".into(), street: "Victoria Road".into() }]);
    let outline = parse_map(
        br#"{"name":" Henesys Hunting Ground I ","streetName":"Victoria Road","isTown":false,
             "mobs":[{"id":2},{"id":1},{"id":1}],"npcs":[{"id":7},{"id":8},{"id":7}]}"#,
    )
    .unwrap();
    assert_eq!(outline.mobs, [(1, 2), (2, 1)], "most spawns first");
    assert_eq!(outline.npcs, [7, 8]);
    let mob = parse_mob(br#"{"name":"Blue Snail","meta":{"level":2,"exp":4,"maxHP":15}}"#, 100101, 9).unwrap();
    assert_eq!(mob, info().mobs[0]);
    assert_eq!(parse_mob(br#"{"name":"Event Snail"}"#, 5, 1), None, "no stats");
    let santa = parse_npc(br#"{"name":"Rudolph","function":"Christmas Event"}"#, 9).unwrap();
    let plain = parse_npc(br#"{"name":"Mia"}"#, 8).unwrap();
    let guide = parse_npc(br#"{"name":"Anne","function":"Guide"}"#, 7).unwrap();
    let map = assemble(104040000, outline, vec![mob], vec![santa, plain, guide]);
    assert_eq!(map.npcs.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Anne", "Mia", "Rudolph"]);
    assert_eq!(map.name, "Henesys Hunting Ground I");
}

#[test]
fn the_cache_keeps_maps_and_title_readings() {
    assert_eq!(map_from_json(&map_to_json(&info())), Some(info()));
    assert_eq!(map_from_json(&map_to_json(&info()).replace("\"version\":2", "\"version\":1")), None);
    assert_eq!(map_from_json("{broken"), None);
    let mut index = std::collections::HashMap::new();
    index.insert(key(&Location::new("Victoria Road", "Henesys Hunting Ground I")), 104040000);
    let again = index_from_json(&index_to_json(&index));
    assert_eq!(again.get(&key(&Location::new("Victoria Road", "Henesys Hunting Ground l"))), Some(&104040000));
}

#[test]
fn the_cache_reads_files_the_python_version_wrote() {
    let python = r#"{"version": 2, "id": 104040000, "name": "Henesys Hunting Ground I", "street": "Victoria Road",
        "is_town": false, "mobs": [{"id": 100101, "name": "Blue Snail", "level": 2, "exp": 4, "hp": 15, "count": 9},
        {"id": 130101, "name": "Red Snail", "level": 4, "exp": 8, "hp": 45, "count": 3}],
        "npcs": [{"id": 1, "name": "Anne", "role": "Guide"}]}"#;
    assert_eq!(map_from_json(python), Some(info()));
}

/// Counts OCR calls; reads `lines` every time.
struct Counting(Vec<TextLine>, Arc<AtomicUsize>);

impl LineOcr for Counting {
    fn read_lines(&mut self, _: &Mask) -> Vec<TextLine> {
        self.1.fetch_add(1, Ordering::Relaxed);
        self.0.clone()
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn ocr_rests_while_it_finds_no_map_tooltip() {
    // Name tags and chat pass the colour pre-check; an OCR scan every poll took several CPU cores in a town.
    let calls = Arc::new(AtomicUsize::new(0));
    let count = |c: &AtomicUsize| c.load(Ordering::Relaxed);
    let mut screen = town_screen();
    let bounds = Rect::new(0, 0, 1920, 1200);
    let mut idle = MapScanner::new(Box::new(Counting(Vec::new(), calls.clone())));
    let polls = (1.0 / config::MAP_POLL_S) as usize; // one second of polling
    for i in 0..polls {
        idle.step(&mut screen, bounds, 1.0, i as f64 * config::MAP_POLL_S);
    }
    assert!(count(&calls) <= 3, "{} OCR scans in a second with nothing to read", count(&calls));
    // Over a real tooltip every poll is read, so the card comes quickly.
    calls.store(0, Ordering::Relaxed);
    let lines = vec![line("Victoria Road : Henesys", 40, 22, 260)];
    let mut busy = MapScanner::new(Box::new(Counting(lines, calls.clone())));
    for i in 0..5 {
        busy.step(&mut screen, bounds, 1.0, i as f64 * config::MAP_POLL_S);
    }
    assert_eq!(count(&calls), 5);
}
