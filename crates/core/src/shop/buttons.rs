//! What the shop window's button column says: is this our own, open store, or someone else's?
//!
//! Own open store:      SOLD ITEMS (green) / OPEN STORE (greyed out) / CLOSE STORE (orange)
//! Own, not open:       SOLD ITEMS (green) / OPEN STORE (orange)     / CLOSE STORE (orange)
//! Someone else's:      SELLER INFO (green) / BUY ITEM (orange)      / LEAVE STORE (orange)
//!
//! The buttons are the only saturated orange in the dialog, so colour finds them; the labels of the top and bottom
//! buttons separate our store (SOLD ITEMS, CLOSE STORE) from someone else's.

use std::sync::OnceLock;

use super::hsv;
use super::layout::{Point, View, template};
use crate::image::GrayImage;
use crate::vision::ncc::best_match;

/// Measured on our own shops: 0.86-1.0 (backdrops vary; the game's pointer over a button spoils that one label
/// only). On someone else's: SELLER INFO 0.41, LEAVE STORE 0.75.
const LABEL_MATCH: f32 = 0.8;
/// Search margin around where a label should be.
const LABEL_PAD: i32 = 8;

// Offsets from the ENTER anchor to the top-left of each button, and their common size.
const MIDDLE: Point = (-488, -455);
const BOTTOM: Point = (-488, -424);
const BUTTON: (i32, i32) = (118, 21);
/// Top-left of the SOLD ITEMS text inside the top button.
const SOLD_AT: Point = (-478, -501);
/// Top-left of the CLOSE STORE text inside the bottom button.
const CLOSE_AT: Point = (-480, -421);
/// Of the button's pixels that must have the colour (the text takes the rest).
const SHARE: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreState {
    Open,
    /// Our own store, being set up or just closed.
    NotOpen,
    /// Someone else's store.
    Visiting,
    /// Buttons not readable (covered by something).
    Unknown,
}

fn labels() -> &'static (GrayImage, GrayImage) {
    static LABELS: OnceLock<(GrayImage, GrayImage)> = OnceLock::new();
    LABELS.get_or_init(|| (template("label_sold_items").to_gray(), template("label_close_store").to_gray()))
}

/// Share of the button at `offset` whose colour is inside the OpenCV-style HSV box `low..=high`.
fn share(view: &View<'_>, (ax, ay): Point, (dx, dy): Point, low: (u8, u8, u8), high: (u8, u8, u8)) -> f32 {
    let area = view.crop((ax + dx, ay + dy, BUTTON.0, BUTTON.1));
    if (area.width as i32, area.height as i32) != BUTTON {
        return 0.0;
    }
    let inside = area
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| {
            let (h, s, v) = hsv([p[0], p[1], p[2]]);
            (low.0..=high.0).contains(&h) && (low.1..=high.1).contains(&s) && (low.2..=high.2).contains(&v)
        })
        .count();
    inside as f32 / (area.width * area.height) as f32
}

fn orange(view: &View<'_>, anchor: Point, offset: Point) -> bool {
    share(view, anchor, offset, (5, 140, 180), (20, 255, 255)) >= SHARE
}

fn grey(view: &View<'_>, anchor: Point, offset: Point) -> bool {
    share(view, anchor, offset, (0, 0, 90), (179, 30, 200)) >= SHARE
}

fn label_score(view: &View<'_>, (ax, ay): Point, label: &GrayImage, (dx, dy): Point) -> f32 {
    let (x, y) = (ax + dx - LABEL_PAD, ay + dy - LABEL_PAD);
    let area = (x, y, label.width as i32 + 2 * LABEL_PAD, label.height as i32 + 2 * LABEL_PAD);
    best_match(&view.crop(area).to_gray(), label).map_or(0.0, |(score, _, _)| score)
}

/// `enter` is the chat ENTER button anchor; everything is read at fixed offsets from it.
pub fn read_state(view: &View<'_>, enter: Point) -> StoreState {
    let (sold, close) = labels();
    let mine = label_score(view, enter, sold, SOLD_AT).max(label_score(view, enter, close, CLOSE_AT)) >= LABEL_MATCH;
    if !mine {
        return if orange(view, enter, BOTTOM) { StoreState::Visiting } else { StoreState::Unknown };
    }
    if grey(view, enter, MIDDLE) {
        StoreState::Open
    } else if orange(view, enter, MIDDLE) {
        StoreState::NotOpen
    } else {
        StoreState::Unknown
    }
}
