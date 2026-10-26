//! World-map tooltips ("Victoria Road : Henesys"): text mask and tooltip box from OCR lines.
//!
//! Unlike item tooltips this widget is not at a measurable offset from the cursor (two samples disagree by ~20 px),
//! so its text lines are read by OCR inside a window next to the cursor and the box is built around them.

use super::Mask;
use crate::geometry::Rect;
use crate::image::{GrayImage, RgbImage};
use crate::mapinfo::Location;
use crate::mapinfo::resolve::parse_location;

/// Wider than a glyph stroke, narrower than the white clouds of the map behind the tooltip.
const TOPHAT_KERNEL: usize = 11;
/// Text is much brighter than its surroundings...
const TOPHAT_MIN: i32 = 50;
/// ...and bright in itself.
const GRAY_MIN: u8 = 150;
/// The title is among the top rows of text; the scene behind may add stray ones above.
const TITLE_ROWS: usize = 4;
/// Lines whose centres are closer than this many line heights are one row of text.
const SAME_ROW: f32 = 0.6;
/// A description line sits within this many line heights below the previous one.
const MAX_LINE_GAP: f32 = 1.2;
/// Tooltip border to text, measured on the two samples.
const PAD_X: i32 = 10;
/// One row: just the title.
const PAD_SINGLE: (i32, i32) = (8, 8);
/// With a description the title sits lower in the box.
const PAD_DESCRIBED: (i32, i32) = (16, 10);
/// A tooltip fills a few % of the scan window; over a bright map its fill barely tints.
pub const TINT_MIN_FRACTION: f32 = 0.01;
/// A tooltip's text spans at least this many pixel rows.
pub const TEXT_ROWS_MIN: usize = 8;

/// One recognised line of text and where it is, in the pixels of the image that was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLine {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl TextLine {
    pub fn new(text: &str, x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { text: text.into(), x, y, w, h }
    }

    fn centre(&self) -> f32 {
        self.y as f32 + self.h as f32 / 2.0
    }
}

/// An OCR engine that also says where each line is (Windows.Media.Ocr does).
pub trait LineOcr {
    fn read_lines(&mut self, mask: &Mask) -> Vec<TextLine>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapTooltip {
    pub location: Location,
    /// The tooltip, in the pixels of the image the lines were read from.
    pub rect: Rect,
}

/// Grey-level erosion or dilation with a `TOPHAT_KERNEL` square, separably; pixels off the image are ignored.
fn morph(gray: &GrayImage, dilate: bool) -> GrayImage {
    let pick = |a: u8, b: u8| if dilate { a.max(b) } else { a.min(b) };
    let half = TOPHAT_KERNEL / 2;
    let pass = |image: &GrayImage, horizontal: bool| {
        let mut out = image.clone();
        for y in 0..image.height {
            for x in 0..image.width {
                let (along, length) = if horizontal { (x, image.width) } else { (y, image.height) };
                let range = along.saturating_sub(half)..(along + half + 1).min(length);
                let mut value = image.get(x, y);
                for k in range {
                    value = pick(value, if horizontal { image.get(k, y) } else { image.get(x, k) });
                }
                out.data[y * image.width + x] = value;
            }
        }
        out
    };
    pass(&pass(gray, true), false)
}

/// Pixels of near-white text, whatever the (translucent) tooltip shows behind it: a top-hat (the image minus its
/// opening) keeps what is brighter than its surroundings.
pub fn text_mask(image: &RgbImage) -> Mask {
    let gray = image.to_gray();
    let opened = morph(&morph(&gray, false), true);
    let bits = gray.data.iter().zip(&opened.data).map(|(&g, &o)| g as i32 - o as i32 > TOPHAT_MIN && g > GRAY_MIN);
    Mask { width: gray.width, height: gray.height, bits: bits.collect() }
}

/// Share of pixels in the tooltip's translucent blue: far bluer than red or green, never bright.
pub fn tint_fraction(image: &RgbImage) -> f32 {
    let tinted = image
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| {
            let (r, g, b) = (p[0] as i16, p[1] as i16, p[2] as i16);
            (80..=200).contains(&b) && b - g >= 12 && b - r >= 12
        })
        .count();
    tinted as f32 / (image.width * image.height).max(1) as f32
}

/// Pixel rows that hold text-like pixels.
pub fn text_rows(mask: &Mask) -> usize {
    (0..mask.height).filter(|&y| (0..mask.width).filter(|&x| mask.get(x, y)).count() >= 3).count()
}

/// Cheap pre-check so OCR only runs where a blue box with text could be (the game scene usually is not).
pub fn may_hold_tooltip(image: &RgbImage, mask: &Mask) -> bool {
    tint_fraction(image) >= TINT_MIN_FRACTION && text_rows(mask) >= TEXT_ROWS_MIN
}

/// Lines grouped into rows, top to bottom (OCR may split one printed row into several lines).
fn rows(lines: &[TextLine]) -> Vec<Vec<TextLine>> {
    let mut sorted: Vec<TextLine> = lines.iter().filter(|l| !l.text.is_empty()).cloned().collect();
    sorted.sort_by_key(|l| l.y);
    let mut rows: Vec<Vec<TextLine>> = Vec::new();
    for line in sorted {
        match rows.last_mut() {
            Some(row) if (line.centre() - centre(row)).abs() < SAME_ROW * line.h as f32 => row.push(line),
            _ => rows.push(vec![line]),
        }
    }
    rows
}

fn centre(row: &[TextLine]) -> f32 {
    row.iter().map(TextLine::centre).sum::<f32>() / row.len() as f32
}

/// The tooltip whose title is the top-most row of text that reads as a location, or None.
///
/// A row with the "Street : Name" colon beats one without: the colon is what separates a title from the stray text
/// OCR finds in the scene behind the tooltip.
pub fn find_tooltip(lines: &[TextLine]) -> Option<MapTooltip> {
    let rows: Vec<Vec<TextLine>> = rows(lines).into_iter().take(TITLE_ROWS).collect();
    let titles: Vec<(usize, Location)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let mut ordered: Vec<&TextLine> = row.iter().collect();
            ordered.sort_by_key(|l| l.x);
            let text = ordered.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
            parse_location(&text).map(|location| (i, location))
        })
        .collect();
    let (i, location) = titles.iter().find(|(_, l)| !l.street.is_empty()).or(titles.first())?.clone();
    Some(tooltip(&rows, i, location))
}

fn tooltip(rows: &[Vec<TextLine>], i: usize, location: Location) -> MapTooltip {
    let row = &rows[i];
    let mut block: Vec<&TextLine> = row.iter().collect();
    let mut bottom = row.iter().map(|l| l.y + l.h).max().unwrap_or(0);
    for below in &rows[i + 1..] {
        // The description, while the rows stay close together.
        let top = below.iter().map(|l| l.y).min().unwrap_or(0);
        let height = below.iter().map(|l| l.h).max().unwrap_or(0);
        if (top - bottom) as f32 > MAX_LINE_GAP * height as f32 {
            break;
        }
        block.extend(below);
        bottom = below.iter().map(|l| l.y + l.h).max().unwrap_or(bottom);
    }
    let left = block.iter().map(|l| l.x).min().unwrap_or(0);
    let top = block.iter().map(|l| l.y).min().unwrap_or(0);
    let right = block.iter().map(|l| l.x + l.w).max().unwrap_or(0);
    let (above, under) = if block.len() == row.len() { PAD_SINGLE } else { PAD_DESCRIBED };
    let rect = Rect::new(left - PAD_X, top - above, right - left + 2 * PAD_X, bottom - top + above + under);
    MapTooltip { location, rect }
}
