//! Text of tooltip titles and name plates: glyph atlas first, an OCR engine only for what the atlas lacks.

use std::collections::{HashMap, VecDeque};

use super::glyphs::Atlas;
use super::{Mask, TEXT_THRESHOLD};
use crate::image::{GrayImage, RgbImage};

/// Title row incl. descenders (q, y, g); the description line starts below this.
pub const TITLE_ROW_H: usize = 46;
/// A long name wraps onto a second title line this far below the first.
pub const TITLE_LINE_H: usize = 24;
/// Skips the blue bullet in front of the title, when the bullet itself is not found.
pub const TITLE_LEFT_SKIP: usize = 18;
/// Where a title's bullet can be (columns, rows from the title row's top): usually near (14, 23), a few pixels off
/// when the tooltip is.
const BULLET_SEARCH: (usize, usize) = (60, TITLE_ROW_H);
/// Where the bullet's top-left usually is.
const BULLET_USUAL: (i32, i32) = (14, 23);
/// The bullet is a round light-blue dot: measured on 790 captures, 7-8 px square with 35-45 pixels passing the colour
/// test (fewer where the pointer covers part of it).
const BULLET_SIZE: std::ops::RangeInclusive<usize> = 5..=9;
const BULLET_PIXELS: std::ops::RangeInclusive<usize> = 20..=50;
/// How far around the dot its dark surroundings are checked, how dark they are and how much of them.
const BULLET_RING: usize = 2;
const RING_DARK: u8 = 150;
const RING_DARK_SHARE: f32 = 0.6;
/// Columns between the bullet and the first letter that are never ink.
const BULLET_GAP: usize = 1;
/// The title's capitals reach this far above the bullet's top row, its descenders this far below its bottom row
/// (with a row or two to spare).
const BULLET_ROWS_ABOVE: usize = 9;
const BULLET_ROWS_BELOW: usize = 12;
const STRAY_LEADING: &[char] = &['\'', '`', '.', ',', '·', '•', ' '];
const CACHE_SIZE: usize = 64;
/// Appended to a mask's cache key for its forced OCR read, which may differ from the atlas read of the same mask.
const OCR_KEY_MARK: u8 = 0xFF;
/// Letters an atlas read needs before it alone counts as proof that a tooltip title was read.
const ATLAS_PROOF_LETTERS: usize = 6;
/// Masks kept so a read the catalog later confirms can teach the atlas.
const RECENT_SIZE: usize = 32;

/// Glyph mask of the title row(s) of the tooltip at the image's top-left, or None when nothing is captured.
pub fn title_mask(image: &RgbImage, tooltip_width: usize, lines: usize) -> Option<Mask> {
    title_mask_at(image, tooltip_width, lines, 0)
}

/// `title_mask` starting `top` rows down (a player-crafted item has the crafter's name on its own line first).
pub fn title_mask_at(image: &RgbImage, tooltip_width: usize, lines: usize, top: usize) -> Option<Mask> {
    title_mask_anchored(image, tooltip_width, lines, top).map(|(mask, _)| mask)
}

/// The title mask and whether it was cut around the title's bullet.
///
/// The title starts right after its blue bullet and shares its rows: anything left of it or above or below it
/// (bright scenery beside a tooltip a few pixels off its usual spot, text of a window behind) would otherwise be read
/// as junk letters around the name. Without a bullet the usual spot is used.
pub fn title_mask_anchored(image: &RgbImage, width: usize, lines: usize, top: usize) -> Option<(Mask, bool)> {
    let extra = lines.saturating_sub(1) * TITLE_LINE_H;
    let (left, y0, y1, anchored) = match find_bullet(image, top) {
        Some(b) => {
            let y0 = (top + b.top).saturating_sub(BULLET_ROWS_ABOVE);
            (b.end, y0, top + b.bottom + BULLET_ROWS_BELOW + extra, true)
        }
        None => (TITLE_LEFT_SKIP, top, top + TITLE_ROW_H + extra, false),
    };
    let right = title_right(image, left, (y0, y1), width);
    let row = image.crop(left, y0, right.max(left), y1);
    (!row.is_empty()).then(|| (Mask::threshold(&title_gray(&row), TEXT_THRESHOLD), anchored))
}

