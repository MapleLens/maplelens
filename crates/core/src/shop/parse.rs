//! Turn OCR'd text of the shop's lines into events.

use super::ShopEvent;
use crate::market::catalog::ratio;

const ENTERED: &str = "has entered.";
const LEFT: &str = "has left the room.";
/// OCR turns "has" into "hae" and drops full stops; the tail is still obviously the phrase.
const TAIL_MATCH: f64 = 80.0;
const TAIL_SLACK: usize = 2;
/// A real chat line has words; single stray glyphs are noise.
const MIN_MESSAGE_CHARS: usize = 3;
/// The name was lost to OCR but the tail was read.
pub const NO_NAME: &str = "?";

/// The name in front of `phrase` (fuzzily matched at the end of `text`); None when the text does not end in it.
fn split_tail(text: &str, phrase: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let phrase_len = phrase.chars().count();
    let (mut best_score, mut best_name) = (0.0, String::new());
    for length in phrase_len - TAIL_SLACK..=phrase_len + TAIL_SLACK {
        let cut = chars.len().saturating_sub(length);
        let tail: String = chars[cut..].iter().collect::<String>().to_lowercase();
        let score = ratio(&tail, phrase);
        if score > best_score {
            best_score = score;
            best_name = chars[..cut].iter().collect::<String>().trim().to_string();
        }
    }
    (best_score >= TAIL_MATCH).then(|| if best_name.is_empty() { NO_NAME.to_string() } else { best_name })
}

/// None for a line that is only OCR debris (the log redrawing, a window being dragged).
pub fn parse_chat(text: &str) -> Option<ShopEvent> {
    let text = text.trim();
    if let Some(name) = split_tail(text, ENTERED) {
        return Some(ShopEvent::PlayerEntered(name));
    }
    if let Some(name) = split_tail(text, LEFT) {
        return Some(ShopEvent::PlayerLeft(name));
    }
    (text.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_MESSAGE_CHARS)
        .then(|| ShopEvent::ChatMessage(with_separator(text)))
}

/// Marks a chat line's " : " is misread as.
const SEPARATORS: &[char] = &[':', '.', ';', ',', '\u{FFFD}', '\u{B7}', '\u{2022}'];

/// A chat line is "name : message": the colon read as another mark ("Frost . 100% gloves?", "Pelqulick. do you") or
/// dropped, leaving two spaces, is put back so the speaker and the message come apart. A line holding " : " stays.
fn with_separator(text: &str) -> String {
    if text.contains(" : ") {
        return text.to_string();
    }
    // A mark standing alone between words is the separator, and all before it the name ("Yytitf31 6 . anyone"): names
    // have no spaces, a narrow '1' leaves a gap.
    let words: Vec<&str> = text.split(' ').collect();
    if let Some(at) = words.iter().position(|w| w.chars().count() == 1 && w.starts_with(SEPARATORS))
        && at > 0
        && at + 1 < words.len()
    {
        return format!("{} : {}", words[..at].concat(), words[at + 1..].join(" ").trim_start());
    }
    let Some((name, rest)) = text.split_once(' ') else { return text.to_string() };
    let glued = name.chars().count() > 1 && name.ends_with(SEPARATORS);
    let name = if glued { name.trim_end_matches(SEPARATORS) } else { name };
    let after = rest.trim_start();
    let mark = after.chars().next().filter(|c| SEPARATORS.contains(c));
    let marked = mark.is_some_and(|m| after[m.len_utf8()..].starts_with(' '));
    if glued {
        format!("{name} : {after}")
    } else if let Some(m) = mark.filter(|_| marked) {
        format!("{name} : {}", after[m.len_utf8()..].trim_start())
    } else if rest.starts_with(' ') {
        format!("{name} : {after}")
    } else {
        text.to_string()
    }
}

/// A half-read sale is still a sale (it was counted by its shape): quantity falls back to 1, buyer to raw text.
pub fn parse_sale(item: &str, buyer: &str, quantity: &str) -> ShopEvent {
    let lower = buyer.to_ascii_lowercase(); // same byte offsets as `buyer`
    let named = lower.find("buyer").and_then(|at| {
        let rest = buyer[at + "buyer".len()..].trim_start();
        rest.strip_prefix(':').map(|name| name.trim().to_string()).filter(|name| !name.is_empty())
    });
    // The trailing run of digits, spaces and commas ("1 000", "1,000").
    let tail = quantity.trim_end();
    let start = tail.rfind(|c: char| !(c.is_ascii_digit() || c == ',' || c.is_whitespace())).map_or(0, |i| i + 1);
    let digits: String = tail[start..].chars().filter(char::is_ascii_digit).collect();
    ShopEvent::Sale {
        item: item.trim().to_string(),
        buyer: named.unwrap_or_else(|| buyer.trim().to_string()),
        quantity: digits.parse().unwrap_or(1),
        mesos: None,
        item_id: None,
    }
}
