//! Name -> Item lookup that tolerates misreads ('Emerald 0re', stray punctuation, case, unknown glyphs '?').

use std::collections::HashMap;
use std::sync::Arc;

use super::models::Item;
use crate::config::{FUZZY_MARGIN, MIN_MATCH_SCORE};

/// Read text without trailing one- or two-character junk ('Kitty Camping Hat ii' -> 'Kitty Camping Hat').
pub fn clean_title(text: &str) -> String {
    let trimmed = text.trim();
    let mut end = trimmed;
    // Drop " xy" tails (1-2 of [A-Za-z0-9|!] after whitespace) while there are any.
    while let Some(space) = end.rfind(char::is_whitespace) {
        let tail = &end[space..].trim_start();
        let junk = (1..=2).contains(&tail.chars().count())
            && tail.chars().all(|c| c.is_ascii_alphanumeric() || c == '|' || c == '!');
        if !junk {
            break;
        }
        end = end[..space].trim_end();
    }
    if end.is_empty() { trimmed.to_string() } else { end.to_string() }
}

/// `text` without a trailing gender mark ('Undershirt (M)'), however the parentheses were read ('lM?', 'IF)'): the
/// catalog names the item without it.
pub fn without_gender(text: &str) -> String {
    let trimmed = text.trim_end();
    let Some(space) = trimmed.rfind(char::is_whitespace) else { return trimmed.to_string() };
    let tail = &trimmed[space..].trim_start();
    let gender = tail.chars().filter(|c| matches!(c, 'M' | 'F')).count() == 1
        && (1..=4).contains(&tail.chars().count())
        && tail.chars().all(|c| matches!(c, 'M' | 'F') || GENDER_BRACKETS.contains(c));
    if gender { trimmed[..space].trim_end().to_string() } else { trimmed.to_string() }
}

/// `text` without a trailing enhancement mark ('Steel Pow (+3)'), however it was read ('1+3)', '(+3'): the catalog
/// names the unenhanced item.
pub fn without_enhancement(text: &str) -> String {
    let trimmed = text.trim_end();
    let Some(space) = trimmed.rfind(char::is_whitespace) else { return trimmed.to_string() };
    let tail = trimmed[space..].trim_start();
    let mark = tail.contains('+')
        && tail.chars().count() <= 6
        && tail.chars().all(|c| c.is_ascii_digit() || c == '+' || ENHANCEMENT_BRACKETS.contains(c));
    if mark { trimmed[..space].trim_end().to_string() } else { trimmed.to_string() }
}

/// What the parentheses around an enhancement read as.
const ENHANCEMENT_BRACKETS: &str = "()[]{}|lI!?K";

/// What the parentheses around a gender mark read as.
const GENDER_BRACKETS: &str = "()[]{}|lI1!?K";

/// An item name with this many digits has a number that identifies it (coupons by their value).
const NUMBER_DIGITS: usize = 3;

fn has_number(name: &str) -> bool {
    name.chars().filter(char::is_ascii_digit).count() >= NUMBER_DIGITS
}

/// The numbers written in `text` ('(10,000)' -> ["10,000"]).
fn numbers(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_digit() || c == ','))
        .map(|run| run.trim_matches(',').to_string())
        .filter(|run| run.chars().any(|c| c.is_ascii_digit()))
        .collect()
}

/// Do the numbers of a read name and an item's name agree? Exactly, or digit for digit when the read lost its
/// commas (OCR drops them; it does not move them).
fn same_numbers(read: &str, name: &str) -> bool {
    let (read, name) = (numbers(read), numbers(name));
    read == name
        || (read.iter().all(|n| !n.contains(','))
            && read.iter().map(|n| n.replace(',', "")).eq(name.iter().map(|n| n.replace(',', ""))))
}

/// Lowercase letters and digits only: what two spellings of one name have in common.
/// A cut name must keep this many letters and digits to name an item.
const MIN_TRUNCATED_KEY: usize = 8;
/// The game cuts only long titles (past ~30 letters at the tooltip's edge): a shorter read that starts names is a
/// misread (a letter lost at its end), left to the fuzzy match.
const MIN_CUT_KEY: usize = 24;