/// Blank columns after which a title has ended: wider than the gap between two words.
const TITLE_END_GAP: usize = 10;
/// A column inked over this share of the title's rows is no letter: the tooltip's border, or a light window behind.
const WALL_SHARE: f32 = 0.8;
/// How far before the cut a border is looked for: a 370 px tooltip's border reads at columns 366 and 369.
const BORDER_LOOKBACK: usize = 8;

/// Where the title row is cut: at `width`, or further when the title runs on past it. The game widens a tooltip for
/// a long name ("Gloves Magic Attack Scroll: Intermediate" is ~465 px), and the cut had read it as "...Scroll: Inter".
/// The cut stays for a tooltip whose border is at it: only beside that narrow equipment tooltip can a compare tooltip
/// be, whose title must not be read.
fn title_right(image: &RgbImage, left: usize, (y0, y1): (usize, usize), width: usize) -> usize {
    if width <= left + BORDER_LOOKBACK || width >= image.width {
        return width;
    }
    let row = title_gray(&image.crop(left, y0, image.width, y1));
    let ink = |c: usize| (0..row.height).filter(|&y| row.data[y * row.width + c] >= TEXT_THRESHOLD).count();
    let wall = |c: usize| ink(c) as f32 >= WALL_SHARE * row.height as f32;
    let cut = width - left;
    if (cut - BORDER_LOOKBACK..cut).any(wall) {
        return width;
    }
    let Some(mut last) = (cut.saturating_sub(TITLE_END_GAP)..cut).rev().find(|&c| ink(c) > 0) else { return width };
    for c in cut..row.width {
        if wall(c) || c - last > TITLE_END_GAP {
            break;
        }
        if ink(c) > 0 {
            last = c;
        }
    }
    (left + last + 2).max(width)
}

/// Warm text is this much redder than blue: the fill and the scenery under it never are.
const WARM_LEAD: i16 = 50;

/// Grey levels of a title row. Enhanced equipment has an orange title (230,164,56), whose luminance (~172) falls
/// under `TEXT_THRESHOLD` and read as nothing at all; for warm pixels the brighter of red and green is used.
fn title_gray(row: &RgbImage) -> GrayImage {
    let mut gray = row.to_gray();
    for (g, p) in gray.data.iter_mut().zip(row.data.as_chunks::<3>().0) {
        if p[0] as i16 - p[2] as i16 >= WARM_LEAD {
            *g = (*g).max(p[0].max(p[1]));
        }
    }
    gray
}

/// A title's bullet: the column right after it and its rows (from the title row's top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bullet {
    pub end: usize,
    pub top: usize,
    pub bottom: usize,
}

/// Where the title row's bullet ends: the first column right of the light-blue dot, if a dot of a bullet's size
/// is near where bullets go.
pub fn bullet_end(image: &RgbImage, top: usize) -> Option<usize> {
    find_bullet(image, top).map(|b| b.end)
}

