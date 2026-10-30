//! Measure the status bar at the bottom of the game, the ruler for how big the game draws its UI (pure).
//!
//! The bar (chat line, level, HP/MP/EXP, menu buttons) is centred at the bottom of the client and outlined by a
//! 1 px near-black line: one unbroken row on top, one column down each side. It is drawn at the same UI size as
//! tooltips and windows, in every "UI Size" mode, so its width tells the UI scale without knowing the mode.

use crate::image::RgbImage;

/// Outline pixels: every channel at most this (measured 1-9).
const DARK_MAX: u8 = 40;
/// At a fractional UI scale (0.9 at 1080p) the top outline is resampled over two rows, each blended with its
/// neighbour: under the open chat log it read 31-84 (measured live), never 40 or less. Only tried when no crisp
/// outline is found; the sides stay crisp and are still judged by `DARK_MAX`.
const SMEARED_TOP_MAX: u8 = 100;
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

/// Each pixel of `strip`: every channel at most `max`.
fn dark_mask(strip: &RgbImage, max: u8) -> Vec<Vec<bool>> {
    (0..strip.height)
        .map(|y| {
            (0..strip.width)
                .map(|x| {
                    let [r, g, b] = strip.pixel(x, y);
                    r.max(g).max(b) <= max
                })
                .collect()
        })
        .collect()
}

/// The status bar in `strip`, the full-width bottom rows of the client, or None when it is not there (hidden in
/// the Cash Shop, or a menu covers its outline: better no answer than a wrong width).
pub fn find_bar(strip: &RgbImage) -> Option<Bar> {
    let dark = dark_mask(strip, DARK_MAX);
    find_bar_in(&dark, &dark).or_else(|| find_bar_in(&dark_mask(strip, SMEARED_TOP_MAX), &dark))
}

/// The bar whose top outline is a long centred run in `top_dark`, with sides and inside judged on `dark`.
fn find_bar_in(top_dark: &[Vec<bool>], dark: &[Vec<bool>]) -> Option<Bar> {
    let (height, width) = (dark.len(), dark.first().map_or(0, Vec::len));
    let counts = dark_below(dark);
    let centre = width as f32 / 2.0;
    let min_width = MIN_WIDTH_SHARE * width as f32;
    // Topmost first: the bar's outline is the first long centred line.
    for (top, row) in top_dark.iter().enumerate().take(height.saturating_sub(2)) {
        let (start, end) = longest_run(row);
        if ((end - start) as f32) < min_width {
            continue;
        }
        let (Some(left), Some(right)) = (side(&counts, top, start, 1), side(&counts, top, end, -1)) else {
            continue;
        };
        let beside_black = left.beside_black || right.beside_black;
        let (left, right) = (left.x, right.x);
        // Black beside the bar makes the top row dark from edge to edge: then the sides, not the run, say where the bar
        // is (a row of a black area walks them past each other).
        let (from, to) = if beside_black { (left, right + 1) } else { (start, end) };
        let centred = ((from + to) as f32 / 2.0 - centre).abs() <= CENTRE_SLACK_PX;
        if right < left || (to.saturating_sub(from) as f32) < min_width || !centred {
            continue;
        }
        if !solid(dark, top, left, right) && !(beside_black && black_under(dark, top, left, right)) {
            return Some(Bar { x: left, top, width: right - left + 1 });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar at 1080p (UI scale 0.9) under the open chat log, as captured live: crisp sides, but the top outline
    /// smeared over two rows, blended with the chat log's border on the left and the quickslots' frame on the right.
    fn bar_under_open_chat() -> RgbImage {
        let (left, right, top) = (398, 1521, 84);
        let mut strip = RgbImage::new(1920, 180);
        for y in 0..strip.height {
            for x in 0..strip.width {
                let inside = (left..=right).contains(&x) && y > top;
                let v = match () {
                    _ if inside && (x == left || x == right) => 12,
                    _ if inside && y == top + 1 => 82,
                    _ if inside => 240,
                    _ if (left..=right).contains(&x) && y == top => {
                        if x < 1170 {
                            42
                        } else {
                            84
                        }
                    }
                    _ => 150,
                };
                strip.set_pixel(x, y, [v, v, v]);
            }
        }
        strip
    }

    #[test]
    fn a_smeared_top_outline_is_the_bar() {
        let strip = bar_under_open_chat();
        let dark = dark_mask(&strip, DARK_MAX);
        assert_eq!(find_bar_in(&dark, &dark), None);
        assert_eq!(find_bar(&strip), Some(Bar { x: 398, top: 84, width: 1124 }));
    }
}
