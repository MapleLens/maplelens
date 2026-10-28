//! The shop's text read by its own fonts: a line's box in, its text out ('?' for each letter the font does not know).
//!
//! The game draws the shop's text two ways. At "Ideal Ratio" (UI scale 0.641) and its 1440p double it is a bitmap
//! font, every letter the same pixels every time: read exactly from the table the Character Info names use
//! (`vision::pixel_font`, the same font). At the default UI sizes it is a vector font at fractional positions: read
//! by drawing it (`vision::vector_font`). Neither needs OCR for the letters it knows; OCR only fills the '?'.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::layout::{Area, View};
use crate::image::RgbImage;
use crate::vision::gaps::fits;
use crate::vision::pixel_font::{PixelFont, line_mask, shop_small_font, unscaled_line_mask};
use crate::vision::vector_font::{Line, read, shop_font};

/// The bitmap font's UI scale ("Ideal Ratio"); it is drawn at whole multiples of it.
const BITMAP_SCALE: f32 = 0.641;
const BITMAP_SLACK: f32 = 0.01;

/// What a line of shop text reads as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    /// Letters (or runs of touching letters) the font does not know, each a '?' in `text`.
    pub unknown: usize,
}

/// Frame pixels per bitmap-font pixel when the UI is drawn in the bitmap font, else None.
pub fn bitmap_block(scale: f32) -> Option<usize> {
    let k = (scale / BITMAP_SCALE).round();
    (k >= 1.0 && (scale / (BITMAP_SCALE * k) - 1.0).abs() < BITMAP_SLACK).then_some(k as usize)
}

fn bitmap_font() -> &'static PixelFont {
    static FONT: OnceLock<PixelFont> = OnceLock::new();
    FONT.get_or_init(shop_small_font)
}

/// Readings remembered by the line's pixels: the same lines are read again whenever a list shifts or a look is
/// retried, and a line costs tens of milliseconds (more in the browser). Shared by a watcher's clones.
#[derive(Clone, Default)]
pub struct Cache(Arc<Mutex<HashMap<u64, Option<Text>>>>);

/// Lines remembered; more are dropped all at once.
const CACHED_LINES: usize = 1024;

impl Cache {
    /// The text in `area` (reference pixels); `light_text` for light letters on a dark plate (the owner's name).
    pub fn read_area(&self, view: &View<'_>, area: Area, light_text: bool) -> Option<Text> {
        let (x0, y0, x1, y1) = view.frame_area(area);
        let pixels = view.frame.crop(x0, y0, x1, y1);
        let key = pixels.data.iter().fold(0xcbf29ce484222325u64 ^ u64::from(light_text), |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        }) ^ (pixels.width as u64) << 48
            ^ view.scale.to_bits() as u64;
        let mut map = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(known) = map.get(&key) {
            return known.clone();
        }
        let text = read_pixels(&pixels, view.scale, light_text);
        if map.len() >= CACHED_LINES {
            map.clear();
        }
        map.insert(key, text.clone());
        text
    }
}

/// The text in `area` (reference pixels), uncached.
pub fn read_area(view: &View<'_>, area: Area, light_text: bool) -> Option<Text> {
    let (x0, y0, x1, y1) = view.frame_area(area);
    read_pixels(&view.frame.crop(x0, y0, x1, y1), view.scale, light_text)
}

/// A line's text by the shop's fonts, from its pixels as captured at UI `scale`.
pub fn read_pixels(pixels: &RgbImage, scale: f32, light_text: bool) -> Option<Text> {
    let (text, unknown) = match bitmap_block(scale) {
        Some(block) => bitmap_font().read_line(&line_mask(pixels, block, light_text)?),
        None => {
            // The default sizes scale the bitmap font up with the whole frame: shrunk back it is read exactly, but a
            // dot can fade away in the shrink. So it only fills the drawn templates' gaps, where it agrees with every
            // letter they are sure of.
            let reading = read(&Line::new(pixels, scale, light_text)?, shop_font())?;
            let exact = (reading.unknown > 0)
                .then(|| unscaled_line_mask(pixels, scale / BITMAP_SCALE, light_text))
                .flatten()
                .map(|mask| bitmap_font().read_line(&mask))
                .filter(|(text, unknown)| *unknown == 0 && fits(&reading.text, text));
            exact.unwrap_or((reading.text, reading.unknown))
        }
    };
    let text = settle(text.trim());
    (!text.is_empty()).then_some(Text { text, unknown })
}

/// A reading settled the way a reader would: its I/l bars (`settle_bars`), and a '.' run into the next letter read
/// back as the space it was ("left.the room.": a speck of anti-aliasing in the word gap; game names have no '.').
pub fn settle(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let spaced: String = chars
        .iter()
        .enumerate()
        .map(|(i, &c)| if c == '.' && chars.get(i + 1).is_some_and(|n| n.is_alphabetic()) { ' ' } else { c })
        .collect();
    settle_bars(&spaced)
}