/// The title's bullet, if a light-blue dot of a bullet's size is near where bullets go. Other blue things nearby
/// (item icons, window frames) are separate blobs.
pub fn find_bullet(image: &RgbImage, top: usize) -> Option<Bullet> {
    let (w, h) = (BULLET_SEARCH.0.min(image.width), BULLET_SEARCH.1.min(image.height.saturating_sub(top)));
    let blue = |x: usize, y: usize| {
        let [r, g, b] = image.pixel(x, top + y);
        b > 200 && g > 150 && r < 150
    };
    let mut seen = vec![false; w * h];
    let mut best: Option<(usize, Bullet)> = None; // (distance from the usual spot, bullet)
    for start in 0..w * h {
        if seen[start] || !blue(start % w, start / w) {
            continue;
        }
        // One 4-connected blob.
        let (mut stack, mut count) = (vec![start], 0);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            count += 1;
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            let neighbours = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
            for (nx, ny) in neighbours {
                if nx < w && ny < h && !seen[ny * w + nx] && blue(nx, ny) {
                    seen[ny * w + nx] = true;
                    stack.push(ny * w + nx);
                }
            }
        }
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let dot = BULLET_SIZE.contains(&w)
            && BULLET_SIZE.contains(&h)
            && w.abs_diff(h) <= 2
            && BULLET_PIXELS.contains(&count)
            && on_dark_fill(image, (x0, top + y0), (x1, top + y1));
        if dot {
            let distance = (x0 as i32 - BULLET_USUAL.0).unsigned_abs() as usize
                + (y0 as i32 - BULLET_USUAL.1).unsigned_abs() as usize;
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, Bullet { end: x1 + 1 + BULLET_GAP, top: y0, bottom: y1 }));
            }
        }
    }
    best.map(|(_, bullet)| bullet)
}

/// A bullet sits on the tooltip's dark fill: the ring of pixels just outside the dot is dark. A light-blue spot of
/// an item icon in a shop list sits on the list's light background.
fn on_dark_fill(image: &RgbImage, (x0, y0): (usize, usize), (x1, y1): (usize, usize)) -> bool {
    let (left, top) = (x0.saturating_sub(BULLET_RING), y0.saturating_sub(BULLET_RING));
    let (right, bottom) = ((x1 + BULLET_RING).min(image.width - 1), (y1 + BULLET_RING).min(image.height - 1));
    let ring: Vec<u8> = (top..=bottom)
        .flat_map(|y| (left..=right).map(move |x| (x, y)))
        .filter(|&(x, y)| x < x0 || x > x1 || y < y0 || y > y1)
        .map(|(x, y)| {
            let [r, g, b] = image.pixel(x, y);
            crate::image::luma(r, g, b)
        })
        .collect();
    let dark = ring.iter().filter(|&&v| v < RING_DARK).count();
    !ring.is_empty() && dark as f32 >= RING_DARK_SHARE * ring.len() as f32
}

/// The atlas of tooltip title letters bundled with MapleLens (built from `vision::samples`).
pub fn title_atlas() -> Atlas {
    Atlas::from_json(include_str!("../../assets/glyphs/title.json")).unwrap_or_else(|_| Atlas::empty(18, 6, 7))
}

/// The atlas of Character Info name plate letters bundled with MapleLens.
pub fn name_atlas() -> Atlas {
    Atlas::from_json(include_str!("../../assets/glyphs/name.json")).unwrap_or_else(|_| Atlas::empty(14, 4, 99))
}

/// OCR engines read larger glyphs far more accurately.
const OCR_UPSCALE: f32 = 3.0;
/// White margin: glyphs touching the image edge are often dropped.
const OCR_PADDING: usize = 20;

/// A mask as OCR engines read it best: dark text on white, upscaled, with a margin. Thin fonts (the name
/// plate) get thicker strokes first, as engines drop 1 px lines.
pub fn ocr_image(mask: &Mask, thicken: bool) -> GrayImage {
    let mask = if thicken { mask.dilated() } else { mask.clone() };
    let scaled = mask.to_ink_on_paper().resized(OCR_UPSCALE);
    let (w, h) = (scaled.width + 2 * OCR_PADDING, scaled.height + 2 * OCR_PADDING);
    let mut out = GrayImage { width: w, height: h, data: vec![255; w * h] };
    for y in 0..scaled.height {
        let row = &scaled.data[y * scaled.width..(y + 1) * scaled.width];
        out.data[(y + OCR_PADDING) * w + OCR_PADDING..][..scaled.width].copy_from_slice(row);
    }
    out
}

