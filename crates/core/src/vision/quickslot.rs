//! The quickslots above the status bar's right end: where each slot is, and the item count drawn in its corner
//! (pure).
//!
//! The 4x2 quickslot window sits at a fixed place over the status bar's right end, drawn at the UI's size like the
//! bar, so the bar's outline is its ruler. Counts are the game's item-count digits: an 8x11 box per digit with a 1 px
//! black outline and a white-to-grey fill, drawn in the slot's bottom-left, each digit's box right after the last.
//! At Ideal Ratio they are drawn 1:1 and match exactly; at other sizes the digits are scaled the way the game scales
//! them and compared within a tolerance.

use super::hud::Bar;
use crate::geometry::Rect;
use crate::image::{RgbImage, luma};

pub const COLUMNS: usize = 4;
pub const ROWS: usize = 2;
pub const SLOTS: usize = COLUMNS * ROWS;
/// The first slot's inside from the bar's outline (x, top), the step to the next one, and a slot's inside, at 1x
/// (measured on Ideal Ratio and Normal captures: the same offsets at 1x and 1.5625x).
const ORIGIN: (f32, f32) = (657.0, -70.0);
const PITCH: (f32, f32) = (35.0, 34.0);
const SLOT: (f32, f32) = (32.0, 30.0);
/// The digits' boxes start at the slot's left edge, this far below its top.
const COUNT_TOP: f32 = 19.0;
/// The status bar is this wide where the UI is drawn 1:1.
const BAR_1X: f32 = 800.0;
/// Counts in the game stop at 4 digits for potions; 5 leaves room.
const MAX_DIGITS: usize = 5;
/// The share of a digit's pixels that may disagree (fill not light, outline not dark) for it to be there. Crisp
/// digits match exactly; scaled ones are smoothed at their edges.
const MAX_ERROR_CRISP: f32 = 0.02;
const MAX_ERROR_SCALED: f32 = 0.2;
/// A fill pixel is at least this light, an outline pixel at most this dark (the fill's darkest row is 195, the
/// outline 0; smoothing pulls both toward what is behind them).
const FILL_MIN: u8 = 110;
const OUTLINE_MAX: u8 = 70;

/// Within this of a whole number the game draws the UI with whole pixels.
const WHOLE_SCALE_SLACK: f32 = 0.02;
/// The fill's luma by box row (rows 1-9): white at the top, grey at the bottom; the outline is black. (The reader
/// only needs light against dark; `draw_count` draws with these.)
const FILL: [u8; 11] = [0, 255, 255, 255, 255, 250, 235, 215, 200, 195, 0];
const OUTLINE: u8 = 0;

/// The game's item-count digits: '#' fill, 'o' outline, ' ' see-through.
const DIGITS: [(u32, [&str; 11]); 10] = [
    (
        0,
        [
            "  oooo  ", " o####o ", "o#oooo#o", "o#o  o#o", "o#o  o#o", "o#o  o#o", "o#o  o#o", "o#o  o#o", "o#oooo#o",
            " o####o ", "  oooo  ",
        ],
    ),
    (1, ["   oo", " oo#o", "o###o", "ooo#o", "  o#o", "  o#o", "  o#o", "  o#o", "  o#o", "  o#o", "  ooo"]),
    (
        2,
        [
            "  oooo  ", " o####o ", "o#oooo#o", "oo   o#o", "     o#o", "   oo#o ", "  o##o  ", " o#oo   ", "o#oooooo",
            "o######o", "oooooooo",
        ],
    ),
    (
        3,
        [
            "  oooo  ", " o####o ", "o#oooo#o", "ooo  o#o", "  oooo#o", "  o###o ", "  oooo#o", "oo   o#o", "o#oooo#o",
            " o####o ", "  oooo  ",
        ],
    ),
    (
        4,
        [
            "     oo ", "    o#o ", "   o##o ", "  o#o#o ", " o#oo#o ", "o#ooo#oo", "o######o", "ooooo#oo", "    o#o ",
            "    o#o ", "    ooo ",
        ],
    ),
    (
        5,
        [
            "oooooooo", "o######o", "o#oooooo", "o#oooo  ", "o#####o ", " ooooo#o", "     o#o", "oo   o#o", "o#oooo#o",
            " o####o ", "  oooo  ",
        ],
    ),
    (
        6,
        [
            "   oooo ", "  o###o ", " o#oooo ", "o#oooo  ", "o#####o ", "o#oooo#o", "o#o  o#o", "o#o  o#o", "o#oooo#o",
            " o####o ", "  oooo  ",
        ],
    ),
    (
        7,
        [
            "oooooooo", "o######o", "oooooo#o", "    o#o ", "    o#o ", "   o#o  ", "   o#o  ", "  o#o   ", "  o#o   ",
            " o#o    ", " ooo    ",
        ],
    ),
    (
        8,
        [
            "  oooo  ", " o####o ", "o#oooo#o", "o#o  o#o", "o#oooo#o", " o####o ", "o#oooo#o", "o#o  o#o", "o#oooo#o",
            " o####o ", "  oooo  ",
        ],
    ),
    (
        9,
        [
            "  oooo  ", " o####o ", "o#oooo#o", "o#o  o#o", "o#o  o#o", "o#oooo#o", " o#####o", "  oooo#o", " ooooo#o",
            " o####o ", " ooooo  ",
        ],
    ),
];

