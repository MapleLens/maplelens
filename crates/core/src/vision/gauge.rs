//! The EXP numbers on the status bar ("EXP. 12,128[59.99%]"), read exactly (pure).
//!
//! The gauge numbers are a 5x7 bitmap font with 1 px strokes, drawn at the UI's size: crisp at Ideal Ratio (whole
//! multiples of 1x), smoothed in between ("Normal" draws them 1.5625x). Blur fills the one-pixel gaps between
//! glyphs, so the text is not cut at blank columns: every glyph is scaled the way the game scales it, and the
//! cheapest left-to-right run of glyphs is found under the shape the game prints, a number with thousands
//! separators and then the percentage in brackets with two decimals. That shape is what tells '[' from '1' when
//! both are a smudge.

use super::hud::Bar;
use crate::image::{GrayImage, RgbImage};

/// The status bar is this wide where the gauge font is drawn 1:1 (Ideal Ratio below 1536 px tall).
pub const GLYPH_BAR_WIDTH: f32 = 800.0;
/// The EXP number and percentage, after the "EXP." label, relative to the bar's outline at 1x (x0, y0, x1, y1).
/// The CASH SHOP button starts at 566.
const EXP_TEXT: (f32, f32, f32, f32) = (465.0, 36.0, 564.0, 50.0);
/// Gauge text is near-white (the brackets light green) on a dark grey panel: luma at or below the floor is panel,
/// the floor plus the span and above is full ink.
const INK_FLOOR: f32 = 70.0;
const INK_SPAN: f32 = 100.0;
/// A small price per glyph, so two half-matching glyphs never beat one whole one.
const GLYPH_COST: f32 = 0.3;
/// Mismatch per pixel of the read area above which the read is no read (a window or a menu over the gauge).
/// Measured: 0.06 crisp, up to 0.145 smoothed at 1.5625x.
const MAX_COST_PER_PX: f32 = 0.25;
/// Within this of a whole number the game draws the font with whole pixels.
const WHOLE_SCALE_SLACK: f32 = 0.02;

/// The gauge font, 7 rows each, '#' = ink.
const GLYPHS: [(char, [&str; 7]); 15] = [
    ('0', [" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('1', ["  #  ", " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('2', [" ### ", "#   #", "    #", "   # ", "  #  ", " #   ", "#####"]),
    ('3', [" ### ", "#   #", "    #", "  ## ", "    #", "#   #", " ### "]),
    ('4', ["   # ", "  ## ", " # # ", "#  # ", "#####", "   # ", "   # "]),
    ('5', ["#####", "#    ", "#    ", " ### ", "    #", "#   #", " ### "]),
    ('6', [" ### ", "#   #", "#    ", "#### ", "#   #", "#   #", " ### "]),
    ('7', ["#####", "    #", "    #", "    #", "   # ", "  #  ", "  #  "]),
    ('8', [" ### ", "#   #", "#   #", " ### ", "#   #", "#   #", " ### "]),
    ('9', [" ### ", "#   #", "#   #", " ####", "    #", "#   #", " ### "]),
    (',', ["  ", "  ", "  ", "  ", "  ", " #", "# "]),
    ('.', [" ", " ", " ", " ", " ", " ", "#"]),
    ('[', ["##", "# ", "# ", "# ", "# ", "# ", "##"]),
    (']', ["##", " #", " #", " #", " #", " #", "##"]),
    ('%', [" #    #", "# #  # ", " #  #  ", "   #   ", "  #  # ", " #  # #", "#    # "]),
];

/// What the bar says: EXP into this level and the share of the level that is, in hundredths of a percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExpReading {
    pub exp: u64,
    pub hundredths: u32,
}

impl ExpReading {
    /// The level's EXP, worked out from the two numbers; None at 0% (no information).
    pub fn needed(&self) -> Option<u64> {
        (self.hundredths > 0).then(|| (self.exp as f64 * 10_000.0 / self.hundredths as f64).round() as u64)
    }

    pub fn percent(&self) -> f64 {
        self.hundredths as f64 / 100.0
    }
}

/// A glyph drawn at the frame's size: ink 0..1, row-major.
struct Scaled {
    ch: char,
    width: usize,
    height: usize,
    ink: Vec<f32>,
}

fn ink(luma: u8) -> f32 {
    ((luma as f32 - INK_FLOOR) / INK_SPAN).clamp(0.0, 1.0)
}

/// `rows` at `scale`: whole pixels at a whole scale, bilinear (as the game smooths) in between; trimmed to its ink.
fn scale_glyph(ch: char, rows: &[&str; 7], scale: f32) -> Scaled {
    let (w, h) = (rows[0].len() + 2, rows.len() + 2); // a blank pixel around it, which smoothing spills into
    let mut image = GrayImage::new(w, h);
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.bytes().enumerate() {
            if c == b'#' {
                image.data[(y + 1) * w + x + 1] = 255;
            }
        }
    }
    let whole = scale.round();
    let image = if (scale - whole).abs() <= WHOLE_SCALE_SLACK && whole >= 1.0 {
        let k = whole as usize;
        let mut big = GrayImage::new(w * k, h * k);
        for y in 0..h * k {
            for x in 0..w * k {
                big.data[y * w * k + x] = image.get(x / k, y / k);
            }
        }
        big
    } else {
        image.resized_to(((w as f32 * scale).round() as usize).max(1), ((h as f32 * scale).round() as usize).max(1))
    };
    let columns: Vec<usize> =
        (0..image.width).filter(|&x| (0..image.height).any(|y| image.get(x, y) as f32 / 255.0 > 0.15)).collect();
    let (first, last) = (columns.first().copied().unwrap_or(0), columns.last().copied().unwrap_or(0));
    let width = last + 1 - first;
    let ink = (0..image.height)
        .flat_map(|y| (first..=last).map(move |x| (x, y)))
        .map(|(x, y)| image.get(x, y) as f32 / 255.0)
        .collect();
    Scaled { ch, width, height: image.height, ink }
}

