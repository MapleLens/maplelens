//! The Store Permit watcher on real captures (native 1920x1200, cropped; `frame` puts them back) and, for cases
//! no single capture can show, on synthetic observations. Reads that need real OCR are in the Windows crate.

use super::align::{new_at_back, new_at_front};
use super::buttons::{StoreState, read_state};
use super::layout::{self, Anchors, Point, View};
use super::lines::{Line, same};
use super::names::same_name_key;
use super::parse::{NO_NAME, parse_chat, parse_sale};
use super::scrollbar::{Thumb, thumb_position};
use super::soldlist::{self, Row};
use super::watcher::{Observation, STORE_GONE_AFTER, STORE_VANISHED_AFTER, ShopWatcher, settle_scale};
use super::{ListKind, ShopEvent, ShopStatus, chatlog};
use crate::image::{RgbImage, decode_png};
use crate::scale_vote::ScaleVote;
use crate::vision::Mask;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const ENTER: Point = (1062, 840);
const SOLD_X: Point = (1505, 246);
const CLIENT: (usize, usize) = (1920, 1200);

/// Where a fixture was cut out of the client, when not (340, 235).
fn origin(name: &str) -> (usize, usize) {
    match name {
        "sale" => (440, 160),
        "inventory_open_edge" | "sold_closed" | "sold_opened_after_sale" => (440, 150),
        "buttons_setup" => (361, 300),
        "buttons_visiting" => (357, 296),
        "ideal_ratio_open" => (600, 470),
        _ => (340, 235),
    }
}

fn frame(name: &str) -> RgbImage {
    let path = format!("{}/../../tests/fixtures/shop_{name}.png", env!("CARGO_MANIFEST_DIR"));
    let crop = decode_png(&std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap();
    let mut canvas = RgbImage::new(CLIENT.0, CLIENT.1);
    paste(&mut canvas, &crop, origin(name));
    canvas
}

fn paste(canvas: &mut RgbImage, part: &RgbImage, (x0, y0): (usize, usize)) {
    for y in 0..part.height.min(canvas.height - y0) {
        for x in 0..part.width.min(canvas.width - x0) {
            canvas.set_pixel(x0 + x, y0 + y, part.pixel(x, y));
        }
    }
}

/// `target[y0..y1, x0..x1] = source[y0..y1, x0..x1]`
fn splice(target: &mut RgbImage, source: &RgbImage, (x0, y0, x1, y1): (usize, usize, usize, usize)) {
    paste(target, &source.crop(x0, y0, x1, y1), (x0, y0));
}

fn fill(image: &mut RgbImage, (x0, y0, x1, y1): (usize, usize, usize, usize), value: u8) {
    for y in y0..y1 {
        for x in x0..x1 {
            image.set_pixel(x, y, [value; 3]);
        }
    }
}

fn black() -> RgbImage {
    RgbImage::new(CLIENT.0, CLIENT.1)
}

fn locate(image: &RgbImage) -> Anchors {
    layout::locate(image, None, &soldlist::has_list).map(|l| l.anchors).unwrap_or_default()
}

/// A fixture read as captured, at UI scale 1.
fn at1(image: &RgbImage) -> View<'_> {
    View { frame: image, scale: 1.0 }
}

fn reader(text: &'static str) -> super::watcher::ReadText {
    Box::new(move |_: &Mask| Some(text.to_string()))
}

/// Frames arrive about once a second; every frame is offered twice so it settles.
struct Feed {
    watcher: ShopWatcher,
    now: f64,
}

impl Feed {
    fn new() -> Self {
        Self::reading("x")
    }

    fn reading(text: &'static str) -> Self {
        Self { watcher: ShopWatcher::new(reader(text)), now: 0.0 }
    }

    fn show_n(&mut self, image: &RgbImage, times: usize, keep_state: bool) -> Vec<ShopEvent> {
        let mut events = Vec::new();
        for _ in 0..times {
            self.now += 1.0;
            events.extend(self.watcher.update(image, self.now));
        }
        events.retain(|e| keep_state || *e != ShopEvent::StoreOpened);
        events
    }

    fn show(&mut self, image: &RgbImage) -> Vec<ShopEvent> {
        self.show_n(image, 2, false)
    }
}

fn sales(events: &[ShopEvent]) -> usize {
    events.iter().filter(|e| matches!(e, ShopEvent::Sale { .. })).count()
}

fn covered(name: &str) -> RgbImage {
    let mut image = frame(name);
    fill(&mut image, (1250, 500, 1400, 560), 255); // stands in for the game's pointer sweeping across a row
    image
}

fn status_after(names: &[&str]) -> ShopStatus {
    let mut feed = Feed::new();
    for name in names {
        feed.show_n(&frame(name), 3, false);
    }
    feed.watcher.status()
}

// The double-count rules.

fn front(old: &str, new: &str) -> Option<usize> {
    let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    new_at_front(&old, &new, |a, b| a == b)
}

fn back(old: &str, new: &str) -> Option<usize> {
    let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    new_at_back(&old, &new, |a, b| a == b)
}

#[test]
fn alignment_rules() {
    assert_eq!((front("ABCD", "ABCD"), back("ABCD", "ABCD")), (Some(0), Some(0)));
    assert_eq!(front("ABCD", "XABC"), Some(1), "a new sale pushes the oldest row off the page");
    assert_eq!(front("ABCD", "XYAB"), Some(2));
    // TranzMog bought 1000 arrows twice: the list shows two identical rows, both are sales.
    assert_eq!(front("ST", "AAST"), Some(2));
    assert_eq!(front("AAST", "BAAS"), Some(1));
    assert_eq!(front("AAB", "AAB"), Some(0), "the same content seen again is not a second sale");
    assert_eq!((front("", "A"), front("", "")), (Some(1), Some(0)));
    assert_eq!(front("EFGH", "ABCD"), None, "a non-overlapping jump is refused");
    assert_eq!(front("ABCD", "XYZA"), None, "one row of overlap is not enough evidence");
    assert_eq!((back("ABC", "ABCD"), back("ABC", "ABCDE")), (Some(1), Some(2)));
    assert_eq!((back("ABCD", "BCDE"), back("ABCD", "CDEF")), (Some(1), Some(2)), "a full chat scrolls up");
    assert_eq!((back("abcdXY", "cdXYXY"), back("abcdXY", "abcdXY")), (Some(2), Some(0)));
    assert_eq!(back("ABCD", "EFGH"), None, "a burst longer than a page is lost, not guessed");
    assert_eq!(back("", "AB"), Some(2));
}

