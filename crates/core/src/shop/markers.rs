//! The two markers everything in the shop is measured from, found by colour on the frame as captured (any size, any
//! UI scale), and the UI scale they are drawn at.
//!
//! - The shop window: its lime SOLD ITEMS (or SELLER INFO) button with the orange CLOSE STORE (or LEAVE STORE)
//!   button `PAIR_DY` below it, the same width. No other pair of such colours is stacked like that on screen, and the
//!   button's width is the window's own scale: the status bar's measure was wrong for two of ten live captures
//!   (Ideal Ratio at 1080p, a 1440p window), and every offset with it.
//! - The Sold Items dialog: its blue close box at the right end of a long white title bar, with list cells below.

use crate::image::RgbImage;

/// The SOLD ITEMS button at UI scale 1: width, and how far below its top the CLOSE STORE button's top is.
pub const BUTTON_W: f32 = 118.0;
pub const PAIR_DY: f32 = 80.0;
/// The buttons' height (to the rows the orange one may be looked at in).
const BUTTON_H: f32 = 18.0;
/// The Sold Items close box at UI scale 1 (side, px), and the white title bar at least this long to its left.
pub const CLOSE_BOX: f32 = 15.0;
const TITLE_BAR: f32 = 200.0;
/// UI scales the game can draw at (Ideal Ratio at 768p is the smallest seen, 0.64).
const SCALE_RANGE: (f32, f32) = (0.45, 1.8);
/// Rows scanned for a button: every other one finds a 14 px high button (0.64) in its text-free top rows.
const ROW_STEP: usize = 2;
/// A run's ends may differ by this between the two buttons (anti-aliased edges).
const EDGE_SLACK: i32 = 2;
/// The game's pointer over a button splits every row of it; runs this close (scaled) are one run.
const POINTER_GAP: f32 = 32.0;
/// A followed marker is looked for this far (scaled) around where it was before the whole frame is searched.
const HINT_PAD: f32 = 40.0;

/// Where the shop window is: the SOLD ITEMS button's top-left in frame pixels, and the scale its UI is drawn at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShopMarker {
    pub x: i32,
    pub y: i32,
    pub scale: f32,
}

/// The lime button, also lit up under the pointer (up to (229, 248, 23)).
fn green([r, g, b]: [u8; 3]) -> bool {
    g > 140 && r >= 100 && b < 110 && g as i32 - b as i32 > 100 && g >= r
}

/// The orange button, also lit up under the pointer.
fn orange([r, g, b]: [u8; 3]) -> bool {
    r > 190 && (80..=200).contains(&g) && b < 120 && r as i32 - g as i32 > 40
}

/// Close-box blue (white X strokes excluded: they are not part of the run then, and the box is found by its rows
/// above and below the X).
fn close_blue([r, g, b]: [u8; 3]) -> bool {
    b > 170 && b as i32 - r as i32 > 70 && g > 90
}

fn white([r, g, b]: [u8; 3]) -> bool {
    r.min(g).min(b) > 225
}

