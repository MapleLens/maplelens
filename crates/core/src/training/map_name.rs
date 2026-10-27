//! Which map the minimap's title names, settled from OCR readings (pure).
//!
//! The same title does not always read the same: the browser's frames are video, so Tesseract reads
//! "East Rocky Mountain III" one frame and "East Rocky Mountain Ill" the next, and sometimes drops a word's letter or
//! the Free Market's room. Neighbouring maps, though, differ by their numeral alone ("East Rocky Mountain V" and "VI").
//! So readings are compared with OCR's look-alikes of I folded together and the numeral apart: the same numeral (or one
//! missing) and nearly the same words is the same map. An exact comparison had taken every misread for a new map and
//! ended the session; a fuzzy one had taken V and VI for one map.

use crate::market::catalog::ratio;

/// Two readings of one map's words score at least this (a letter or two off); other maps' words score lower ("Ant
/// Tunnel" and "Ant Tunnel Park" 82).
const SAME_WORDS: f64 = 90.0;
/// A different map counts once this many readings in a row name it.
const CONFIRM: u32 = 2;

/// "East Rocky Mountain Ill" -> ("eastrockymountain", Some("iii")): letters and digits in lower case, I's look-alikes
/// read as i, and a last word that is a numeral (Roman or digits) kept apart.
fn key(text: &str) -> (String, Option<String>) {
    let fold = |c: char| match c.to_ascii_lowercase() {
        'l' | '1' | '|' | '!' => 'i',
        c => c,
    };
    let mut words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '|' | '!')).map(fold).collect())
        .filter(|w: &String| !w.is_empty())
        .collect();
    let numeral = |w: &String| w.chars().all(|c| matches!(c, 'i' | 'v' | 'x') || c.is_ascii_digit());
    let last = (words.len() > 1).then(|| words.last().filter(|w| numeral(w)).cloned()).flatten();
    if last.is_some() {
        words.pop();
    }
    (words.concat(), last)
}

/// Do two readings name the same map?
pub fn same_place(a: &str, b: &str) -> bool {
    let ((words_a, numeral_a), (words_b, numeral_b)) = (key(a), key(b));
    let numerals_agree = match (numeral_a, numeral_b) {
        (Some(x), Some(y)) => x == y,
        _ => true, // OCR left it out
    };
    numerals_agree && ratio(&words_a, &words_b) >= SAME_WORDS
}

/// The map the player is on, from readings of the minimap's title.
#[derive(Clone, Debug, Default)]
pub struct MapName {
    /// The settled map, as it was read when it settled (later readings of it do not change it).
    map: Option<String>,
    /// Another map being confirmed: its latest reading, and how many readings in a row named it.
    candidate: Option<(String, u32)>,
}

impl MapName {
    /// One reading of the title; returns the settled map.
    pub fn read(&mut self, text: &str) -> Option<&str> {
        if self.map.as_deref().is_some_and(|map| same_place(map, text)) {
            self.candidate = None;
        } else {
            let reads = match &self.candidate {
                Some((candidate, n)) if same_place(candidate, text) => n + 1,
                _ => 1,
            };
            if reads >= CONFIRM {
                self.map = Some(text.to_string());
                self.candidate = None;
            } else {
                self.candidate = Some((text.to_string(), reads));
            }
        }
        self.map.as_deref()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misreads_of_one_title_are_one_map() {
        for (a, b) in [
            ("East Rocky Mountain III", "East Rocky Mountain Ill"),
            ("East Rocky Mountain III", "East Rocky Mountaln lll"),
            ("East Rocky Mountain V", "East Rocky Mountain"), // the numeral left out
            ("Free Market <8>", "Free Market"),
            ("Free Market <8>", "Free Market <8›"),
            ("Ant Tunnel I", "Ant TunneI |"),
        ] {
            assert!(same_place(a, b), "{a} / {b}");
        }
    }

    #[test]
    fn neighbouring_maps_are_not() {
        for (a, b) in [
            ("East Rocky Mountain V", "East Rocky Mountain VI"),
            ("Ant Tunnel I", "Ant Tunnel II"),
            ("Ant Tunnel I", "Ant Tunnel Park"),
            ("Free Market <8>", "Free Market <3>"),
            ("Henesys", "Ellinia"),
        ] {
            assert!(!same_place(a, b), "{a} / {b}");
        }
    }

    #[test]
    fn a_new_map_settles_on_two_readings_and_misreads_never_move_it() {
        let mut name = MapName::default();
        assert_eq!(name.read("East Rocky Mountain V"), None);
        assert_eq!(name.read("East Rocky Mountain V"), Some("East Rocky Mountain V"));
        for misread in ["East Rocky Mountain", "East Rocky Mountaln V", "East Rocky Mountain V", "Ea5t Rocky Mountain"]
        {
            assert_eq!(name.read(misread), Some("East Rocky Mountain V"));
        }
        // One odd reading is not a move; two of the next map are.
        assert_eq!(name.read("East Rocky Mountain VI"), Some("East Rocky Mountain V"));
        assert_eq!(name.read("East Rocky Mountain V"), Some("East Rocky Mountain V"));
        assert_eq!(name.read("East Rocky Mountain VI"), Some("East Rocky Mountain V"));
        assert_eq!(name.read("East Rocky Mountain Vl"), Some("East Rocky Mountain Vl"));
        name.clear();
        assert_eq!(name.read("Henesys"), None);
    }
}
