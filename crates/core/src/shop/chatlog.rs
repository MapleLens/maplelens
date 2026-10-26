//! Lines of the shop's chat log: black text on white, newest at the bottom.

use super::layout::{Point, View, chat_log};
use super::lines::{Line, make_line};

/// A pixel row with anything this dark belongs to a text line.
const INK_ROW: u8 = 150;
/// The log is a white page with black text.
const PAGE_WHITE: u8 = 235;
/// A log full of visitors is ~84% white, one full of the owner's own long messages ~77% (less once resampled); a
/// tooltip or dialog over it drops far below.
const MIN_PAGE: f32 = 0.65;
/// Rows around a line kept so descenders and the glyph tops survive the vertical shift.
const PAD: usize = 2;
/// A tooltip's fill reaching over the page would read as a line. It is a dark purple-navy (measured (48, 46, 89):
/// green no higher than red); the store owner's own messages are slate-blue text (measured (90, 107, 128): green
/// well above red), which must not count. A few such pixels are anti-aliasing at the page's border.
const TOOLTIP_FILL: (u8, i32, i32) = (150, 25, 8);
const MAX_TOOLTIP: usize = 20;
/// The page holds black, grey and slate-blue text only (channels at most ~50 apart); a dialog over it (the store's
/// set-price window: pink labels, orange buttons) is strongly coloured. Its pieces otherwise read as chat lines.
const SATURATED: i32 = 80;
const MAX_SATURATED: usize = 20;
/// The game's hand pointer: pale cyan (221, 255, 255) and cyan-greys, green and blue both well above red (the page's
/// border is ~20 above, the owner's text ~17). Resting on the page it would read as a line of its own.
const POINTER_CYAN: i32 = 30;
const MAX_POINTER: usize = 15;

fn pointer([r, g, b]: [u8; 3]) -> bool {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    r >= 140 && g - r >= POINTER_CYAN && b - r >= POINTER_CYAN && (g - b).abs() <= 20
}

fn tooltip_fill([r, g, b]: [u8; 3]) -> bool {
    r.max(g).max(b) < TOOLTIP_FILL.0 && b as i32 - r as i32 > TOOLTIP_FILL.1 && g as i32 - r as i32 <= TOOLTIP_FILL.2
}

/// The log's lines; None while something (an item tooltip, a dialog, the pointer) lies over it.
pub fn read_lines(view: &View<'_>, anchor: Point) -> Option<Vec<Line>> {
    let (x, y, w, h) = chat_log(anchor);
    let page = view.crop((x, y, w, h));
    let tooltip = page.data.as_chunks::<3>().0.iter().filter(|p| tooltip_fill([p[0], p[1], p[2]])).count();
    let saturated = page
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| i32::from(p[0].max(p[1]).max(p[2])) - i32::from(p[0].min(p[1]).min(p[2])) > SATURATED)
        .count();
    let pointed = page.data.as_chunks::<3>().0.iter().filter(|p| pointer([p[0], p[1], p[2]])).count();
    if tooltip > MAX_TOOLTIP || saturated > MAX_SATURATED || pointed > MAX_POINTER {
        return None;
    }
    let gray = page.to_gray();
    let white = gray.data.iter().filter(|&&v| v > PAGE_WHITE).count();
    if gray.data.is_empty() || (white as f32) < MIN_PAGE * gray.data.len() as f32 {
        return None;
    }
    let inked: Vec<bool> = (0..gray.height).map(|y| (0..gray.width).any(|x| gray.get(x, y) < INK_ROW)).collect();
    let mut lines = Vec::new();
    let mut start = None;
    for (row, has_ink) in inked.into_iter().chain([false]).enumerate() {
        match (has_ink, start) {
            (true, None) => start = Some(row),
            (false, Some(first)) => {
                let (top, bottom) = (first.saturating_sub(PAD), (row + PAD).min(gray.height));
                let mut line = make_line(&gray.crop(0, top, gray.width, bottom), None);
                line.source = Some(((x, y + top as i32, w, (bottom - top) as i32), false));
                lines.push(line);
                start = None;
            }
            _ => {}
        }
    }
    Some(lines)
}
