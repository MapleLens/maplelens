//! Measure the status bar at the bottom of the game, the ruler for how big the game draws its UI (pure).
//!
//! The bar (chat line, level, HP/MP/EXP, menu buttons) is centred at the bottom of the client and outlined by a
//! 1 px near-black line: one unbroken row on top, one column down each side. It is drawn at the same UI size as
//! tooltips and windows, in every "UI Size" mode, so its width tells the UI scale without knowing the mode.

use crate::image::RgbImage;

/// Outline pixels: every channel at most this (measured 1-9).
const DARK_MAX: u8 = 40;
/// The bar spans at least this much of the client (Ideal Ratio at 2560 px: ~31%).
const MIN_WIDTH_SHARE: f32 = 0.2;
/// The bar is centred; a dark line elsewhere is scenery.
const CENTRE_SLACK_PX: f32 = 6.0;
/// Rounded corners: the side outline stands this close to the ends of the top row.
const CORNER_PX: i32 = 6;
/// Share of the rows under the top outline where a side outline is dark (measured 0.91).
const SIDE_MIN_DARK: f32 = 0.6;
/// Inside the outline, keep this far from the sides.
const INSIDE_PX: usize = 10;
/// More dark than this inside the outline is a black area, not the bar.
const SOLID_SHARE: f32 = 0.8;

/// The status bar's outline in strip coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub x: usize,
    pub top: usize,
    pub width: usize,
}

/// Start and end (exclusive) of the longest run of `true`.
fn longest_run(row: &[bool]) -> (usize, usize) {
    let (mut best, mut start) = ((0, 0), None);
    for (x, &dark) in row.iter().chain([false].iter()).enumerate() {
        match (dark, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                if x - s > best.1 - best.0 {
                    best = (s, x);
                }
                start = None;
            }
            _ => {}
        }
    }
    best
}

/// The column around `around` that is dark the most below `top`, if it is dark enough to be an outline.
fn side(dark: &[Vec<bool>], top: usize, around: usize) -> Option<usize> {
    let below = &dark[top + 1..];
    if below.is_empty() {
        return None;
    }
    let width = dark[0].len() as i32;
    let (score, x) = (around as i32 - CORNER_PX..around as i32 + CORNER_PX)
        .filter(|&x| (0..width).contains(&x))
        .map(|x| (below.iter().filter(|row| row[x as usize]).count() as f32 / below.len() as f32, x as usize))
        .fold((f32::MIN, 0), |best, c| if c.0 > best.0 { c } else { best });
    (score >= SIDE_MIN_DARK).then_some(x)
}

/// Is the area under the top outline as dark as the outline itself? Then it is a black border or letterbox, not
/// the bar (whose inside is the chat line, gauges and buttons).
fn solid(dark: &[Vec<bool>], top: usize, left: usize, right: usize) -> bool {
    let rows = &dark[(top + 3).min(dark.len() - 1)..];
    let inside = (left + INSIDE_PX..right.saturating_sub(INSIDE_PX)).flat_map(|x| rows.iter().map(move |row| row[x]));
    let (dark_px, total) = inside.fold((0usize, 0usize), |(d, t), is_dark| (d + is_dark as usize, t + 1));
    total > 0 && dark_px as f32 > SOLID_SHARE * total as f32
}

/// The status bar in `strip`, the full-width bottom rows of the client, or None when it is not there (hidden in
/// the Cash Shop, or a menu covers its outline: better no answer than a wrong width).
pub fn find_bar(strip: &RgbImage) -> Option<Bar> {
    let dark: Vec<Vec<bool>> = (0..strip.height)
        .map(|y| {
            (0..strip.width)
                .map(|x| {
                    let [r, g, b] = strip.pixel(x, y);
                    r.max(g).max(b) <= DARK_MAX
                })
                .collect()
        })
        .collect();
    let centre = strip.width as f32 / 2.0;
    // Topmost first: the bar's outline is the first long centred line.
    for top in 0..strip.height.saturating_sub(2) {
        let (start, end) = longest_run(&dark[top]);
        let centred = ((start + end) as f32 / 2.0 - centre).abs() <= CENTRE_SLACK_PX;
        if ((end - start) as f32) < MIN_WIDTH_SHARE * strip.width as f32 || !centred {
            continue;
        }
        if let (Some(left), Some(right)) = (side(&dark, top, start), side(&dark, top, end))
            && !solid(&dark, top, left, right)
        {
            return Some(Bar { x: left, top, width: right - left + 1 });
        }
    }
    None
}
