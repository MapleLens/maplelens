//! A QR code in MapleLens's colours, the same design as the web app's (`docs/app/mapleqr.js`) and the website's
//! (`examples/phone_qr.rs`): navy dots, the three finders as navy frames around a maple-orange button, the alignment
//! marks solid, and the meso coin on a white tile in the middle, which high error correction recovers
//! (`notify::phone::qr_modules`, `LOGO_SHARE`).

use super::canvas::Canvas;
use super::theme::Rgba;
use crate::notify::phone::LOGO_SHARE;

const INK: Rgba = [44, 50, 71, 255];
const PAPER: Rgba = [255, 255, 255, 255];
const ORANGE_TOP: Rgba = [208, 112, 10, 255];
const ORANGE_BOTTOM: Rgba = [138, 67, 0, 255];
const EDGE: Rgba = [255, 173, 31, 255];
/// Light modules around the code, as scanners expect.
pub const QUIET: usize = 4;
/// Alignment pattern centres per version (ISO/IEC 18004, annex E), versions 2 to 15.
const ALIGNMENT: [&[usize]; 16] = [
    &[],
    &[],
    &[6, 18],
    &[6, 22],
    &[6, 26],
    &[6, 30],
    &[6, 34],
    &[6, 22, 38],
    &[6, 24, 42],
    &[6, 26, 46],
    &[6, 28, 50],
    &[6, 30, 54],
    &[6, 32, 58],
    &[6, 34, 62],
    &[6, 26, 46, 66],
    &[6, 26, 48, 70],
];

/// The coin's tile in modules: odd, so it sits on the centre module.
pub fn logo_span(width: usize) -> usize {
    let span = ((width as f64 * LOGO_SHARE).round() as usize).max(7);
    if span % 2 == 1 { span } else { span + 1 }
}

/// Whether module (x, y) belongs to a finder pattern (drawn as a shape, not dots).
pub fn in_finder(x: usize, y: usize, width: usize) -> bool {
    let corner = |cx: usize, cy: usize| x >= cx && x < cx + 7 && y >= cy && y < cy + 7;
    corner(0, 0) || corner(width - 7, 0) || corner(0, width - 7)
}

/// The alignment patterns' centres (5x5 each). Drawn solid: as separate dots, the gaps between them broke the dark
/// ring a scanner looks for at large sizes.
pub fn alignment_centres(width: usize) -> Vec<(usize, usize)> {
    let at = ALIGNMENT.get(width.saturating_sub(17) / 4).copied().unwrap_or(&[]);
    at.iter().flat_map(|&y| at.iter().map(move |&x| (x, y))).filter(|&(x, y)| !in_finder(x, y, width)).collect()
}

/// Draws the code of `modules` (`width`² booleans, row by row) with its quiet zone, `size` design units square,
/// top-left at (x, y).
pub fn draw(cv: &mut Canvas<'_>, (x, y): (f32, f32), size: f32, (width, modules): (usize, &[bool])) {
    let unit = size / (width + 2 * QUIET) as f32;
    let at = |n: usize| (n + QUIET) as f32 * unit;
    cv.rounded((x, y, size, size), 3.0 * unit, Some(EDGE), None);
    cv.rounded((x + 0.6 * unit, y + 0.6 * unit, size - 1.2 * unit, size - 1.2 * unit), 2.6 * unit, Some(PAPER), None);

    let span = logo_span(width);
    let from = (width - span) / 2;
    let under_logo = |mx: usize, my: usize| mx >= from && mx < from + span && my >= from && my < from + span;
    let marks = alignment_centres(width);
    let in_mark = |mx: usize, my: usize| marks.iter().any(|&(cx, cy)| mx.abs_diff(cx) <= 2 && my.abs_diff(cy) <= 2);

    for (i, _) in modules.iter().enumerate().filter(|(_, dark)| **dark) {
        let (mx, my) = (i % width, i / width);
        if in_finder(mx, my, width) || in_mark(mx, my) || under_logo(mx, my) {
            continue;
        }
        // Touching neighbours, so a blurred camera image still sees solid runs.
        cv.circle(x + at(mx) + unit / 2.0, y + at(my) + unit / 2.0, unit / 2.0, INK, None);
    }
    let square = |cx: usize, cy: usize, n: f32, r: f32| (x + at(cx), y + at(cy), n * unit, n * unit, r * unit);
    for (cx, cy) in [(0, 0), (width - 7, 0), (0, width - 7)] {
        let (fx, fy, w, h, r) = square(cx, cy, 7.0, 2.2);
        cv.rounded((fx, fy, w, h), r, Some(INK), None);
        let (fx, fy, w, h, r) = square(cx + 1, cy + 1, 5.0, 1.5);
        cv.rounded((fx, fy, w, h), r, Some(PAPER), None);
        let (fx, fy, w, h, r) = square(cx + 2, cy + 2, 3.0, 0.9);
        cv.gradient((fx, fy, w, h), r, &[(0.0, ORANGE_TOP), (1.0, ORANGE_BOTTOM)]);
    }
    for &(cx, cy) in marks.iter().filter(|&&(mx, my)| !under_logo(mx, my)) {
        for (offset, n, r, colour) in [(2, 5.0, 1.4, INK), (1, 3.0, 0.8, PAPER), (0, 1.0, 0.3, INK)] {
            let (fx, fy, w, h, r) = square(cx - offset, cy - offset, n, r);
            cv.rounded((fx, fy, w, h), r, Some(colour), None);
        }
    }

    // The coin's tile: white, edged in orange, the meso in the middle.
    let tile = at(from) + 0.4 * unit;
    let tile_size = span as f32 * unit - 0.8 * unit;
    cv.rounded((x + tile, y + tile, tile_size, tile_size), 1.6 * unit, Some(EDGE), None);
    let inset = 0.5 * unit;
    let inner_tile = (x + tile + inset, y + tile + inset, tile_size - 2.0 * inset, tile_size - 2.0 * inset);
    cv.rounded(inner_tile, 1.3 * unit, Some(PAPER), None);
    let (cw, ch, rgba) = super::glance::coin();
    let inner = tile_size - 2.4 * unit;
    let k = inner / (*cw.max(ch) as f32);
    let (w, h) = (*cw as f32 * k, *ch as f32 * k);
    cv.image(x + tile + (tile_size - w) / 2.0, y + tile + (tile_size - h) / 2.0, w, h, (*cw, *ch, rgba));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::phone::{phone_url, qr_modules};

    #[test]
    fn the_pairing_code_keeps_its_patterns_clear_of_the_coin() {
        let key = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";
        let (width, _) = qr_modules(&phone_url(key)).unwrap();
        let span = logo_span(width);
        assert_eq!(span % 2, 1);
        assert!(span * span * 100 <= width * width * 5, "the coin hides about 4% of the modules");
        let from = (width - span) / 2;
        // Finders sit in the corners, far from the coin's tile.
        assert!(from > 8 && from + span < width - 8);
        assert!(in_finder(0, 0, width) && in_finder(width - 1, 6, width) && !in_finder(width - 1, width - 1, width));
        // Version 2+ has alignment marks, none inside a finder.
        let marks = alignment_centres(width);
        assert!(!marks.is_empty());
        assert!(marks.iter().all(|&(x, y)| !in_finder(x, y, width)));
        assert_eq!(alignment_centres(21), Vec::new(), "version 1 has none");
    }
}