#[test]
fn one_spoiled_entry_is_forgiven_only_with_plenty_of_overlap() {
    assert_eq!(front("ABCD", "A?CD"), Some(0));
    assert_eq!(front("ABCDE", "XA?CD"), Some(1));
    assert_eq!(front("ABCD", "XYA?"), None);
    assert_eq!(back("ABCDE", "BC?EF"), Some(1));
    // Exact alignment wins over a forgiving one.
    assert_eq!(front("AAAB", "BAAA"), Some(1));
    assert_eq!(front("ABAB", "ABAB"), Some(0));
}

// Parsing OCR text.

#[test]
fn chat_lines() {
    let cases: [(&str, Option<ShopEvent>); 10] = [
        ("Glarfugus has entered.", Some(ShopEvent::PlayerEntered("Glarfugus".into()))),
        ("Glarfugus has left the room.", Some(ShopEvent::PlayerLeft("Glarfugus".into()))),
        ("Instant Tea has entered", Some(ShopEvent::PlayerEntered("Instant Tea".into()))), // no full stop
        ("tizz has left the rooms", Some(ShopEvent::PlayerLeft("tizz".into()))),           // stray glyph
        ("Omar hae left the room.", Some(ShopEvent::PlayerLeft("Omar".into()))),           // s read as e
        ("has entered.", Some(ShopEvent::PlayerEntered(NO_NAME.into()))),                  // name lost to OCR
        ("Zsin : hello there", Some(ShopEvent::ChatMessage("Zsin : hello there".into()))),
        ("", None), // redraw debris
        ("DLE", Some(ShopEvent::ChatMessage("DLE".into()))),
        ("i .", None),
    ];
    for (text, event) in cases {
        assert_eq!(parse_chat(text), event, "{text:?}");
    }
}

fn sale(item: &str, buyer: &str, quantity: u64) -> ShopEvent {
    ShopEvent::Sale { item: item.into(), buyer: buyer.into(), quantity, mesos: None, item_id: None }
}

#[test]
fn sale_lines() {
    assert_eq!(
        parse_sale("Arrows for Bows", "Buyer: TranzMog", "Number purchased: 1 000"),
        sale("Arrows for Bows", "TranzMog", 1000)
    );
    assert_eq!(parse_sale("Leaf", "Buyer: Korone", "Number purchased: 30"), sale("Leaf", "Korone", 30));
    // A half-read sale is still a sale.
    assert_eq!(parse_sale("Leaf", "3uyer: Korone", "garbled"), sale("Leaf", "3uyer: Korone", 1));
}

#[test]
fn console_lines() {
    assert_eq!(sale("Leaf", "Korone", 30).describe(), "SALE      Leaf x30  to Korone");
    assert_eq!(ShopEvent::PlayerEntered("tizz".into()).describe(), "ENTERED   tizz");
    assert_eq!(ShopEvent::PlayerLeft("tizz".into()).describe(), "LEFT      tizz");
    assert_eq!(ShopEvent::ChatMessage("hi".into()).describe(), "CHAT      hi");
    assert!(ShopEvent::StoreClosed.describe().starts_with("CLOSED"));
    assert!(ShopEvent::StoreOpened.describe().starts_with("OPENED"));
    assert!(ShopEvent::AmbiguousSale.describe().starts_with("SALE?"));
    assert!(ShopEvent::LostSync(ListKind::Chat).describe().starts_with("LOST SYNC the chat"));
}

// Reading the windows.

/// The scale each frame is read at, for these measures one a second, starting at t = 0.
fn scales_read(measures: &[f32]) -> Vec<f32> {
    let (mut vote, mut held) = (ScaleVote::default(), None);
    measures
        .iter()
        .enumerate()
        .map(|(t, &m)| {
            let scale = settle_scale(&mut vote, held, m, t as f64);
            held = Some(scale);
            scale
        })
        .collect()
}

#[test]
fn one_odd_measure_does_not_rescale_the_shop() {
    // The pointer over SOLD ITEMS once: the button reads 3% narrow, which would forget every list.
    assert_eq!(scales_read(&[1.2, 1.201, 1.163, 1.199, 1.2]), [1.2; 5]);
}

#[test]
fn the_first_measure_is_believed_and_jitter_is_held() {
    assert_eq!(scales_read(&[0.9, 0.905, 0.896]), [0.9; 3]);
}

#[test]
fn a_shop_redrawn_at_another_scale_is_followed_within_seconds() {
    let mut measures = vec![1.0; 6];
    measures.extend([1.282; 6]);
    let read = scales_read(&measures);
    let switched = read.iter().position(|&s| s == 1.282).expect("the new scale took over");
    assert!((6 + 2..=6 + 5).contains(&switched), "{read:?}");
    assert!(read[switched..].iter().all(|&s| s == 1.282));
}

#[test]
fn a_shop_back_after_a_gap_is_read_at_its_new_scale_at_once() {
    let (mut vote, held) = (ScaleVote::default(), Some(1.0));
    for t in 0..5 {
        settle_scale(&mut vote, held, 1.0, t as f64);
    }
    // The game switched resolution: no shop for 20 s, then the window at 1.2.
    assert_eq!(settle_scale(&mut vote, held, 1.2, 25.0), 1.2);
}

