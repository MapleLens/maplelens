//! The minimap's title: the street and the map's name, which the game prints in white under the MINI MAP bar
//! (pure). The text itself is read by OCR; this finds the window and cuts its two lines out as grey images, dark
//! ink on white paper. Grey, not a mask: the letters' smoothed edges are what keeps "Street" from reading as
//! "Stl eet" (Windows OCR, measured on both captures).
//!
//! The minimap sits in the client's top-left corner unless the player drags it away. Its title bar ends in an
//! orange WORLD button, whose right end is (nearly) the window's: everything right of it is scenery, often white
//! clouds or name tags that would read as text.

use super::Mask;
use crate::image::{GrayImage, RgbImage};

/// Measured at UI scale 1 (1920x1200, "Normal" UI) from the client's top-left corner.
/// The WORLD button's rows, and how far right it can be (the window grows with the map's name).
const BUTTON_ROWS: (f32, f32) = (14.0, 23.0);
const BUTTON_MAX_X: f32 = 900.0;
/// The button is about 51 px wide; shorter orange runs are scenery.
const BUTTON_MIN_W: f32 = 30.0;
/// The title bar is white from here to the -/+ buttons, under the "MINI MAP" letters.
const TITLE_ROW: f32 = 26.0;
const TITLE_LEFT: f32 = 10.0;
const TITLE_MIN_WHITE: f32 = 0.5;
/// Street and name lines lie between these rows; the window's edge is this far right of the button.
const TEXT_ROWS: (f32, f32) = (38.0, 90.0);
const EDGE_PAST_BUTTON: f32 = 4.0;
/// The window's light left edge runs down beside the text; the text starts further in.
const LEFT_BORDER: f32 = 6.0;
/// The title's letters are near-white on a light steel blue (153,187,204) with a dark shadow; their smoothed edges
/// reach down to ~200, and a cut that leaves them out breaks the letters apart for OCR ("Hjdllen Str").
const TEXT_MIN: u8 = 200;
/// A street or map name is at least this many rows of ink (the "MINI MAP" letters are smaller, and outside).
const MIN_LINE_ROWS: f32 = 6.0;
/// Words are at most ~9 px apart; a map icon's white rim, left of the text, is 16 px away.
const ICON_GAP: f32 = 12.0;

/// Whiteness 0..1 runs from the panel's blue (its darkest channel 153) to white.
const PAPER_FROM: u8 = 150;
/// Room around a line's ink, so OCR sees its edges.
const MARGIN: usize = 2;

/// The two lines of the minimap's title, ready for OCR: dark ink on white.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimapTitle {
    pub street: GrayImage,
    pub map: GrayImage,
    /// The minimap window's right edge, in the frame's pixels.
    pub right: usize,
}

fn is_orange([r, g, b]: [u8; 3]) -> bool {
    r > 200 && (120..190).contains(&g) && b < 80
}

fn is_white(p: [u8; 3]) -> bool {
    p.into_iter().min().unwrap_or(0) >= TEXT_MIN
}

/// The WORLD button's left and right ends, if a long enough orange run crosses the button rows.
fn world_button(frame: &RgbImage, scale: f32) -> Option<(usize, usize)> {
    let max_x = ((BUTTON_MAX_X * scale) as usize).min(frame.width);
    let rows = (BUTTON_ROWS.0 * scale).round() as usize..=(BUTTON_ROWS.1 * scale).round() as usize;
    let min_w = (BUTTON_MIN_W * scale) as usize;
    let mut best: Option<(usize, usize)> = None;
    for y in rows.filter(|&y| y < frame.height) {
        let mut x = 0;
        while x < max_x {
            if !is_orange(frame.pixel(x, y)) {
                x += 1;
                continue;
            }
            let start = x;
            // The button's letters are white: a short break inside the run is still the button.
            let mut gap = 0;
            let mut end = x;
            while x < max_x && gap <= 3 {
                if is_orange(frame.pixel(x, y)) {
                    end = x;
                    gap = 0;
                } else {
                    gap += 1;
                }
                x += 1;
            }
            if end + 1 - start >= min_w && best.is_none_or(|(s, e)| end - start > e - s) {
                best = Some((start, end));
            }
        }
    }
    best
}