/// The UI's scale against 1x, from the status bar.
pub fn scale_of(bar: Bar) -> f32 {
    bar.width as f32 / BAR_1X
}

/// The inside of quickslot `index` (left to right, then the second row) in the bar's image, given where the bar's
/// image sits (`origin`, added to every coordinate).
pub fn slot_rect(bar: Bar, origin: (i32, i32), index: usize) -> Rect {
    let k = scale_of(bar);
    let (column, row) = ((index % COLUMNS) as f32, (index / COLUMNS) as f32);
    let x = bar.x as f32 + (ORIGIN.0 + column * PITCH.0) * k;
    let y = bar.top as f32 + (ORIGIN.1 + row * PITCH.1) * k;
    Rect::new(
        origin.0 + x.round() as i32,
        origin.1 + y.round() as i32,
        (SLOT.0 * k).round() as i32,
        (SLOT.1 * k).round() as i32,
    )
}

/// The whole quickslot window's area (all slots), in the same coordinates as `slot_rect`.
pub fn area(bar: Bar, origin: (i32, i32)) -> Rect {
    let (first, last) = (slot_rect(bar, origin, 0), slot_rect(bar, origin, SLOTS - 1));
    Rect::new(first.x, first.y, last.right() - first.x, last.bottom() - first.y)
}

/// A digit's pixels at 1x: (x, y, is fill) for each fill and outline pixel.
struct Glyph {
    digit: u32,
    cells: Vec<(f32, f32, bool)>,
    /// The box's width at 1x: the next digit starts there.
    advance: f32,
}

fn glyph(digit: u32, rows: &[&str; 11]) -> Glyph {
    let cells = rows
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.bytes().enumerate().filter_map(move |(x, c)| match c {
                b'#' => Some((x as f32, y as f32, true)),
                b'o' => Some((x as f32, y as f32, false)),
                _ => None,
            })
        })
        .collect();
    Glyph { digit, cells, advance: rows[0].len() as f32 }
}

/// Reads counts at one UI scale.
pub struct CountFont {
    k: f32,
    glyphs: Vec<Glyph>,
}

impl CountFont {
    pub fn new(k: f32) -> Self {
        Self { k, glyphs: DIGITS.iter().map(|(digit, rows)| glyph(*digit, rows)).collect() }
    }

    pub fn scale(&self) -> f32 {
        self.k
    }

    fn crisp(&self) -> bool {
        (self.k - self.k.round()).abs() <= WHOLE_SCALE_SLACK
    }