#[test]
fn a_shop_back_after_a_gap_at_its_scale_keeps_it() {
    // The pointer over SOLD ITEMS for 10 s (no measure), then a measure a pixel off: still the same scale, or every
    // list would be forgotten and a sale made meanwhile missed.
    let mut vote = ScaleVote::default();
    for t in 0..3 {
        settle_scale(&mut vote, Some(1.2), 1.2, t as f64);
    }
    assert_eq!(settle_scale(&mut vote, Some(1.2), 1.199, 12.0), 1.2);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn anchors_are_found_in_a_capture() {
    // The close box's found edge may be a pixel off its old template anchor; reading is relative to it either way.
    let anchors = locate(&frame("025123"));
    assert_eq!(anchors.enter, Some(ENTER));
    let (x, y) = anchors.sold_x.expect("the Sold Items dialog");
    assert!((x - SOLD_X.0).abs() <= 1 && (y - SOLD_X.1).abs() <= 1, "{:?}", (x, y));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn sold_rows_of_a_fresh_shop_end_with_the_totals_row() {
    let rows = soldlist::read_rows(&at1(&frame("025123")), SOLD_X).unwrap();
    assert_eq!(rows.iter().map(|r| r.is_total).collect::<Vec<_>>(), [false, false, true]);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_full_page_has_no_totals_row() {
    let rows = soldlist::read_rows(&at1(&frame("041720")), SOLD_X).unwrap();
    assert_eq!(rows.len(), 4);
    assert!(!rows.iter().any(|r| r.is_total));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn same_row_matches_across_slots() {
    // Two identical sales sit in different slots (fractional spacing) and still compare equal.
    let rows = soldlist::read_rows(&at1(&frame("030620")), SOLD_X).unwrap();
    assert!(soldlist::same(&rows[0], &rows[1]));
    assert!(!soldlist::same(&rows[0], &rows[2]));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_capture_reads_the_same_twice() {
    for name in ["025123", "030620", "041720"] {
        let (one, two) = (
            soldlist::read_rows(&at1(&frame(name)), SOLD_X).unwrap(),
            soldlist::read_rows(&at1(&frame(name)), SOLD_X).unwrap(),
        );
        assert!(one.len() == two.len() && one.iter().zip(&two).all(|(a, b)| soldlist::same(a, b)), "{name}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn real_scrollbars() {
    let image = frame("041720");
    let (column, track) = layout::sold_scroll(SOLD_X);
    assert_eq!(thumb_position(&at1(&image), column, track), Thumb::Start);
    let (column, track) = layout::chat_scroll(ENTER);
    assert_eq!(thumb_position(&at1(&image), column, track), Thumb::End);
}

#[test]
fn scroll_thumb_positions() {
    let mut column = RgbImage::filled(1, 400, [230; 3]);
    let track = (100, 300);
    let paint = |image: &mut RgbImage, rows: std::ops::Range<usize>, rgb: [u8; 3]| {
        for y in rows {
            image.set_pixel(0, y, rgb);
        }
    };
    let blue = [50, 100, 200]; // B well above R: an enabled thumb
    assert_eq!(thumb_position(&at1(&column), 0, track), Thumb::None);
    paint(&mut column, 100..130, blue);
    assert_eq!(thumb_position(&at1(&column), 0, track), Thumb::Start);
    paint(&mut column, 100..130, [230; 3]);
    paint(&mut column, 150..180, blue);
    assert_eq!(thumb_position(&at1(&column), 0, track), Thumb::Middle);
    paint(&mut column, 150..180, [230; 3]);
    paint(&mut column, 270..296, blue);
    assert_eq!(thumb_position(&at1(&column), 0, track), Thumb::End);
    // The arrow buttons reaching a pixel into the track (its ends placed a pixel off) are not the thumb.
    paint(&mut column, 80..101, blue);
    paint(&mut column, 299..320, blue);
    assert_eq!(thumb_position(&at1(&column), 0, track), Thumb::End);
    paint(&mut column, 270..296, [230; 3]);
    assert_eq!(thumb_position(&at1(&column), 0, track), Thumb::None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_chat_line_compares_equal_after_it_scrolls() {
    // The font is drawn at fractional offsets, so scrolled lines are not pixel-identical.
    let before = chatlog::read_lines(&at1(&frame("025420")), ENTER).unwrap();
    let after = chatlog::read_lines(&at1(&frame("025520")), ENTER).unwrap();
    assert!(after[..after.len() - 1].iter().any(|line| same(line, before.last().unwrap())));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_shop_dragged_elsewhere_over_another_backdrop_is_still_found() {
    // Captured live: a different shop position and backdrop scored 0.875 against the first anchor template.
    let image = frame("sale");
    let anchors = locate(&image);
    assert_eq!(anchors, Anchors { enter: Some((1189, 767)), sold_x: Some((1632, 173)) });
    let rows = soldlist::read_rows(&at1(&image), anchors.sold_x.unwrap()).unwrap();
    assert_eq!(rows.iter().map(|r| r.is_total).collect::<Vec<_>>(), [false, true]);
    assert_eq!(Feed::new().show(&image), [], "first sight: the sale is history");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_inventorys_look_alike_close_box_is_not_taken_for_the_sold_items_dialog() {
    // Captured live: with the inventory open, its close box outscored the real one. Also covers the pointer and an
    // item tooltip over the shop, and a Sold Items dialog at the screen edge.
    let image = frame("inventory_open_edge");
    let sold_x = locate(&image).sold_x.expect("the Sold Items dialog");
    assert!(sold_x.0 > 1600, "{sold_x:?}");
    let rows = soldlist::read_rows(&at1(&image), sold_x).unwrap();
    assert_eq!(rows.iter().map(|r| r.is_total).collect::<Vec<_>>(), [false, true]);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn candidates_at_the_screen_edge_have_no_list() {
    assert!(!soldlist::has_list(&at1(&frame("025123")), (1915, 246)));
    // Its list area would start at a negative x, which numpy read as "from the right edge".
    assert!(!soldlist::has_list(&at1(&frame("not_open")), (126, 9)));
    assert_eq!(locate(&frame("not_open")).sold_x, None);
}

// Which kind of store is on screen.

fn state_of(name: &str, enter: Point) -> StoreState {
    read_state(&at1(&frame(name)), enter)
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn store_states() {
    assert_eq!(state_of("025123", ENTER), StoreState::Open);
    assert_eq!(state_of("sale", (1189, 767)), StoreState::Open);
    assert_eq!(state_of("sold_closed", (1265, 838)), StoreState::Open);
    assert_eq!(state_of("buttons_setup", ENTER), StoreState::NotOpen);
    assert_eq!(state_of("buttons_visiting", ENTER), StoreState::Visiting);
    assert_eq!(read_state(&at1(&black()), ENTER), StoreState::Unknown);
}

// The watcher over time.

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn history_on_first_sight_is_not_news() {
    assert_eq!(Feed::new().show(&frame("025123")), []);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn two_identical_sales_are_two_sales_and_never_more() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    assert_eq!(sales(&feed.show(&frame("030620"))), 2);
    for _ in 0..5 {
        assert_eq!(feed.show(&frame("030620")), [], "the same list seen again adds nothing");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn sales_and_visitors_are_read_by_the_shops_own_font_without_ocr() {
    // The reader stands in for OCR and knows nothing: every word below comes from the font.
    let mut feed = Feed { watcher: ShopWatcher::new(Box::new(|_: &Mask| Some(String::new()))), now: 0.0 };
    feed.show(&frame("025123"));
    let events = feed.show(&frame("030620"));
    let arrows = ShopEvent::Sale {
        item: "Arrows for Bows".into(),
        buyer: "TranzMog".into(),
        quantity: 1000,
        mesos: None,
        item_id: None,
    };
    assert_eq!(events.iter().filter(|e| **e == arrows).count(), 2, "{events:?}");
    // A minute later the chat log has moved on by a line.
    feed.show(&frame("025420"));
    assert_eq!(
        feed.show(&frame("025520")),
        [ShopEvent::PlayerEntered("DreadLancer".into()), ShopEvent::PlayerLeft("DreadLancer".into())]
    );
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn one_sale_on_a_full_page() {
    let mut feed = Feed::new();
    feed.show(&frame("041620"));
    assert_eq!(sales(&feed.show(&frame("041720"))), 1);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_pointer_over_a_row_adds_nothing() {
    let mut feed = Feed::new();
    feed.show(&frame("041720"));
    let mut events = feed.show_n(&covered("041720"), 1, false);
    events.extend(feed.show(&frame("041720")));
    events.extend(feed.show(&frame("041720")));
    assert_eq!(events, []);
    let mut feed = Feed::new();
    feed.show(&frame("041720"));
    assert_eq!(sales(&feed.show_n(&covered("041720"), 6, false)), 0, "a row that stays covered invents nothing");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn chat_lines_are_counted_once_each() {
    let mut feed = Feed::reading("Foo has entered.");
    feed.show(&frame("034020"));
    assert_eq!(feed.show(&frame("034120")).len(), 3);
    assert_eq!(feed.show(&frame("034120")), []);
    let mut feed = Feed::new();
    feed.show(&frame("025420"));
    for _ in 0..4 {
        assert_eq!(feed.show(&frame("025420")), [], "an unchanged log has no news");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn store_closing_is_announced_once() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    assert_eq!(feed.show_n(&black(), 3, false), []);
    feed.now += STORE_VANISHED_AFTER;
    assert_eq!(feed.show_n(&black(), 1, false), [ShopEvent::StoreClosed]);
    assert_eq!(feed.show_n(&black(), 5, false), []);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_window_gone_while_the_game_switches_resolution_is_not_a_closing() {
    // Live at 1440p: the window was gone ~20 s and came back the same open store.
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    feed.show_n(&black(), 3, false);
    feed.now += 30.0;
    assert_eq!(feed.show_n(&black(), 3, false), []);
    assert_eq!(feed.show_n(&frame("025123"), 3, true), []);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_short_cover_up_is_not_a_closing() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    assert_eq!(feed.show(&black()), []);
    assert_eq!(feed.show(&frame("025123")), []);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_window_back_at_another_scale_starts_its_lists_afresh() {
    // Live: 1440p default ratio (1.2), a frame without the shop while the game switched, then Ideal Ratio (1.282).
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    feed.show(&black());
    let events = feed.show_n(&live("1440"), 3, true);
    assert!(!events.iter().any(|e| matches!(e, ShopEvent::LostSync(_))), "{events:?}");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_reopened_shop_starts_fresh() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    feed.show(&black());
    feed.now += STORE_VANISHED_AFTER;
    assert_eq!(feed.show_n(&black(), 1, false), [ShopEvent::StoreClosed]);
    assert_eq!(feed.show(&frame("030620")), [], "a new window's history is not news");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn lost_sync_is_reported_not_guessed() {
    let mut feed = Feed::new();
    feed.show(&frame("030620"));
    let events = feed.show(&frame("054321")); // nothing in common with the page before
    assert!(events.contains(&ShopEvent::LostSync(ListKind::Sold)));
    assert_eq!(sales(&events), 0);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn an_empty_frame_is_skipped_not_fatal() {
    // A minimized window delivers zero-sized frames; the Python watcher once died on one.
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    assert_eq!(feed.show_n(&RgbImage::new(0, 0), 3, false), []);
    feed.now += 100.0;
    assert_eq!(feed.show_n(&RgbImage::new(0, 0), 1, false), [], "and it is not mistaken for a closed shop");
    assert_eq!(sales(&feed.show(&frame("030620"))), 2);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_dragged_window_forgets_its_baseline_instead_of_reading_garbage() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    let original = frame("025123");
    let mut shifted = black();
    paste(&mut shifted, &original.crop(0, 0, CLIENT.0 - 60, CLIENT.1 - 40), (60, 40));
    assert_eq!(feed.show(&shifted), []);
    assert_eq!(feed.show(&shifted), []);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_sale_made_while_the_sold_items_window_was_closed_is_found_when_it_opens() {
    // Captured live (11:38-11:39): the dialog was closed, 5 mesos came in, then the dialog was opened.
    let mut feed = Feed::reading("Foo has entered.");
    assert_eq!(feed.show(&frame("sold_closed")), []);
    assert_eq!(sales(&feed.show(&frame("sold_opened_after_sale"))), 1);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn opening_is_announced_and_visiting_stores_are_ignored() {
    let mut feed = Feed::new();
    assert_eq!(feed.show_n(&frame("025123"), 2, true), [ShopEvent::StoreOpened]);
    assert_eq!(feed.show_n(&frame("025123"), 2, true), []);
    // A visited store has the same chat and a sold-looking list, but must never produce events.
    let mut visiting = frame("030620");
    splice(&mut visiting, &frame("buttons_visiting"), (361, 300, 719, 480));
    assert_eq!(Feed::new().show_n(&visiting, 4, true), []);
}

/// `name`'s frame with the store's buttons as they are while it is being set up.
fn being_set_up(name: &str) -> RgbImage {
    let mut setup = frame(name);
    splice(&mut setup, &frame("buttons_setup"), (361, 300, 719, 480));
    setup
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn visitors_right_after_opening_are_news_when_the_store_was_seen_being_set_up() {
    let dread = [ShopEvent::PlayerEntered("DreadLancer".into()), ShopEvent::PlayerLeft("DreadLancer".into())];
    let font_only = || Feed { watcher: ShopWatcher::new(Box::new(|_: &Mask| Some(String::new()))), now: 0.0 };
    // Seen set up with the page as it was, then open a line later: that line is news, the page before it is not.
    let mut feed = font_only();
    feed.show_n(&being_set_up("025420"), 3, false);
    assert_eq!(feed.show_n(&frame("025520"), 3, false), dread);
    // Its page covered while it was set up: everything after the page's first line (the owner entering) is news.
    let mut covered = being_set_up("025420");
    let (x, y, w, h) = layout::chat_log(ENTER);
    fill(&mut covered, (x as usize, y as usize, (x + w) as usize, (y + h) as usize), 128);
    let mut feed = font_only();
    feed.show_n(&covered, 3, false);
    let page = chatlog::read_lines(&at1(&frame("025520")), ENTER).expect("a readable page");
    assert_eq!(feed.show_n(&frame("025520"), 3, false).len(), page.len() - 1);
    // A store found already open has only history on its page.
    assert_eq!(font_only().show_n(&frame("025520"), 3, false), []);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn each_look_says_what_it_saw() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    let look = feed.watcher.look().to_string();
    assert!(look.contains("store Some(Open)") && look.contains("chat: no change"), "{look}");
    feed.show(&black());
    assert!(feed.watcher.look().starts_with("no shop window"), "{}", feed.watcher.look());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn our_store_going_back_to_not_open_is_a_closing() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    let mut setup = frame("025123");
    splice(&mut setup, &frame("buttons_setup"), (361, 300, 719, 480)); // buttons now read: not open
    assert_eq!(feed.show_n(&setup, 3, false), []);
    feed.now += STORE_GONE_AFTER;
    assert_eq!(feed.show_n(&setup, 1, false), [ShopEvent::StoreClosed]);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_pointer_over_a_button_does_not_close_the_store() {
    let mut feed = Feed::new();
    feed.show(&frame("025123"));
    let mut smudged = frame("025123");
    fill(&mut smudged, (570, 330, 700, 450), 128); // buttons unreadable
    feed.now += 10.0 * STORE_GONE_AFTER;
    assert_eq!(feed.show_n(&smudged, 3, false), []);
    assert_eq!(feed.show_n(&frame("025123"), 3, false), []);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_scaled_client_is_still_found_and_watched() {
    // The UI scales with the window (or not, in Ideal Ratio): the shop measures the scale it is drawn at itself.
    // A resized fixture is blurrier than the game's own drawing at that scale, so only finding the shop is
    // checked here; counting at other scales needs real captures.
    for scale in [0.9, 0.75, 0.64] {
        let small = frame("025123").resized(scale);
        let mut feed = Feed::new();
        let mut events = Vec::new();
        for _ in 0..3 {
            feed.now += 1.0;
            events.extend(feed.watcher.update(&small, feed.now));
        }
        assert_eq!(events, [ShopEvent::StoreOpened], "{scale}");
        assert!(feed.watcher.sees_shop(), "{scale}");
        assert_ne!(feed.watcher.status(), ShopStatus::Hidden, "{scale}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_live_ideal_ratio_store_is_found_and_read() {
    // Captured live at UI Size "Ideal Ratio" (UI scale 0.64), store open, Sold Items window closed.
    let image = frame("ideal_ratio_open");
    let mut feed = Feed::new();
    let mut events = Vec::new();
    for _ in 0..3 {
        feed.now += 1.0;
        events.extend(feed.watcher.update(&image, feed.now));
    }
    assert_eq!(events, [ShopEvent::StoreOpened]);
    assert_eq!(feed.watcher.status(), ShopStatus::NeedSoldWindow);
    let located = layout::locate(&image, None, &soldlist::has_list).expect("the shop window");
    assert!((located.scale - 0.64).abs() < 0.005, "{}", located.scale);
    let view = View { frame: &image, scale: located.scale };
    assert_eq!(chatlog::read_lines(&view, located.anchors.enter.unwrap()).map(|l| l.len()), Some(11));
}

/// A live capture of a client of another size, put back where it was cut out: (fixture, client size, origin).
fn live(name: &str) -> RgbImage {
    let ((w, h), origin) = match name {
        "1440" | "1440_tooltip" => ((2560, 1440), (700, 280)),
        "1080_ideal" => ((1920, 1080), (560, 300)),
        "768" => ((1366, 768), (370, 140)),
        _ => unreachable!("unknown live fixture {name}"),
    };
    let path = format!("{}/../../tests/fixtures/shop_live_{name}.png", env!("CARGO_MANIFEST_DIR"));
    let crop = decode_png(&std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap();
    let mut canvas = RgbImage::new(w, h);
    paste(&mut canvas, &crop, origin);
    canvas
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn live_stores_at_every_ui_size_are_found_at_their_own_scale_and_read() {
    // Captured live in one session while the player switched resolution and UI Size. The status bar said 0.90 for
    // the Ideal Ratio 1080p window and 1.20 for the second 1440p one; the shop window measures itself.
    for (name, scale, chat_lines) in [("768", 0.641, 11), ("1080_ideal", 0.641, 11), ("1440", 1.2, 11)] {
        let image = live(name);
        let located = layout::locate(&image, None, &soldlist::has_list).expect(name);
        assert!((located.scale - scale).abs() < 0.004, "{name}: {}", located.scale);
        let view = View { frame: &image, scale: located.scale };
        let enter = located.anchors.enter.unwrap();
        assert_eq!(read_state(&view, enter), StoreState::Open, "{name}");
        assert_eq!(chatlog::read_lines(&view, enter).map(|l| l.len()), Some(chat_lines), "{name}");
        // The only row is the totals row: no sale.
        let rows = soldlist::read_rows(&view, located.anchors.sold_x.expect(name)).expect(name);
        assert_eq!(rows.iter().map(|r| r.is_total).collect::<Vec<_>>(), [true], "{name}");
        let mut feed = Feed::new();
        assert_eq!(feed.show_n(&image, 3, true), [ShopEvent::StoreOpened], "{name}");
        assert_eq!(feed.watcher.status(), ShopStatus::Watching, "{name}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_tooltip_over_the_chat_page_is_a_cover_not_a_line() {
    // Captured live at 1440p (UI scale 1.28): a skill tooltip's edge reached into the page and read as a 12th line.
    let image = live("1440_tooltip");
    let located = layout::locate(&image, None, &soldlist::has_list).expect("the shop window");
    assert!((located.scale - 1.282).abs() < 0.004, "{}", located.scale);
    let view = View { frame: &image, scale: located.scale };
    assert_eq!(chatlog::read_lines(&view, located.anchors.enter.unwrap()), None);
    let mut feed = Feed::new();
    feed.show_n(&image, 3, true);
    assert_eq!(feed.watcher.status(), ShopStatus::Covered);
}

// Cases no single capture can show, from synthetic observations.

fn fake_line(seed: u64) -> Line {
    // A small LCG: deterministic "ink" that differs per seed.
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let profile = (0..247)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) as f32 / (1u64 << 31) as f32 * 500.0
        })
        .collect();
    Line { profile, glyphs: Mask::new(3, 3), source: None }
}

fn fake_row(seed: u64) -> Row {
    Row { lines: (0..3).map(|i| fake_line(seed * 10 + i)).collect(), is_total: false }
}

fn fake_observation(rows: Option<Vec<Row>>, balance: u8) -> Observation {
    Observation {
        sold: rows,
        sold_live: true,
        balance: Some(RgbImage::filled(141, 21, [balance; 3])),
        balance_area: None,
        chat: Some(Vec::new()),
        chat_live: true,
    }
}

/// The sold list's events for a synthetic observation (no frame behind it).
fn sold(watcher: &mut ShopWatcher, observation: &Observation) -> Vec<ShopEvent> {
    sold_at(watcher, observation, 0.0)
}

fn sold_at(watcher: &mut ShopWatcher, observation: &Observation, now: f64) -> Vec<ShopEvent> {
    let frame = RgbImage::new(1, 1);
    watcher.sold_update(&View { frame: &frame, scale: 1.0 }, observation, now)
}

/// A row whose glyphs say which sale it is (the browser's OCR reads by glyphs): `ocr_by_width` reads it back.
fn numbered_row(n: usize) -> Row {
    let mut row = fake_row(100 + n as u64);
    for (i, line) in row.lines.iter_mut().enumerate() {
        line.glyphs = Mask::new(10 * n + i + 1, 3);
    }
    row
}

fn ocr_by_width(width: usize) -> String {
    let width = width - 8; // OCR is given the glyphs with a 4 px margin
    match width % 10 {
        1 => format!("Item {}", width / 10),
        2 => "Buyer: Tikko".into(),
        _ => "Number purchased: 1".into(),
    }
}

/// OCR as the browser has it: nothing answered until `answering` is set.
fn late_reader(answering: &Arc<AtomicBool>) -> super::watcher::ReadText {
    let answering = answering.clone();
    Box::new(move |mask: &Mask| answering.load(Ordering::Relaxed).then(|| ocr_by_width(mask.width)))
}

fn items(events: &[ShopEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            ShopEvent::Sale { item, .. } => item.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_burst_of_sales_is_counted_while_their_text_is_still_being_read() {
    // One buyer took four items in a few seconds while the browser's OCR had not answered yet. Waiting for the text
    // before believing the page had let it move four rows on a page of four, which no longer aligns: all four were
    // lost. The page is followed at once; the sales wait for their text and come in order.
    let answering = Arc::new(AtomicBool::new(false));
    let mut watcher = ShopWatcher::new(late_reader(&answering));
    let mut page: Vec<Row> = (0..4).map(|i| fake_row(1 + i)).collect();
    sold(&mut watcher, &fake_observation(Some(page.clone()), 10));
    for n in 1..=4 {
        page.insert(0, numbered_row(n));
        page.truncate(4);
        assert_eq!(sold(&mut watcher, &fake_observation(Some(page.clone()), 10)), [], "sale {n} waits for its text");
        assert!(watcher.missing_text());
    }
    answering.store(true, Ordering::Relaxed);
    let events = sold(&mut watcher, &fake_observation(Some(page), 10));
    assert_eq!(items(&events), ["Item 1", "Item 2", "Item 3", "Item 4"]);
    assert!(!watcher.missing_text());
}

#[test]
fn a_sale_whose_text_never_comes_is_told_with_what_the_font_read() {
    let answering = Arc::new(AtomicBool::new(false));
    let mut watcher = ShopWatcher::new(late_reader(&answering));
    let page = vec![fake_row(1), fake_row(2)];
    sold_at(&mut watcher, &fake_observation(Some(page), 10), 0.0);
    let page = vec![numbered_row(1), fake_row(1), fake_row(2)];
    assert_eq!(sold_at(&mut watcher, &fake_observation(Some(page.clone()), 10), 1.0), []);
    assert_eq!(sold_at(&mut watcher, &fake_observation(Some(page.clone()), 10), 20.0), []);
    let events = sold_at(&mut watcher, &fake_observation(Some(page), 10), 31.0);
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(events[0], ShopEvent::Sale { .. }));
}

#[test]
fn a_page_of_identical_rows_is_ambiguous_when_the_balance_moves() {
    let page = vec![fake_row(1); 4];
    let mut watcher = ShopWatcher::new(reader("x"));
    assert_eq!(sold(&mut watcher, &fake_observation(Some(page.clone()), 10)), []);
    assert_eq!(sold(&mut watcher, &fake_observation(Some(page.clone()), 10)), [], "unmoved balance is quiet");
    assert_eq!(sold(&mut watcher, &fake_observation(Some(page), 90)), [ShopEvent::AmbiguousSale]);
}

#[test]
fn a_sale_on_a_repeating_page_is_a_plain_sale() {
    let mut watcher = ShopWatcher::new(reader("x"));
    sold(&mut watcher, &fake_observation(Some(vec![fake_row(1), fake_row(1), fake_row(2)]), 10));
    let events = sold(&mut watcher, &fake_observation(Some(vec![fake_row(3), fake_row(1), fake_row(1)]), 90));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ShopEvent::Sale { .. }));
}

#[test]
fn a_scrolled_list_is_ignored() {
    let mut watcher = ShopWatcher::new(reader("x"));
    let page = vec![fake_row(1), fake_row(2)];
    sold(&mut watcher, &fake_observation(Some(page.clone()), 10));
    let mut scrolled = fake_observation(Some(vec![fake_row(9), fake_row(1)]), 10);
    scrolled.sold_live = false;
    assert_eq!(sold(&mut watcher, &scrolled), []);
    assert_eq!(sold(&mut watcher, &fake_observation(Some(page), 10)), []);
}

#[test]
fn balance_change_with_the_dialog_closed_is_reported_as_an_unidentified_sale() {
    let mut watcher = ShopWatcher::new(reader("x"));
    assert_eq!(sold(&mut watcher, &fake_observation(None, 10)), []);
    assert_eq!(sold(&mut watcher, &fake_observation(None, 10)), []);
    assert_eq!(sold(&mut watcher, &fake_observation(None, 90)), [ShopEvent::SaleUnseen { mesos: None }]);
    // Opening the dialog then reveals what sold: the newest row, once.
    let page = vec![fake_row(1), fake_row(2)];
    let events = sold(&mut watcher, &fake_observation(Some(page.clone()), 90));
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ShopEvent::Sale { .. }));
    assert_eq!(sold(&mut watcher, &fake_observation(Some(page), 90)), []);
}

#[test]
fn balance_gain_is_read_from_the_counter() {
    // The counter is read once when first seen (the baseline), then whenever it moves.
    let mut readings = ["1,000", "1,250", "1,500"].into_iter();
    let mut watcher = ShopWatcher::new(Box::new(move |_: &Mask| Some(readings.next().unwrap_or_default().to_string())));
    sold(&mut watcher, &fake_observation(None, 10));
    assert_eq!(sold(&mut watcher, &fake_observation(None, 90)), [ShopEvent::SaleUnseen { mesos: Some(250) }]);
    assert_eq!(sold(&mut watcher, &fake_observation(None, 10)), [ShopEvent::SaleUnseen { mesos: Some(250) }]);
    assert_eq!(watcher.context().balance, Some(1_500));
}

#[test]
fn the_counter_reads_look_alike_letters_as_digits_and_otherwise_gives_no_amount() {
    // Read live at Ideal Ratio: 5,990 came out as "5,ggo".
    let mut readings = ["5,ggo", "6,250", "6,2?0", "6,300"].into_iter();
    let mut watcher = ShopWatcher::new(Box::new(move |_: &Mask| Some(readings.next().unwrap_or_default().to_string())));
    sold(&mut watcher, &fake_observation(None, 10));
    assert_eq!(sold(&mut watcher, &fake_observation(None, 90)), [ShopEvent::SaleUnseen { mesos: Some(260) }]);
    assert_eq!(sold(&mut watcher, &fake_observation(None, 10)), [ShopEvent::SaleUnseen { mesos: None }]);
    assert_eq!(sold(&mut watcher, &fake_observation(None, 90)), [ShopEvent::SaleUnseen { mesos: None }]);
}

// The status the pill shows.

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn statuses() {
    assert_eq!(ShopWatcher::new(reader("x")).status(), ShopStatus::Hidden);
    assert_eq!(status_after(&["025123"]), ShopStatus::Watching);
    assert_eq!(status_after(&["sold_closed"]), ShopStatus::NeedSoldWindow);
    assert_eq!(status_after(&["not_open"]), ShopStatus::NotOpen);
    assert_eq!(status_after(&["buttons_visiting"]), ShopStatus::Hidden);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn status_hides_once_the_shop_window_is_gone_but_not_for_one_odd_frame() {
    let mut feed = Feed::new();
    feed.show_n(&frame("025123"), 3, false);
    feed.show_n(&frame("sold_closed"), 1, false); // one frame without the Sold Items window
    assert_eq!(feed.watcher.status(), ShopStatus::Watching);
    feed.show_n(&black(), 3, false);
    assert_eq!(feed.watcher.status(), ShopStatus::Hidden);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn an_hour_of_captures_counts_every_sale_exactly_once() {
    // A store watched minute by minute (02:51 to 05:46), checked by eye against each Sold Items page: two
    // identical TranzMog sales, then Orange Mushroom Cap, then Mithril Ore and Brown Lamelle, then Iron Ore.
    let sequence = [
        ("025123", 0),
        ("025220", 0),
        ("025320", 0),
        ("025420", 0),
        ("025520", 0),
        ("030620", 2),
        ("034020", 0),
        ("034120", 0),
        ("041620", 0),
        ("041720", 1),
        ("054321", 2),
        ("054621", 1),
    ];
    let mut feed = Feed::new();
    for (name, expected) in sequence {
        let events = feed.show(&frame(name));
        assert_eq!(sales(&events), expected, "{name}: {events:?}");
        assert!(!events.contains(&ShopEvent::LostSync(ListKind::Sold)), "{name}: the sold list stayed in sync");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_owner_is_read_off_the_name_plate_and_the_balance_off_the_counter() {
    // The plate's letters are light on dark: the mask handed to the reader is a few letters wide, not the plate's edge.
    let widths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = widths.clone();
    let owner = local_name("owner");
    let plate = owner.clone();
    let mut watcher = ShopWatcher::new(Box::new(move |mask: &Mask| {
        seen.lock().unwrap().push((mask.width, mask.height));
        Some(if mask.height > 12 { plate.clone() } else { "39,311".to_string() })
    }));
    let image = frame("sale");
    for t in 0..4 {
        watcher.update(&image, f64::from(t));
    }
    let context = watcher.context();
    assert_eq!(context.owner.as_deref(), Some(owner.as_str()), "{:?}", widths.lock().unwrap());
    assert!(widths.lock().unwrap().iter().any(|&(w, _)| (20..=64).contains(&w)), "{:?}", widths.lock().unwrap());
}

/// A whole 1366x768 client drawn by the game's own engine (mscw-render), not a capture: an open store whose owner's name
/// is in the pixel font, on the plate and in the status bar.
fn render_768() -> RgbImage {
    let path = format!("{}/../../tests/fixtures/shop_render_768.png", env!("CARGO_MANIFEST_DIR"));
    decode_png(&std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

/// The owner's name in `render_768`'s status bar, as the renderer reported its box: (x, y, width, height).
const RENDER_768_HUD_NAME: (usize, usize, usize, usize) = (368, 752, 71, 9);

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_name_with_a_d_is_read_off_the_plate_by_the_font_alone() {
    // Names draw 'd' as 'c' and 'l' touching, so the plate read exactly as "...cl..." and the store kept that as its
    // owner (a player's 1366x768 screenshot, seen through the browser).
    let image = render_768();
    let mut watcher = ShopWatcher::new(reader(""));
    for t in 0..3 {
        watcher.update(&image, f64::from(t));
    }
    assert_eq!(watcher.context().owner, Some(local_name("render_owner")), "{:?}", watcher.owner_reads());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_status_bars_name_box_holds_a_whole_pixel_font_name_however_the_client_is_captured() {
    // The bar used to be looked for after resampling to reference pixels, which lost its outline at 1366x768 and in the
    // scaled-down capture, and the box cut a long name's last letter.
    let native = render_768();
    let retina = native.upscaled_nearest(2);
    // A browser's capture of the Retina window, scaled down to 1920 wide.
    let shrunk = retina.resized_to(1920, 1079);
    for (label, image) in [("native", &native), ("retina", &retina), ("shrunk", &shrunk)] {
        let located = layout::locate(image, None, &soldlist::has_list).expect(label);
        let view = View { frame: image, scale: located.scale };
        let area = super::watcher::hud_name_area(&view).unwrap_or_else(|| panic!("{label}: no status bar"));
        let (x0, y0, x1, y1) = view.frame_area(area);
        let k = image.width as f32 / native.width as f32;
        let (x, y, w, h) = RENDER_768_HUD_NAME;
        let name = [x, y, x + w, y + h].map(|v| v as f32 * k);
        let held = x0 as f32 <= name[0] && y0 as f32 <= name[1] && x1 as f32 >= name[2] && y1 as f32 >= name[3];
        assert!(held, "{label}: box {:?} for the name at {name:?}", (x0, y0, x1, y1));
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_owner_is_read_off_the_status_bar_when_the_plate_cannot_be() {
    // The plate's name painted over: the status bar is all there is. The reader knows the name only when it is given
    // whole (its ink as wide as the renderer drew it), not cut short by the box.
    let mut image = render_768();
    let paper = image.pixel(321, 263);
    for y in 261..273 {
        for x in 319..402 {
            image.set_pixel(x, y, paper);
        }
    }
    let owner = local_name("render_owner");
    let name = owner.clone();
    let mut watcher = ShopWatcher::new(Box::new(move |mask: &Mask| {
        let inked: Vec<usize> = (0..mask.width).filter(|&x| (0..mask.height).any(|y| mask.get(x, y))).collect();
        let span = inked.first().zip(inked.last()).map_or(0, |(first, last)| last - first + 1);
        Some(if mask.height < 30 && span == RENDER_768_HUD_NAME.2 { name.clone() } else { String::new() })
    }));
    for t in 0..4 {
        watcher.update(&image, f64::from(t));
    }
    assert_eq!(watcher.context().owner, Some(owner), "{:?}", watcher.owner_reads());
    assert!(watcher.owner_reads().iter().all(|r| r.starts_with("hud:")), "{:?}", watcher.owner_reads());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_owner_is_found_when_the_reads_differ_only_in_case_and_a_given_name_wins() {
    // Tesseract gave three spellings of one name, each as often as the others.
    let owner = local_name("owner");
    let alternating: String =
        owner.chars().enumerate().map(|(i, c)| if i % 2 == 0 { c.to_ascii_uppercase() } else { c }).collect();
    let ways = [alternating, owner.to_uppercase(), owner.to_lowercase()];
    let spellings = Arc::new(std::sync::Mutex::new(0usize));
    let counter = spellings;
    let read = move |mask: &Mask| {
        if mask.height > 12 {
            let mut n = counter.lock().unwrap();
            *n += 1;
            Some(ways[*n % 3].clone())
        } else {
            Some("39,311".to_string())
        }
    };
    let mut watcher = ShopWatcher::new(Box::new(read.clone()));
    let image = frame("sale");
    for t in 0..6 {
        watcher.update(&image, f64::from(t));
    }
    let found = watcher.context().owner.expect("one name read three ways is one name");
    assert_eq!(same_name_key(&found), same_name_key(&owner));
    // A hint from elsewhere settles the spelling.
    let mut hinted = ShopWatcher::new(Box::new(read.clone()));
    hinted.suggest_owner(Some(&owner));
    for t in 0..6 {
        hinted.update(&image, f64::from(t));
    }
    assert_eq!(hinted.context().owner.as_deref(), Some(owner.as_str()));
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_owner_is_spelled_as_the_chat_log_spells_it() {
    // The plate reads the name in capitals (case is unreliable there); the chat's first line has it as typed.
    let owner = local_name("owner");
    let (entered, plate) = (format!("{owner} has entered."), owner.to_uppercase());
    let mut watcher = ShopWatcher::new(Box::new(move |mask: &Mask| {
        Some(if mask.width > 100 {
            entered.clone()
        } else if mask.height > 12 {
            plate.clone()
        } else {
            "39,311".to_string()
        })
    }));
    let image = frame("sale");
    for t in 0..6 {
        watcher.update(&image, f64::from(t));
    }
    assert_eq!(watcher.context().owner.as_deref(), Some(owner.as_str()));
}

/// A player's name a local capture shows (`tests/fixtures/names.txt`, `key=name` lines): real names stay out of git
/// with the captures.
fn local_name(key: &str) -> String {
    let path = format!("{}/../../tests/fixtures/names.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no {key}= in {path}"))
        .trim()
        .to_string()
}

/// The status bar of a real 1920x1200 capture (its outline starts at x 335, y 1094), put back where it was cut from.
fn with_status_bar(mut canvas: RgbImage) -> RgbImage {
    let path = format!("{}/../../tests/fixtures/hud_status_bar.png", env!("CARGO_MANIFEST_DIR"));
    let bar = decode_png(&std::fs::read(&path).unwrap()).unwrap();
    // The game's scenery is not black: the bar finder looks for the bar's dark outline against it.
    for y in 1000..CLIENT.1 {
        for x in 0..CLIENT.0 {
            canvas.set_pixel(x, y, [96, 150, 70]);
        }
    }
    paste(&mut canvas, &bar, (335, 1094));
    canvas
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_owner_is_read_from_the_status_bar_where_the_name_stands_under_the_job() {
    // The mask given to OCR is the name alone: a few letters wide, not the job above it nor the level box.
    let widths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = widths.clone();
    let owner = local_name("owner");
    let hud = owner.clone();
    let mut watcher = ShopWatcher::new(Box::new(move |mask: &Mask| {
        seen.lock().unwrap().push((mask.width, mask.height));
        Some(if (45..=75).contains(&mask.width) && mask.height < 30 { hud.clone() } else { "39,311".to_string() })
    }));
    let image = with_status_bar(frame("sale"));
    for t in 0..6 {
        watcher.update(&image, f64::from(t));
    }
    assert_eq!(watcher.context().owner, Some(owner), "{:?}", widths.lock().unwrap());
    assert!(watcher.owner_reads().iter().any(|r| r.starts_with("hud:")));
}
