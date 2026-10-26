//! Find where a tooltip starts when the game has slid it up to stay on screen (pure).
//!
//! A tooltip normally starts at the mouse + `config::TOOLTIP_OFFSET`, but one too tall for the room below the
//! mouse is pushed up until its bottom touches the bottom of the game. Its height is unknown until it is
//! found, so the only cue left is its title: the first row of title glyphs sits a fixed distance under the
//! tooltip's top edge, below a band of plain fill.

use crate::config;
use crate::image::RgbImage;
use crate::vision::TEXT_THRESHOLD;
use crate::vision::tooltip::{measure_width, tooltip_present};

/// Same columns the title reader reads.
const TITLE_COLS: (usize, usize) = (18, config::TOOLTIP_TITLE_WIDTH as usize);
/// A title glyph row has at least this many near-white pixels (descender rows: ~10).
const GLYPH_PIXELS_MIN: usize = 8;
/// Measured: first title glyph row is this far below the tooltip's top edge.
const TITLE_TOP_PAD: usize = 18;
/// Rows of plain fill that must precede it (the top border itself is excluded).
const CLEAR_ROWS: usize = 14;
/// Reading is the expensive step; real titles rank ahead of stray text lines.
const MAX_CANDIDATES: usize = 8;

/// Rows of `strip` (starting at the tooltip's left edge) where a tooltip could start, best first.
pub fn candidate_tops(strip: &RgbImage) -> Vec<usize> {
    let gray = strip.crop(TITLE_COLS.0, 0, TITLE_COLS.1, strip.height).to_gray();
    let glyph: Vec<bool> = (0..gray.height)
        .map(|y| {
            gray.data[y * gray.width..(y + 1) * gray.width].iter().filter(|&&v| v > TEXT_THRESHOLD).count()
                >= GLYPH_PIXELS_MIN
        })
        .collect();
    // Glyph rows seen before each row, so "any glyph in this window" is O(1).
    let mut seen = vec![0usize; glyph.len() + 1];
    for (i, &g) in glyph.iter().enumerate() {
        seen[i + 1] = seen[i] + g as usize;
    }
    let mut tops = Vec::new();
    for row in TITLE_TOP_PAD..glyph.len() {
        if glyph[row] && seen[row] - seen[row - CLEAR_ROWS] == 0 {
            let top = row - TITLE_TOP_PAD;
            // Tooltips never stack in one column: lines of text in the body of the one just found are not
            // tops (the fill test accepts a tooltip's dark lower half since it takes dark fill).
            if tops.last().is_some_and(|&last| top < last + config::TOOLTIP_MAX_HEIGHT as usize) {
                continue;
            }
            let image = strip.crop(0, top, strip.width, top + config::TOOLTIP_CAPTURE_SIZE.1 as usize);
            if tooltip_present(&image) && measure_width(&image, &config::TOOLTIP_WIDTHS).is_some() {
                tops.push(top);
                if tops.len() == MAX_CANDIDATES {
                    break;
                }
            }
        }
    }
    tops
}