/// The printed shape as a small automaton: 0 start, a number with separators (1-2), "[", one to three digits of
/// percent (3-6), ".", two decimals (7-8), "%", "]" (10, accept).
fn next_state(state: usize, ch: char) -> Option<usize> {
    let digit = ch.is_ascii_digit();
    match (state, ch) {
        (0..=2, _) if digit => Some(1),
        (1, ',') => Some(2),
        (1, '[') => Some(3),
        (3..=5, _) if digit => Some(state + 1),
        (4..=6, '.') => Some(7),
        (7, _) if digit => Some(8),
        (8, _) if digit => Some(9),
        (9, '%') => Some(10),
        (10, ']') => Some(ACCEPT),
        _ => None,
    }
}
const STATES: usize = 12;
const ACCEPT: usize = 11;

/// How the decoder reached a column and state: from (column, state), with the glyph placed (None: a blank column).
type Step = (usize, usize, Option<usize>);

/// The cheapest text of the printed shape in `line` (ink, `width` x `height`), and its cost.
fn decode(line: &[f32], width: usize, height: usize, glyphs: &[Scaled]) -> Option<(String, f32)> {
    let column_ink: Vec<f32> = (0..width).map(|x| (0..height).map(|y| line[y * width + x]).sum()).collect();
    // cost[g][x]: glyph g with its left edge at x, at its best height; ink beside it in those columns counts too.
    let costs: Vec<Vec<f32>> = glyphs
        .iter()
        .map(|g| {
            (0..width)
                .map(|x| {
                    if x + g.width > width || g.height > height {
                        return f32::INFINITY;
                    }
                    let total: f32 = column_ink[x..x + g.width].iter().sum();
                    (0..=height - g.height)
                        .map(|top| {
                            let mut sad = 0.0;
                            let mut inside = 0.0;
                            for gy in 0..g.height {
                                let row = &line[(top + gy) * width + x..][..g.width];
                                for (v, t) in row.iter().zip(&g.ink[gy * g.width..][..g.width]) {
                                    sad += (v - t).abs();
                                    inside += v;
                                }
                            }
                            sad + total - inside
                        })
                        .fold(f32::INFINITY, f32::min)
                })
                .collect()
        })
        .collect();
    let mut best = vec![[f32::INFINITY; STATES]; width + 1];
    let mut back: Vec<[Option<Step>; STATES]> = vec![[None; STATES]; width + 1];
    best[0][0] = 0.0;
    for x in 0..=width {
        for state in 0..STATES {
            let here = best[x][state];
            if !here.is_finite() {
                continue;
            }
            if x < width && here + column_ink[x] < best[x + 1][state] {
                best[x + 1][state] = here + column_ink[x];
                back[x + 1][state] = Some((x, state, None));
            }
            for (g, glyph) in glyphs.iter().enumerate() {
                let (Some(next), Some(&cost)) = (next_state(state, glyph.ch), costs[g].get(x)) else { continue };
                let total = here + cost + GLYPH_COST;
                if cost.is_finite() && total < best[x + glyph.width][next] {
                    best[x + glyph.width][next] = total;
                    back[x + glyph.width][next] = Some((x, state, Some(g)));
                }
            }
        }
    }
    let cost = best[width][ACCEPT];
    if !cost.is_finite() {
        return None;
    }
    let (mut text, mut at) = (Vec::new(), (width, ACCEPT));
    while let Some((x, state, glyph)) = back[at.0][at.1] {
        text.extend(glyph.map(|g| glyphs[g].ch));
        at = (x, state);
    }
    text.reverse();
    Some((text.into_iter().collect(), cost))
}

