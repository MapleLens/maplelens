//! Exact readings with unknown letters ('?') completed by OCR: OCR's letters are only taken where the exact reader
//! had none, and only when OCR agrees with every letter the exact reader is sure of (pure).

/// An unknown piece holds at most this many letters (letters drawn touching, a span of a vector font with the
/// letters it touched).
const GAP_LETTERS: usize = 6;

/// OCR's reading where it agrees with every letter of `exact` (each `?` one to a few letters), else `exact` with
/// OCR's letters in its gaps when both count the same letters, else OCR's reading. Spaces are not compared (OCR
/// drops and adds them); the result is spaced like `exact` when it fits.
pub fn fill_gaps(exact: &str, ocr: &str) -> String {
    if ocr.trim().is_empty() {
        // OCR read nothing (or there is none): the exact reading, its gaps left open.
        return exact.to_string();
    }
    let pattern: Vec<char> = exact.chars().filter(|c| !c.is_whitespace()).collect();
    let text: Vec<char> = ocr.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some(fills) = fit(&pattern, &text).filter(|fills| fills.iter().all(|f| plausible(f))) {
        let mut fills = fills.into_iter();
        return exact
            .chars()
            .map(|c| if c == '?' { fills.next().unwrap_or_default() } else { c.to_string() })
            .collect();
    }
    if fit(&pattern, &text).is_some() {
        // It fits, but with what cannot be text: the exact reading, its gaps left open.
        return exact.to_string();
    }
    if pattern.len() == text.len() {
        let mut letters = text.iter();
        return exact
            .chars()
            .map(|c| {
                if c.is_whitespace() {
                    return c;
                }
                let t = letters.next().copied().unwrap_or(c);
                if c == '?' { t } else { c }
            })
            .collect();
    }
    ocr.to_string()
}

/// What OCR puts in a gap must look like text: letters and digits, or one punctuation mark (the ':' after "Buyer").
/// A run like "zfy•yy" is OCR guessing at letters drawn touching.
fn plausible(fill: &str) -> bool {
    fill.chars().all(char::is_alphanumeric)
        || (fill.chars().count() == 1 && fill.chars().all(|c| c.is_ascii_punctuation()))
}

/// The letters each `?` of `pattern` stands for, when `text` fits it.
fn fit(pattern: &[char], text: &[char]) -> Option<Vec<String>> {
    match pattern.split_first() {
        None => text.is_empty().then(Vec::new),
        Some(('?', rest)) => (1..=GAP_LETTERS.min(text.len())).find_map(|n| {
            let mut fills = fit(rest, &text[n..])?;
            fills.insert(0, text[..n].iter().collect());
            Some(fills)
        }),
        Some((c, rest)) => (text.first() == Some(c)).then(|| fit(rest, &text[1..])).flatten(),
    }
}

/// Does `text` fit `pattern`, where `?` stands for one to a few letters?
pub fn fits(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().filter(|c| !c.is_whitespace()).collect();
    let t: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    fit(&p, &t).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_fills_only_the_gaps_and_must_agree_with_the_known_letters() {
        assert_eq!(fill_gaps("?a?y has entered.", "Mary has entered."), "Mary has entered.");
        // OCR's spacing does not matter; the exact reading's does.
        assert_eq!(
            fill_gaps("Bl?ckKity has left the room.", "BlackKity hasleft the room."),
            "BlackKity has left the room."
        );
        // Same letter count but a disagreement: the known letters win, the gap takes OCR's letter.
        assert_eq!(fill_gaps("Ro?kEyz", "RoaKEyz"), "RoakEyz");
        // Nothing fits: OCR's reading.
        assert_eq!(fill_gaps("Zo?", "Something else"), "Something else");
        assert!(fits("?amStory", "JamStory") && !fits("?amStory", "amStory"));
        // OCR's guess at letters drawn touching is not taken; one punctuation mark is.
        assert_eq!(fill_gaps("Cra? has entered.", "Crazfy•yy has entered."), "Cra? has entered.");
        assert_eq!(fill_gaps("Buyer? Frans", "Buyer: Frans"), "Buyer: Frans");
    }
}