/// Capital I and lowercase l are the same bar in both fonts (Arial draws them alike; the bitmap font identically),
/// so a bar is settled the way a reader would: between capitals it is an I, between lowercase letters an l, starting
/// a word an I before a consonant and an l before a vowel, two bars starting a word "Il" ("Ilovestudy": no name
/// starts "ll"), after a capital and before lowercase an l ("Blvd", "Clara": a capital I there would start a word of
/// its own), and at the end of a word after lowercase letters an l, otherwise as the font read it ("DIAksdiw",
/// "KELEBUJIABIN", "Dalliance", "Steel").
pub fn settle_bars(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let is_bar = |c: char| c == 'I' || c == 'l';
    let neighbour = |from: usize, step: isize| {
        let mut i = from as isize + step;
        while i >= 0 && (i as usize) < chars.len() && is_bar(chars[i as usize]) {
            i += step;
        }
        (i >= 0 && (i as usize) < chars.len()).then(|| chars[i as usize])
    };
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            if !is_bar(c) {
                return c;
            }
            let (before, after) = (neighbour(i, -1), neighbour(i, 1));
            let upper = |n: Option<char>| n.is_some_and(|c| c.is_ascii_uppercase());
            let lower = |n: Option<char>| n.is_some_and(|c| c.is_ascii_lowercase());
            let starts_word = i == 0 || chars[i - 1] == ' ';
            let pair_start = starts_word && chars.get(i + 1).is_some_and(|&n| is_bar(n));
            let pair_second = i > 0 && is_bar(chars[i - 1]) && (i == 1 || chars[i - 2] == ' ');
            if (pair_start || pair_second)
                && chars.get(i + 2 - usize::from(pair_second)).is_some_and(|&n| !is_bar(n))
                && lower(after)
            {
                if pair_start { 'I' } else { 'l' }
            } else if upper(before) && upper(after) || upper(before) && after.is_none_or(|c| !c.is_alphanumeric()) {
                'I'
            } else if starts_word && lower(after) {
                // Starting a word, the next letter tells: a consonant follows a capital I (It, Ir, Iz: "ItsDaniel",
                // "Iron"), a vowel a lowercase l ("lazynash", "looking4deal", "left").
                if after.is_some_and(|c| "aeiouy".contains(c)) { 'l' } else { 'I' }
            } else if upper(before) && lower(after) {
                'l'
            } else if lower(before) && (lower(after) || upper(after) || after.is_none_or(|c| !c.is_alphanumeric())) {
                // After lowercase letters a bar ends the lowercase run: before more lowercase, the end of a word
                // ("Steel", "Mithril") or a capital starting the next part of a name ("ApexKillKing").
                'l'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_are_settled_by_their_neighbours() {
        assert_eq!(settle_bars("DlAksdiw has entered."), "DIAksdiw has entered.");
        assert_eq!(settle_bars("KELEBUJlABlN"), "KELEBUJIABIN");
        assert_eq!(settle_bars("DaIIiance000"), "Dalliance000");
        assert_eq!(settle_bars("IzzysKitty"), "IzzysKitty");
        assert_eq!(settle_bars("lazynash has Ieft the room."), "lazynash has left the room.");
        assert_eq!(settle_bars("lzzysKitty has Ieft the room."), "IzzysKitty has left the room.");
        assert_eq!(settle_bars("ltsDaniel"), "ItsDaniel");
        assert_eq!(settle_bars("Iooking4deal Iicksack lron"), "looking4deal licksack Iron");
        assert_eq!(settle_bars("YlNO"), "YINO");
        assert_eq!(settle_bars("ElliottBoy"), "ElliottBoy");
        assert_eq!(settle_bars("SteeI Titans"), "Steel Titans");
        assert_eq!(settle_bars("MithriI Ore"), "Mithril Ore");
        assert_eq!(settle_bars("ApexKiIIKing"), "ApexKillKing");
        assert_eq!(settle_bars("BIvd has entered."), "Blvd has entered.");
        assert_eq!(settle_bars("llovestudy has entered."), "Ilovestudy has entered.");
        assert_eq!(settle_bars("IIovestudy"), "Ilovestudy");
        assert_eq!(settle("Nelson has left.the room."), "Nelson has left the room.");
        assert_eq!(settle("Total Sales /.Total Received"), "Total Sales / Total Received");
    }

    #[test]
    fn the_bitmap_font_is_used_at_ideal_ratio_and_its_double_only() {
        assert_eq!(bitmap_block(0.641), Some(1));
        assert_eq!(bitmap_block(1.282), Some(2));
        assert_eq!(bitmap_block(0.9), None);
        assert_eq!(bitmap_block(1.2), None);
        assert_eq!(bitmap_block(1.0), None);
    }
}