fn key(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Pairs of letters OCR engines swap in the game font ('Stiff' -> 'Stitt', 'Ore' -> '0re', 'Iron' -> 'Iran'). Swapping
/// one costs a quarter of a wrong letter in `similarity`, so one confusion costs little and a name with several still
/// fits, while the cutoff stays as high as for real typos.
const CONFUSABLE: [(u8, u8); 15] = [
    (b'o', b'0'),
    (b'o', b'q'),
    (b'o', b'a'),
    (b'o', b'c'),
    (b'l', b'i'),
    (b'l', b'1'),
    (b'i', b'1'),
    (b'e', b'c'),
    (b'f', b't'),
    (b's', b'5'),
    (b'u', b'v'),
    (b'n', b'h'),
    (b'n', b'r'),
    (b'g', b'q'),
    (b'g', b'9'),
];
/// A wrong letter costs 2 (one letter lost and one gained, as in `ratio`); a confusable one this much.
const CONFUSABLE_COST: f32 = 0.5;

fn confusable(a: u8, b: u8) -> bool {
    CONFUSABLE.iter().any(|&(x, y)| (a == x && b == y) || (a == y && b == x))
}

/// Similarity 0..100 of two keys: `ratio`'s measure (insertions and deletions cost 1, a wrong letter 2) except that a
/// confusable letter costs `CONFUSABLE_COST`. None when it is under `cutoff`, which lets the scan give up on a candidate
/// as soon as even its best row cannot get the total cost under the budget (nearly all names are far from a read).
fn similarity(a: &[u8], b: &[u8], cutoff: f64) -> Option<f64> {
    let total = (a.len() + b.len()) as f64;
    if total == 0.0 {
        return Some(100.0);
    }
    let budget = ((1.0 - cutoff / 100.0) * total) as f32;
    if (a.len() as f32 - b.len() as f32).abs() > budget {
        return None;
    }
    let mut previous: Vec<f32> = (0..=b.len()).map(|j| j as f32).collect();
    let mut row = vec![0.0f32; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        row[0] = (i + 1) as f32;
        let mut cheapest = row[0];
        for (j, &cb) in b.iter().enumerate() {
            let swap = if ca == cb {
                0.0
            } else if confusable(ca, cb) {
                CONFUSABLE_COST
            } else {
                2.0
            };
            let cost = (previous[j] + swap).min(previous[j + 1] + 1.0).min(row[j] + 1.0);
            row[j + 1] = cost;
            cheapest = cheapest.min(cost);
        }
        // Costs only grow along a path, and every path crosses this row.
        if cheapest > budget {
            return None;
        }
        std::mem::swap(&mut previous, &mut row);
    }
    let score = 100.0 * (1.0 - f64::from(previous[b.len()]) / total);
    (score >= cutoff).then_some(score)
}

/// Similarity 0..100 like rapidfuzz's `fuzz.ratio`: 2 * longest common subsequence / total length.
pub fn ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() && b.is_empty() {
        return 100.0;
    }
    let mut row = vec![0usize; b.len() + 1];
    for &ca in &a {
        let mut diagonal = 0;
        for (j, &cb) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if ca == cb { diagonal + 1 } else { up.max(row[j]) };
            diagonal = up;
        }
    }
    200.0 * row[b.len()] as f64 / (a.len() + b.len()) as f64
}