/// What an OCR engine made of a mask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OcrAnswer {
    Text(String),
    /// Asked; the answer arrives later (the browser's engine is asynchronous). Ask again next frame.
    Pending,
    /// No engine on this platform.
    Unavailable,
}

/// A platform OCR engine (Windows.Media.Ocr natively, Tesseract in the browser).
pub trait Ocr {
    fn recognize(&mut self, mask: &Mask) -> OcrAnswer;
}

/// OCR of a ready grey image, dark text on white paper: for text a mask would break apart (the minimap's title).
pub trait ImageOcr {
    fn recognize_image(&mut self, image: &GrayImage) -> OcrAnswer;
}

pub struct NoOcr;

impl ImageOcr for NoOcr {
    fn recognize_image(&mut self, _image: &GrayImage) -> OcrAnswer {
        OcrAnswer::Unavailable
    }
}

impl Ocr for NoOcr {
    fn recognize(&mut self, _mask: &Mask) -> OcrAnswer {
        OcrAnswer::Unavailable
    }
}

/// Reads text masks with a glyph atlas, asking the OCR engine only when a glyph is unknown; caches by mask.
pub struct TextReader {
    atlas: Atlas,
    ocr: Box<dyn Ocr + Send>,
    /// Reads by mask, and whether the atlas alone read them (every glyph matched a letter of the title font).
    cache: HashMap<Vec<u8>, (String, bool)>,
    /// Recent reads with unknown glyphs, and whether their mask is a bullet-anchored title (only those teach: other
    /// UI under the pointer has other fonts).
    recent: VecDeque<(String, Mask, bool)>,
    learned: bool,
    /// Whether unknown glyphs may be sent to OCR (`allow_ocr`).
    ocr_allowed: bool,
}