    /// The share of `glyph`'s pixels that disagree with the frame (fill not light, outline not dark), its box's
    /// top-left at frame point (x, y). Each 1x pixel is looked at in the middle of where the game drew it, which is
    /// inside that pixel's block however the game scaled it.
    fn error(&self, frame: &RgbImage, glyph: &Glyph, x: f32, y: f32) -> f32 {
        let mut wrong = 0usize;
        for &(cx, cy, fill) in &glyph.cells {
            let (fx, fy) = ((x + (cx + 0.5) * self.k).floor(), (y + (cy + 0.5) * self.k).floor());
            if fx < 0.0 || fy < 0.0 || fx as usize >= frame.width || fy as usize >= frame.height {
                return f32::INFINITY;
            }
            let [r, g, b] = frame.pixel(fx as usize, fy as usize);
            let seen = luma(r, g, b);
            wrong += usize::from(!if fill { seen >= FILL_MIN } else { seen <= OUTLINE_MAX });
        }
        wrong as f32 / glyph.cells.len() as f32
    }

    /// The count drawn in the slot whose inside is `slot` (frame pixels), if there is one.
    pub fn read(&self, frame: &RgbImage, slot: Rect) -> Option<u32> {
        let k = self.k;
        let crisp = self.crisp();
        let max_error = if crisp { MAX_ERROR_CRISP } else { MAX_ERROR_SCALED };
        // Scaled, where the game's pixel grid falls is not known to a pixel: a few phases are tried.
        let phases: &[f32] = if crisp { &[0.0] } else { &[-0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75] };
        let top = slot.y as f32 + COUNT_TOP * k;
        let mut x = slot.x as f32;
        let mut digits = String::new();
        while digits.len() < MAX_DIGITS {
            // Of the digits that fit, the one that explains the most pixels: a 0 fits wherever an 8 is (its middle is
            // see-through), but the 8's crossbar is pixels only the 8 explains.
            let mut best: Option<(usize, f32, &Glyph, f32)> = None;
            for glyph in &self.glyphs {
                for &dy in phases {
                    for &dx in phases {
                        let error = self.error(frame, glyph, x + dx, top + dy);
                        if error > max_error {
                            continue;
                        }
                        let matched = ((1.0 - error) * glyph.cells.len() as f32).round() as usize;
                        if best.is_none_or(|(m, e, ..)| matched > m || (matched == m && error < e)) {
                            best = Some((matched, error, glyph, dx));
                        }
                    }
                }
            }
            let Some((_, _, glyph, dx)) = best else { break };
            digits.push(char::from_digit(glyph.digit, 10).unwrap_or('?'));
            x += dx + glyph.advance * k;
        }
        digits.parse().ok()
    }
}

/// Draw `count` into the slot whose inside is `slot` as the game draws it at 1x (for tests and samples).
pub fn draw_count(frame: &mut RgbImage, slot: Rect, count: u32) {
    let mut x = slot.x.max(0) as usize;
    let y = (slot.y as f32 + COUNT_TOP).max(0.0) as usize;
    for digit in count.to_string().bytes().map(|b| u32::from(b - b'0')) {
        let rows = &DIGITS[digit as usize].1;
        for (dy, row) in rows.iter().enumerate() {
            for (dx, c) in row.bytes().enumerate() {
                let v = match c {
                    b'#' => FILL[dy],
                    b'o' => OUTLINE,
                    _ => continue,
                };
                if x + dx < frame.width && y + dy < frame.height {
                    frame.set_pixel(x + dx, y + dy, [v, v, v]);
                }
            }
        }
        x += rows[0].len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_digit_is_a_box_of_eleven_rows_with_its_outline() {
        for (digit, rows) in DIGITS {
            let width = rows[0].len();
            assert!(rows.iter().all(|r| r.len() == width), "{digit}");
            // Every fill pixel is ringed by outline or more fill, so it never touches what is behind.
            for (y, row) in rows.iter().enumerate() {
                for (x, c) in row.bytes().enumerate() {
                    if c != b'#' {
                        continue;
                    }
                    for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                        let near = rows.get(ny).and_then(|r| r.as_bytes().get(nx)).copied().unwrap_or(b' ');
                        assert!(near != b' ', "digit {digit} at {x},{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_count_drawn_at_1x_reads_back() {
        let mut frame = RgbImage::filled(60, 40, [90, 120, 60]);
        let slot = Rect::new(4, 2, 32, 30);
        draw_count(&mut frame, slot, 180);
        assert_eq!(CountFont::new(1.0).read(&frame, slot), Some(180));
        assert_eq!(CountFont::new(1.0).read(&RgbImage::filled(60, 40, [90, 120, 60]), slot), None);
    }
}