/// Does `key` fit `pattern`, where each `?` (an unknown glyph run) stands for 1 to 3 characters?
fn fits(pattern: &[char], key: &[char]) -> bool {
    match pattern.split_first() {
        None => key.is_empty(),
        Some(('?', rest)) => (1..=3.min(key.len())).any(|n| fits(rest, &key[n..])),
        Some((c, rest)) => key.first() == Some(c) && fits(rest, &key[1..]),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    /// When MesoWatch exported the data (shown as data age).
    pub generated_at: Option<String>,
    by_key: HashMap<String, Arc<Item>>,
    keys: Vec<String>,
}

impl Catalog {
    pub fn new(items: impl IntoIterator<Item = Item>, generated_at: Option<String>) -> Self {
        let mut by_key: HashMap<String, Arc<Item>> = HashMap::new();
        let mut keys = Vec::new();
        for item in items {
            let k = key(&item.name);
            match by_key.get(&k) {
                // Names can collide (variants of one item); keep the one with the most sales evidence.
                Some(known) if item.sold <= known.sold => {}
                Some(_) => {
                    by_key.insert(k, Arc::new(item));
                }
                None => {
                    keys.push(k.clone());
                    by_key.insert(k, Arc::new(item));
                }
            }
        }
        Self { generated_at, by_key, keys }
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    pub fn items(&self) -> impl Iterator<Item = &Arc<Item>> {
        self.keys.iter().filter_map(|k| self.by_key.get(k))
    }

    /// The item a read title names, if any.
    pub fn lookup(&self, text: &str) -> Option<Arc<Item>> {
        // As read first: some names carry the mark themselves ('Natalie's Fashion Box (M)' and '(F)' are two items).
        let item = self.lookup_read(text).or_else(|| {
            let bare = without_gender(&clean_title(&without_enhancement(text)));
            (bare != text.trim()).then(|| self.lookup_read(&bare)).flatten()
        })?;
        (!self.lost_gender_mark(text, &item)).then_some(item)
    }

    /// The item a read names letter for letter (case, spaces and punctuation aside; a gender or enhancement mark
    /// may be on the read and not on the name). Never fuzzy: for reads known to be exact, such as the game's own
    /// font, where the closest other name would be another item.
    pub fn lookup_exact(&self, text: &str) -> Option<Arc<Item>> {
        let bare = without_gender(&clean_title(&without_enhancement(text)));
        let item = [text, bare.as_str()].into_iter().find_map(|candidate| {
            let item = self.by_key.get(&key(candidate))?;
            (!has_number(&item.name) || same_numbers(candidate, &item.name)).then(|| item.clone())
        })?;
        (!self.lost_gender_mark(text, &item)).then_some(item)
    }

    /// An exact read with gaps (the game's own font, '?' where it met a shape it has not learnt): the one item whose
    /// name fits the letters around them, each gap 1 to 3 characters. Never fuzzy, as `lookup_exact`; nothing when
    /// several names fit or a third of the read is gaps.
    pub fn lookup_gaps(&self, text: &str) -> Option<Arc<Item>> {
        let bare = without_gender(&without_enhancement(text));
        // A gap that is a word of its own at either end is something beside the title (scenery read from a spot a
        // little off), not one of its letters: "? Pole Arm" must not name "Axe Pole Arm".
        let words: Vec<&str> = bare.split_whitespace().collect();
        let alone = |w: Option<&&str>| w.is_some_and(|w| w.chars().all(|c| c == '?'));
        if words.len() > 1 && (alone(words.first()) || alone(words.last())) {
            return None;
        }
        // Punctuation counts here: a letter read as "(r((" must not leave "rd" to fit "Board".
        let spelled = |t: &str| -> Vec<char> {
            t.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_ascii_lowercase()).collect()
        };
        let pattern = spelled(&bare);
        let unknown = pattern.iter().filter(|&&c| c == '?').count();
        if unknown == 0 || pattern.len() < 3 || unknown * 3 > pattern.len() {
            return None;
        }
        let mut fitting = self.items().filter(|it| fits(&pattern, &spelled(&it.name)));
        let item = match (fitting.next(), fitting.next()) {
            (Some(only), None) => only.clone(),
            _ => return None,
        };
        (!self.lost_gender_mark(text, &item)).then_some(item)
    }

    /// A name the game cut to fit its box ("Two-handed Axe Attack Scr..", the Sold Items list): the one item whose
    /// name starts with what is left. None when it was not cut, when several names start so (the scroll's grade is
    /// what was cut off) and when too little is left. Never fuzzy: the cut part is unknown, not misread.
    pub fn lookup_truncated(&self, text: &str) -> Option<Arc<Item>> {
        let stem = text.trim_end().strip_suffix("..")?.trim_end_matches('.');
        let k = key(stem);
        if k.len() < MIN_TRUNCATED_KEY {
            return None;
        }
        let mut starting = self.keys.iter().filter(|c| c.starts_with(&k));
        match (starting.next(), starting.next()) {
            (Some(only), None) => self.by_key.get(only).cloned(),
            _ => None,
        }
    }

    /// A read with brackets but no gender letter ('Red Cloth Pants ()') for an item that exists with and without
    /// one: the letter was lost, and which of the two it was cannot be told. (An enhancement mark has brackets too.)
    ///
    /// A letter that was read tells: 'Dark Shadow Pants (M)' where the catalog has the plain name and its (F) is the
    /// plain one (an (F) read would have named the (F) item before this is asked).
    fn lost_gender_mark(&self, text: &str, item: &Item) -> bool {
        let brackets = text.contains("()") || text.trim_end().ends_with('(') || text.trim_end().ends_with(')');
        let key = key(&item.name);
        let has = |g: &str| self.by_key.contains_key(&format!("{key}{g}"));
        let read = text.trim_end();
        let letter_read =
            (read.ends_with("(M)") && has("f") && !has("m")) || (read.ends_with("(F)") && has("m") && !has("f"));
        brackets && !letter_read && !text.contains('+') && !item.name.contains('(') && (has("f") || has("m"))
    }

    fn lookup_read(&self, text: &str) -> Option<Arc<Item>> {
        if text.contains('?') {
            return self.lookup_pattern(text);
        }
        for candidate in [text.to_string(), clean_title(text)] {
            let k = key(&candidate);
            if k.is_empty() {
                return None;
            }
            // Names are keyed without punctuation, so '(10,00)' and '(1,000)' are one key: the numbers must also agree.
            if let Some(item) = self.by_key.get(&k)
                && (!has_number(&item.name) || same_numbers(&candidate, &item.name))
            {
                return Some(item.clone());
            }
            // The start of names (the game cut a long title at the tooltip's edge): the one name it starts, or none
            // when it starts several. A fuzzy match would name the closest of them ('...Scroll: Interme' as Greater).
            if k.len() >= MIN_CUT_KEY {
                // The bars folded: a cut "...Attack ScroI" starts every grade as much as "...Scrol" does.
                let bars = |t: &str| t.replace(['i', '1'], "l");
                let folded = bars(&k);
                let mut starting = self.keys.iter().filter(|c| c.len() > k.len() && bars(c).starts_with(&folded));
                match (starting.next(), starting.next()) {
                    (Some(only), None) => return self.by_key.get(only).cloned(),
                    (Some(_), Some(_)) => return None,
                    _ => {}
                }
            }
            if let Some(item) = self.fuzzy(&candidate, &k) {
                return Some(item);
            }
        }
        None
    }

    /// The item whose name is closest to the read, when it is clearly the closest: two names about as close ('Hat
    /// P Scroll' for HP or MP, 'Dark Cala' for Calaf or Calas) leave no basis for a pick, and a wrong price is
    /// worse than no card.
    fn fuzzy(&self, read: &str, key: &str) -> Option<Arc<Item>> {
        let (mut best, mut runner_up): (Option<(f64, usize)>, f64) = (None, f64::MIN);
        for (i, candidate) in self.keys.iter().enumerate() {
            let Some(score) = similarity(key.as_bytes(), candidate.as_bytes(), MIN_MATCH_SCORE) else { continue };
            // Numbers are never fuzzy: 'Leaf Points (1,000) Exchange Coupon' is not the (10,000) one with a zero lost.
            let name = &self.by_key[&self.keys[i]].name;
            if has_number(name) && !same_numbers(read, name) {
                continue;
            }
            match best {
                Some((s, _)) if score <= s => runner_up = runner_up.max(score),
                _ => {
                    runner_up = best.map_or(runner_up, |(s, _)| runner_up.max(s));
                    best = Some((score, i));
                }
            }
        }
        let (score, i) = best?;
        (score - runner_up >= FUZZY_MARGIN).then(|| self.by_key.get(&self.keys[i]).cloned()).flatten()
    }

    /// A read with unknown glyphs: the one item whose name fits the known letters around the gaps; nothing when
    /// several do; the closest name with the gaps left out when none does. Needs most of the name read: `?????` fits
    /// too much.
    fn lookup_pattern(&self, text: &str) -> Option<Arc<Item>> {
        let pattern: Vec<char> =
            text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '?').map(|c| c.to_ascii_lowercase()).collect();
        let unknown = pattern.iter().filter(|&&c| c == '?').count();
        if pattern.len() < 3 || unknown * 3 > pattern.len() {
            return None;
        }
        let mut fitting = self.keys.iter().filter(|k| fits(&pattern, &k.chars().collect::<Vec<_>>()));
        match (fitting.next(), fitting.next()) {
            (Some(only), None) => self.by_key.get(only).cloned(),
            // Several names fit ('?P Reset Scroll': AP or SP): any pick would be a guess.
            (Some(_), Some(_)) => None,
            // None fits: a letter was misread as well; the closest name to the letters that were read.
            (None, _) => {
                let known: String = pattern.iter().filter(|&&c| c != '?').collect();
                self.fuzzy(text, &known)
            }
        }
    }
}
