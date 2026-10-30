//! OCR'd tooltip title -> map.

use super::Location;
use crate::market::catalog::ratio;

/// Noisy reads ('Kerrting City Middle Forestl') still land close to the real name...
const MIN_SCORE: f64 = 80.0;
/// ...but the best candidate must beat the runner-up ('Ground I' vs 'Ground II').
const MIN_MARGIN: f64 = 4.0;
const EXACT_BONUS: f64 = 10.0;
/// Without a colon only a long phrase is trusted as a title ('Wi pin' is background noise).
const MIN_BARE_CHARS: usize = 12;

/// A search result: one map's name and street.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: u32,
    pub name: String,
    pub street: String,
}

/// What a real word of a map name may be made of.
fn is_word(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "'.,-".contains(c))
}

/// Drop edge tokens OCR made of stray symbols ('Ground I l/', '� Victoria'); hyphenated words stay.
fn strip_junk(text: &str) -> String {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    while words.len() > 1 && words.last().is_some_and(|w| !is_word(w) || !w.chars().any(char::is_alphanumeric)) {
        words.pop();
    }
    while words.len() > 1 && (!is_word(words[0]) || !words[0].chars().any(char::is_alphabetic)) {
        words.remove(0);
    }
    words.join(" ")
}

/// "Street : Name" with a street of at least three letters, spaces, apostrophes, dots or hyphens.
fn split_title(text: &str) -> Option<(&str, &str)> {
    let at = text.find([':', ';'])?;
    let (street, name) = (text[..at].trim(), text[at + 1..].trim());
    let valid = street.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && street.chars().count() >= 3
        && street.chars().all(|c| c.is_ascii_alphabetic() || " '.-".contains(c));
    (valid && !name.is_empty()).then_some((street, name))
}

/// 'i Warning Street' -> 'Warning Street': single letters in front are OCR debris.
fn drop_stray_letters(street: &str) -> &str {
    let mut rest = street;
    loop {
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            (Some(c), Some(w)) if c.is_ascii_alphabetic() && w.is_whitespace() => rest = rest[1..].trim_start(),
            _ => return rest,
        }
    }
}

/// "Victoria Road : Henesys" -> Location; None for anything else (item tooltips, UI hints, noise).
///
/// OCR sometimes drops the colon (or turns it into a symbol); the text then counts as a bare name (street "") if
/// it looks like a title.
pub fn parse_location(text: &str) -> Option<Location> {
    let text = strip_junk(text.trim());
    if let Some((street, name)) = split_title(&text)
        && name.chars().any(char::is_alphabetic)
    {
        return Some(Location::new(drop_stray_letters(street), &strip_junk(name)));
    }
    // A symbol where the colon should be ('Victoria Road � 3-Way') is dropped, leaving a bare phrase.
    let bare = text.split_whitespace().filter(|w| is_word(w)).collect::<Vec<_>>().join(" ");
    let letters = bare.chars().filter(|c| c.is_alphabetic() || " -'".contains(*c)).count();
    let length = bare.chars().count();
    let titled = bare.chars().next().is_some_and(char::is_uppercase);
    (bare.split(' ').count() >= 2 && length >= MIN_BARE_CHARS && titled && letters as f64 >= 0.8 * length as f64)
        .then(|| Location::new("", &bare))
}

/// The game font makes OCR swap these ('Ground l' for 'Ground I', 'Swarnp' for 'Swamp'); folding both sides costs
/// nothing.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .replace("rn", "m")
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .map(|c| match c {
            'l' | '1' | 'j' => 'i',
            '0' => 'o',
            other => other,
        })
        .collect()
}

/// Identity of a reading that ignores OCR look-alike slips ('Ground l' / 'Ground I').
pub fn key(location: &Location) -> String {
    fold(&format!("{}{}", location.street, location.name))
}

/// How well a search result matches what was read (0-115).
pub fn score(location: &Location, candidate: &Candidate) -> f64 {
    let (name, street, read) = (fold(&candidate.name), fold(&candidate.street), fold(&location.name));
    let joined = format!("{street}{name}");
    let points = if location.street.is_empty() {
        ratio(&read, &name).max(ratio(&read, &joined)) // no colon: the street is in the text
    } else {
        ratio(&read, &name) + if fold(&location.street) == street { 5.0 } else { 0.0 }
    };
    // Neighbouring maps differ by a numeral ('Ground I' / 'Ground II'): an exact reading must beat them clearly.
    points + if read == name || read == joined { EXACT_BONUS } else { 0.0 }
}

/// Id of the search result named like `location`, or None if none is close enough or two are tied.
pub fn pick<'a>(location: &Location, candidates: impl IntoIterator<Item = &'a Candidate>) -> Option<u32> {
    let mut ranked: Vec<(f64, u32)> = candidates.into_iter().map(|c| (score(location, c), c.id)).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let &(best, id) = ranked.first().filter(|(s, _)| *s >= MIN_SCORE)?;
    // Two maps that fit equally well: better no card than a wrong one.
    ranked.get(1).is_none_or(|(second, _)| best - second >= MIN_MARGIN).then_some(id)
}

/// Queries to try, most specific first (OCR may have mangled one word).
pub fn search_terms(location: &Location) -> Vec<String> {
    let mut words: Vec<&str> = Vec::new();
    for word in location.name.split_whitespace().filter(|w| w.chars().count() >= 4) {
        if !words.contains(&word) {
            words.push(word);
        }
    }
    words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    let mut terms = vec![location.name.clone()];
    terms.extend(words.into_iter().take(5).filter(|w| *w != location.name).map(String::from));
    terms
}