impl TextReader {
    pub fn new(atlas: Atlas, ocr: Box<dyn Ocr + Send>) -> Self {
        Self { atlas, ocr, cache: HashMap::new(), recent: VecDeque::new(), learned: false, ocr_allowed: true }
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// Swap in another atlas (one merged with letters learnt earlier); cached reads are dropped.
    pub fn replace_atlas(&mut self, atlas: Atlas) {
        self.atlas = atlas;
        self.cache.clear();
    }

    /// Let unknown glyphs go to OCR, or read with the atlas alone (`?` for what it does not know, nothing cached or
    /// learnt). OCR is the expensive part of reading, and only worth it where its answer could count.
    pub fn allow_ocr(&mut self, allowed: bool) {
        self.ocr_allowed = allowed;
    }

    /// True once since the atlas last learnt something (the caller saves it then).
    /// Forget what was read so far: reads made while OCR was unavailable ought to be tried again once it is not.
    pub fn forget_reads(&mut self) {
        self.cache.clear();
    }

    pub fn take_learned(&mut self) -> bool {
        std::mem::take(&mut self.learned)
    }

    /// Item name printed in the title row(s) of the tooltip at the image's top-left ("" if unreadable).
    pub fn read_title(&mut self, image: &RgbImage, tooltip_width: usize, lines: usize) -> String {
        self.read_title_at(image, tooltip_width, lines, 0)
    }

    /// `read_title` starting `top` rows down (below a crafted item's crafter-name line).
    pub fn read_title_at(&mut self, image: &RgbImage, tooltip_width: usize, lines: usize, top: usize) -> String {
        match title_mask_anchored(image, tooltip_width, lines, top) {
            Some((mask, anchored)) => self.read(&mask, anchored),
            None => String::new(), // capture clipped at the screen edge (or a mid-resize frame)
        }
    }

    /// OCR's reading of the title, even when the atlas read every glyph: a clean read the catalog does not know may
    /// hold a glyph the atlas took for the wrong letters ('h' cut into 'la'). None without an answer.
    pub fn ocr_title(&mut self, image: &RgbImage, tooltip_width: usize) -> Option<String> {
        let (mask, _) = title_mask_anchored(image, tooltip_width, 1, 0)?;
        self.ocr_mask(&mask)
    }

    /// OCR's reading of a mask, the atlas bypassed and nothing learnt from it: for text in a font the atlas does not
    /// hold, where an atlas match is a look-alike from another font. None without an answer (yet).
    pub fn ocr_mask(&mut self, mask: &Mask) -> Option<String> {
        let mut key = mask.key();
        key.push(OCR_KEY_MARK);
        if let Some((text, _)) = self.cache.get(&key) {
            return Some(text.clone());
        }
        if !self.ocr_allowed {
            return None;
        }
        let OcrAnswer::Text(text) = self.ocr.recognize(mask) else { return None };
        let text = text.trim().trim_start_matches(STRAY_LEADING).to_string();
        if self.cache.len() >= CACHE_SIZE {
            self.cache.clear();
        }
        self.cache.insert(key, (text.clone(), false));
        Some(text)
    }

    /// The atlas's own read of the title with unknown glyphs as `?`, when it has any (else `read_title` already is
    /// the atlas's read). OCR resolves unknown glyphs by reading the whole title again, and on a busy backdrop it
    /// can lose letters the atlas had ('SiI?er Ore' -> 'Ore').
    pub fn atlas_title(&mut self, image: &RgbImage, tooltip_width: usize) -> Option<String> {
        let (mask, _) = title_mask_anchored(image, tooltip_width, 1, 0)?;
        let glyphs = self.atlas.read(&mask);
        (glyphs.unknown > 0).then_some(glyphs.text)
    }

    /// Text of a glyph mask. Unknown glyphs read as `?` when no OCR answer is available.
    pub fn read_mask(&mut self, mask: &Mask) -> String {
        self.read(mask, true)
    }

    fn read(&mut self, mask: &Mask, learnable: bool) -> String {
        let key = mask.key();
        if let Some((text, _)) = self.cache.get(&key) {
            return text.clone();
        }
        let glyphs = self.atlas.read(mask);
        if glyphs.unknown > 0 && !self.ocr_allowed {
            return glyphs.text;
        }
        let (text, settled) = if glyphs.unknown == 0 {
            (glyphs.text, true)
        } else {
            match self.ocr.recognize(mask) {
                OcrAnswer::Text(text) => (text.trim().trim_start_matches(STRAY_LEADING).to_string(), true),
                OcrAnswer::Pending => (glyphs.text, false),
                OcrAnswer::Unavailable => (glyphs.text, true),
            }
        };
        if glyphs.unknown > 0 {
            if self.recent.len() >= RECENT_SIZE {
                self.recent.pop_front();
            }
            self.recent.push_back((text.clone(), mask.clone(), learnable));
        }
        if settled {
            if self.cache.len() >= CACHE_SIZE {
                self.cache.clear();
            }
            self.cache.insert(key, (text.clone(), glyphs.unknown == 0));
        }
        text
    }

    /// Whether `text` was a recent read the atlas made by itself: every glyph matched a letter of the title font,
    /// which only an item title (or a name plate) shows. OCR reads any text, the shop list behind included.
    pub fn read_by_atlas(&self, text: &str) -> bool {
        // A few glyph-sized blobs of other UI can pass for a short word ('Egg'); a long name cannot.
        text.chars().filter(|c| c.is_alphanumeric()).count() >= ATLAS_PROOF_LETTERS
            && self.cache.values().any(|(read, atlas)| *atlas && read == text)
    }

    /// The market catalog says `read` was really `truth`: learn the glyphs the atlas did not know.
    pub fn confirm(&mut self, read: &str, truth: &str) -> usize {
        let Some(index) = self.recent.iter().position(|(text, ..)| text == read) else { return 0 };
        let Some((_, mask, true)) = self.recent.remove(index) else { return 0 };
        let added = self.atlas.learn(&mask, truth);
        if added > 0 {
            self.learned = true;
            self.cache.clear(); // earlier `?` reads can now be read properly
        }
        added
    }
}