/// "12,128[59.99%]" -> the reading, if the separators group thousands properly and the share is at most 100%.
fn parse(text: &str) -> Option<ExpReading> {
    let (number, rest) = text.split_once('[')?;
    let groups: Vec<&str> = number.split(',').collect();
    let well_grouped = (1..=3).contains(&groups[0].len()) && groups[1..].iter().all(|g| g.len() == 3);
    let exp = number.replace(',', "").parse().ok().filter(|_| well_grouped)?;
    let (whole, fraction) = rest.strip_suffix("%]")?.split_once('.')?;
    let hundredths = whole.parse::<u32>().ok()? * 100 + fraction.parse::<u32>().ok()?;
    (hundredths <= 10_000).then_some(ExpReading { exp, hundredths })
}

/// Read the EXP on the status bar `bar` found in `frame` (the same image's coordinates).
pub fn read_exp(frame: &RgbImage, bar: Bar) -> Option<ExpReading> {
    read_exp_text(frame, bar).and_then(|(text, _)| parse(&text))
}

/// The raw text and its mismatch per pixel, for tests and tuning.
pub fn read_exp_text(frame: &RgbImage, bar: Bar) -> Option<(String, f32)> {
    let scale = bar.width as f32 / GLYPH_BAR_WIDTH;
    let at = |offset: f32, origin: usize| origin + (offset * scale).round() as usize;
    let (x0, y0, x1, y1) = EXP_TEXT;
    let (left, top, right, bottom) = (at(x0, bar.x), at(y0, bar.top), at(x1, bar.x), at(y1, bar.top));
    if right > frame.width || bottom > frame.height || right <= left || bottom <= top {
        return None;
    }
    let gray = frame.crop(left, top, right, bottom).to_gray();
    let line: Vec<f32> = gray.data.iter().map(|&v| ink(v)).collect();
    let glyphs: Vec<Scaled> = GLYPHS.iter().map(|(ch, rows)| scale_glyph(*ch, rows, scale)).collect();
    let (text, cost) = decode(&line, gray.width, gray.height, &glyphs)?;
    let per_px = cost / line.len() as f32;
    (per_px <= MAX_COST_PER_PX).then_some((text, per_px))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_printed_shape() {
        assert_eq!(parse("12,128[59.99%]"), Some(ExpReading { exp: 12_128, hundredths: 5999 }));
        assert_eq!(parse("0[0.00%]"), Some(ExpReading { exp: 0, hundredths: 0 }));
        assert_eq!(parse("1,234,567[100.00%]"), Some(ExpReading { exp: 1_234_567, hundredths: 10_000 }));
        assert_eq!(parse("12,12[59.99%]"), None);
        assert_eq!(parse("1234,128[59.99%]"), None);
        assert_eq!(parse("5[100.01%]"), None);
    }

    #[test]
    fn the_level_is_worked_out_from_both_numbers() {
        let reading = ExpReading { exp: 12_128, hundredths: 5999 };
        assert_eq!(reading.needed(), Some(20_217));
        assert_eq!(ExpReading { exp: 0, hundredths: 0 }.needed(), None);
    }

    #[test]
    fn whole_scales_keep_whole_pixels() {
        let one = scale_glyph('1', &GLYPHS[1].1, 2.0);
        assert_eq!((one.width, one.height), (4, 18));
        assert!(one.ink.iter().all(|&v| v == 0.0 || v == 1.0));
    }
}