/// Runs of `test` in row `y` between `x0..x1`, as (start, end) with end exclusive.
fn runs(frame: &RgbImage, y: usize, (x0, x1): (usize, usize), test: fn([u8; 3]) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for x in x0..=x1 {
        let on = x < x1 && test(frame.pixel(x, y));
        match (on, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                out.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// `runs` with gaps of at most `gap` pixels closed.
fn bridged(runs: Vec<(usize, usize)>, gap: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in runs {
        match out.last_mut() {
            Some(last) if a - last.1 <= gap => last.1 = b,
            _ => out.push((a, b)),
        }
    }
    out
}

/// The run of `test` in row `y` (gaps up to `gap` bridged) that covers column `x`, looking `reach` pixels around it.
fn run_at(
    frame: &RgbImage,
    x: usize,
    y: usize,
    reach: usize,
    gap: usize,
    test: fn([u8; 3]) -> bool,
) -> Option<(usize, usize)> {
    if y >= frame.height {
        return None;
    }
    let span = (x.saturating_sub(reach), (x + reach).min(frame.width));
    bridged(runs(frame, y, span, test), gap).into_iter().find(|&(a, b)| a <= x && x < b)
}

/// Rows `y` of column band `x0..x1` that `test` covers mostly, top to bottom from `y` while they last.
fn extent_down(frame: &RgbImage, (x0, x1): (usize, usize), y: usize, test: fn([u8; 3]) -> bool) -> usize {
    let mostly = |y: usize| {
        let hits = (x0..x1).filter(|&x| test(frame.pixel(x, y))).count();
        hits * 2 > x1 - x0
    };
    let mut bottom = y;
    while bottom < frame.height && mostly(bottom) {
        bottom += 1;
    }
    bottom
}

/// The top row of a solid band containing row `y` (walks up while the band's columns mostly pass `test`).
fn extent_up(frame: &RgbImage, (x0, x1): (usize, usize), y: usize, test: fn([u8; 3]) -> bool) -> usize {
    let mostly = |y: usize| {
        let hits = (x0..x1).filter(|&x| test(frame.pixel(x, y))).count();
        hits * 2 > x1 - x0
    };
    let mut top = y;
    while top > 0 && mostly(top - 1) {
        top -= 1;
    }
    top
}

/// A green run in row `y` that is the top button of the pair: the orange button below matches it.
fn pair_from_run(frame: &RgbImage, (x0, x1): (usize, usize), y: usize) -> Option<ShopMarker> {
    let width = (x1 - x0) as f32;
    let scale = width / BUTTON_W;
    if !(SCALE_RANGE.0..=SCALE_RANGE.1).contains(&scale) {
        return None;
    }
    // The green button's top edge (text inside interrupts its middle rows; its top rows are solid).
    let top = extent_up(frame, (x0, x1), y, green);
    let below = (top as f32 + PAIR_DY * scale).round() as usize;
    let probe = (x0 + x1) / 2;
    // Any row of the orange button whose run lines up with the green one (the pointer may cover some of them).
    let rows = below.saturating_sub(2)..=below + (BUTTON_H * scale) as usize;
    let gap = (POINTER_GAP * scale) as usize;
    let (ox0, ox1, oy) = rows.into_iter().find_map(|oy| {
        let (a, b) = run_at(frame, probe, oy, x1 - x0, gap, orange)?;
        let lined_up = (a as i32 - x0 as i32).abs() <= EDGE_SLACK && (b as i32 - x1 as i32).abs() <= EDGE_SLACK;
        lined_up.then_some((a, b, oy))
    })?;
    let orange_top = extent_up(frame, (ox0, ox1), oy, orange);
    // Two measures of the scale, each to a pixel: the width and the distance between the buttons.
    let by_gap = (orange_top as f32 - top as f32) / PAIR_DY;
    if (by_gap - scale).abs() > 0.04 {
        return None;
    }
    Some(ShopMarker { x: x0 as i32, y: top as i32, scale: (scale + by_gap) / 2.0 })
}

fn search_shop(frame: &RgbImage, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> Option<ShopMarker> {
    let min_run = (BUTTON_W * SCALE_RANGE.0) as usize;
    let gap = (POINTER_GAP * SCALE_RANGE.0) as usize;
    (y0..y1).step_by(ROW_STEP).find_map(|y| {
        bridged(runs(frame, y, (x0, x1), green), gap)
            .into_iter()
            .filter(|(a, b)| b - a >= min_run)
            .find_map(|run| pair_from_run(frame, run, y))
    })
}

/// Clips a box around `(x, y)` to the frame.
fn around(frame: &RgbImage, x: i32, y: i32, w: f32, h: f32, pad: f32) -> (usize, usize, usize, usize) {
    let clip = |v: f32, max: usize| (v.max(0.0) as usize).min(max);
    (
        clip(x as f32 - pad, frame.width),
        clip(y as f32 - pad, frame.height),
        clip(x as f32 + w + pad, frame.width),
        clip(y as f32 + h + pad, frame.height),
    )
}

/// The shop window's marker; `hint` (the last one) is looked at first.
pub fn find_shop(frame: &RgbImage, hint: Option<ShopMarker>) -> Option<ShopMarker> {
    if let Some(h) = hint {
        let near = around(frame, h.x, h.y, BUTTON_W * h.scale, 30.0 * h.scale, HINT_PAD * h.scale);
        if let Some(found) = search_shop(frame, near) {
            return Some(found);
        }
    }
    search_shop(frame, (0, 0, frame.width, frame.height))
}

/// Top-left of the Sold Items dialog's close box in frame pixels, for a UI drawn at `scale`. `has_list` confirms a
/// candidate (the inventory's close box sits on a white bar too).
pub fn find_sold(
    frame: &RgbImage,
    scale: f32,
    hint: Option<(i32, i32)>,
    has_list: &dyn Fn((i32, i32)) -> bool,
) -> Option<(i32, i32)> {
    let side = CLOSE_BOX * scale;
    let search = |(x0, y0, x1, y1): (usize, usize, usize, usize)| {
        (y0..y1).step_by(ROW_STEP).find_map(|y| {
            runs(frame, y, (x0, x1), close_blue).into_iter().find_map(|(a, b)| {
                let w = (b - a) as f32;
                if (w - side).abs() > 2.0 + 0.1 * side {
                    return None;
                }
                // The box's rows (the X breaks the middle ones) and the title bar left of it.
                let top = extent_up(frame, (a, b), y, close_blue);
                let bottom = extent_down(frame, (a, b), y, close_blue);
                let tall = (bottom - top) as f32;
                if tall < side * 0.25 {
                    return None;
                }
                let bar_row = top + (side / 2.0) as usize;
                let bar_end = a.saturating_sub((3.0 * scale).ceil() as usize);
                let bar_start = bar_end.saturating_sub((TITLE_BAR * scale) as usize);
                if bar_row >= frame.height || bar_end <= bar_start + 10 {
                    return None;
                }
                let lit = (bar_start..bar_end).filter(|&x| white(frame.pixel(x, bar_row))).count();
                if (lit as f32) < 0.9 * (bar_end - bar_start) as f32 {
                    return None;
                }
                let at = (a as i32, top as i32);
                has_list(at).then_some(at)
            })
        })
    };
    if let Some((hx, hy)) = hint
        && let Some(found) = search(around(frame, hx, hy, side, side, HINT_PAD * scale))
    {
        return Some(found);
    }
    search((0, 0, frame.width, frame.height))
}
