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
/// Right inside a side outline the bar is light: dark in at most this share of the rows (measured 0.06-0.09), where
/// scenery or black beside it is darker.
const INSIDE_MAX_DARK: f32 = 0.3;
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

/// Dark pixels in each column from each row down (`counts[y][x]`: rows y.. of column x; one row more than the strip),
/// so how much of a column is dark under a row is one lookup.
fn dark_below(dark: &[Vec<bool>]) -> Vec<Vec<u16>> {
    let width = dark.first().map_or(0, Vec::len);
    let mut counts = vec![vec![0u16; width]; dark.len() + 1];
    for y in (0..dark.len()).rev() {
        for x in 0..width {
            counts[y][x] = counts[y + 1][x] + dark[y][x] as u16;
        }
    }
    counts
}

/// A side outline, and whether it was found at the end of a black area.
#[derive(Clone, Copy)]
struct Side {
    x: usize,
    beside_black: bool,
}

/// The side outline at `around` (an end of the top row's run, the bar's inside toward `inward`, +1 or -1): the column
/// near it that is dark the most below `top`, if it is dark enough to be an outline.
///
/// The game draws black around a map smaller than the client (Kerning City Pharmacy). Under the top row that black is
/// as dark as the outline, so the column found can lie in it: when the column outside it is dark too, the outline is
/// the last column before the bar's light inside.
fn side(counts: &[Vec<u16>], top: usize, around: usize, inward: isize) -> Option<Side> {
    let width = counts[0].len();
    let rows = counts.len().saturating_sub(top + 2);
    if rows == 0 {
        return None;
    }
    let share = |x: usize| counts[top + 1][x] as f32 / rows as f32;
    let (score, found) = (around as i32 - CORNER_PX..around as i32 + CORNER_PX)
        .filter(|&x| (0..width as i32).contains(&x))
        .map(|x| (share(x as usize), x as usize))
        .fold((f32::MIN, 0), |best, c| if c.0 > best.0 { c } else { best });
    if score < SIDE_MIN_DARK {
        return None;
    }
    // Off the strip counts as dark outside: a black area reaching the client's edge.
    let outside = found.checked_add_signed(-inward).filter(|&x| x < width);
    if outside.is_some_and(|x| share(x) < SIDE_MIN_DARK) {
        return Some(Side { x: found, beside_black: false });
    }
    let mut x = found;
    while let Some(next) = x.checked_add_signed(inward).filter(|&n| n < width && share(n) > INSIDE_MAX_DARK) {
        x = next;
    }
    (share(x) >= SIDE_MIN_DARK).then_some(Side { x, beside_black: x != found })
}

/// Is the area under the top outline as dark as the outline itself? Then it is a black border or letterbox, not
/// the bar (whose inside is the chat line, gauges and buttons).
fn solid(dark: &[Vec<bool>], top: usize, left: usize, right: usize) -> bool {
    let rows = &dark[(top + 3).min(dark.len() - 1)..];
    let inside = (left + INSIDE_PX..right.saturating_sub(INSIDE_PX)).flat_map(|x| rows.iter().map(move |row| row[x]));
    let (dark_px, total) = inside.fold((0usize, 0usize), |(d, t), is_dark| (d + is_dark as usize, t + 1));
    total > 0 && dark_px as f32 > SOLID_SHARE * total as f32
}

/// Is the row under `top` dark between the sides (inside the rounded corners)? Then `top` is a row of a black area
/// above the bar, not its outline: the bar's inside starts right under the outline.
fn black_under(dark: &[Vec<bool>], top: usize, left: usize, right: usize) -> bool {
    let (from, to) = (left + CORNER_PX as usize, right.saturating_sub(CORNER_PX as usize));
    let inside = &dark[top + 1][from.min(to)..to];
    inside.iter().filter(|&&d| d).count() as f32 > SOLID_SHARE * inside.len() as f32
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
    let counts = dark_below(&dark);
    let centre = strip.width as f32 / 2.0;
    let min_width = MIN_WIDTH_SHARE * strip.width as f32;
    // Topmost first: the bar's outline is the first long centred line.
    for top in 0..strip.height.saturating_sub(2) {
        let (start, end) = longest_run(&dark[top]);
        if ((end - start) as f32) < min_width {
            continue;
        }
        let (mut left_side, mut right_side) = (side(&counts, top, start, 1), side(&counts, top, end, -1));
        // Another window's dark edge can run on from the outline (the quick slots' bottom joins its top row): then
        // the run's far end is not the bar's, and the side found at the near end, mirrored about the centre, is.
        let off_centre = ((start + end) as f32 / 2.0 - centre).abs() > CENTRE_SLACK_PX;
        if off_centre {
            let mirror = |x: usize| (2.0 * centre - x as f32).round().max(0.0) as usize;
            if let Some(l) = left_side.filter(|l| mirror(l.x) < end) {
                right_side = side(&counts, top, mirror(l.x), -1);
            } else if let Some(r) = right_side.filter(|r| mirror(r.x) > start) {
                left_side = side(&counts, top, mirror(r.x), 1);
            }
        }
        let (Some(left), Some(right)) = (left_side, right_side) else {
            continue;
        };
        let beside_black = left.beside_black || right.beside_black;
        let (left, right) = (left.x, right.x);
        // Black beside the bar makes the top row dark from edge to edge: then the sides, not the run, say where the bar
        // is (a row of a black area walks them past each other).
        let (from, to) = if beside_black || off_centre { (left, right + 1) } else { (start, end) };
        let centred = ((from + to) as f32 / 2.0 - centre).abs() <= CENTRE_SLACK_PX;
        if right < left || (to.saturating_sub(from) as f32) < min_width || !centred {
            continue;
        }
        if !solid(&dark, top, left, right) && !((beside_black || off_centre) && black_under(&dark, top, left, right)) {
            return Some(Bar { x: left, top, width: right - left + 1 });
        }
    }
    None
}