/// Columns with ink in `mask`, cut into runs (start, end exclusive).
fn runs(mask: &Mask) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for x in 0..=mask.width {
        let ink = x < mask.width && (0..mask.height).any(|y| mask.get(x, y));
        match (ink, start) {
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

/// Row bands holding ink, (top, bottom exclusive).
fn bands(mask: &Mask) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for y in 0..=mask.height {
        let ink = y < mask.height && (0..mask.width).any(|x| mask.get(x, y));
        match (ink, start) {
            (true, None) => start = Some(y),
            (false, Some(s)) => {
                out.push((s, y));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The columns of the title: the widest cluster of inked columns, so a map icon's white rim on the left is left out.
fn text_columns(mask: &Mask, scale: f32) -> Option<(usize, usize)> {
    let gap = (ICON_GAP * scale).ceil() as usize;
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    for (start, end) in runs(mask) {
        match clusters.last_mut() {
            Some(last) if start - last.1 < gap => last.1 = end,
            _ => clusters.push((start, end)),
        }
    }
    clusters.into_iter().max_by_key(|(s, e)| e - s)
}

/// The minimap's street and map lines in `frame`, whose top-left is the client's, at UI `scale`.
pub fn find_title(frame: &RgbImage, scale: f32) -> Option<MinimapTitle> {
    let (button_left, button_right) = world_button(frame, scale)?;
    let title_y = (TITLE_ROW * scale).round() as usize;
    let title_from = (TITLE_LEFT * scale) as usize;
    if title_y >= frame.height || button_left <= title_from {
        return None;
    }
    let white = (title_from..button_left).filter(|&x| is_white(frame.pixel(x, title_y))).count();
    if (white as f32) < TITLE_MIN_WHITE * (button_left - title_from) as f32 {
        return None;
    }
    let edge = (button_right + (EDGE_PAST_BUTTON * scale).ceil() as usize).min(frame.width);
    let (top, bottom) = ((TEXT_ROWS.0 * scale) as usize, ((TEXT_ROWS.1 * scale).ceil() as usize).min(frame.height));
    if bottom <= top {
        return None;
    }
    let inside = ((LEFT_BORDER * scale).ceil() as usize).min(edge);
    let mut mask = Mask::new(edge, bottom - top);
    for y in top..bottom {
        for x in inside..edge {
            mask.set(x, y - top, is_white(frame.pixel(x, y)));
        }
    }
    let (left, right) = text_columns(&mask, scale)?;
    let text = mask.crop(left, 0, right, mask.height);
    let min_rows = (MIN_LINE_ROWS * scale).ceil() as usize;
    let lines: Vec<(usize, usize)> = bands(&text).into_iter().filter(|(t, b)| b - t >= min_rows).collect();
    let [street, map] = lines[..] else { return None };
    let line = |(band_top, band_bottom): (usize, usize)| {
        let pieces = runs(&text.crop(0, band_top, text.width, band_bottom));
        let (first, last) = (left + pieces.first()?.0, left + pieces.last()?.1);
        let (x0, x1) = (first.saturating_sub(MARGIN), (last + MARGIN).min(edge));
        let (y0, y1) = ((top + band_top).saturating_sub(MARGIN), (top + band_bottom + MARGIN).min(frame.height));
        Some(paper(&frame.crop(x0, y0, x1, y1)))
    };
    Some(MinimapTitle { street: line(street)?, map: line(map)?, right: edge })
}

/// How white each pixel is, as ink on paper: white letters become black, the blue panel white.
fn paper(image: &RgbImage) -> GrayImage {
    let span = (255 - PAPER_FROM) as u32;
    let mut out = GrayImage::new(image.width, image.height);
    for y in 0..image.height {
        for x in 0..image.width {
            let white = image.pixel(x, y).into_iter().min().unwrap_or(0).saturating_sub(PAPER_FROM) as u32;
            out.data[y * image.width + x] = 255 - (white * 255 / span).min(255) as u8;
        }
    }
    out
}
