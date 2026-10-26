//! Player names read off the shop's lines are noisy ("AssassinSSR" entered, "As sass jnSSR" bought): the same
//! player's name is read many ways, each a little different. A player who leaves or buys was in the store, so every
//! read is matched against the players already seen, those inside the store first, and reported as the spelling
//! read most often.

use super::parse::NO_NAME;

/// A name this long (letters and digits) may differ from another read of it by one letter, and from this many on by
/// two: short names differ from each other by one letter all the time ("Zift" / "Zift2" are two players).
const ONE_OFF_FROM: usize = 7;
const TWO_OFF_FROM: usize = 11;
/// Among the players inside the store a read may be further off (at most a third of its letters wrong): there are a
/// few of them, and one of them is who leaves or buys.
const INSIDE_MIN_LETTERS: usize = 4;
const INSIDE_ERRORS_PER_LETTER: usize = 3;
/// Fewer letters than this carry no identity (a stray glyph).
const MIN_LETTERS: usize = 3;

/// Letters the game's font draws alike, folded to one: l, I, i, j and 1 differ by a pixel, and so do 0 and O, or a
/// stray accent for a letter.
fn fold(c: char) -> char {
    match c.to_ascii_lowercase() {
        'i' | 'l' | '1' | '|' | 'j' | '!' => 'l',
        'o' | '0' | 'q' | 'ö' | 'ó' => 'o',
        'ü' | 'ú' | 'ù' | 'û' => 'u',
        'ß' | '6' | '8' => 'b',
        '5' => 's',
        c => c,
    }
}

/// What two reads of one name share, as one string: letters and digits, case and look-alikes aside.
pub fn same_name_key(name: &str) -> String {
    key(name).into_iter().collect()
}

/// What two reads of one name share: its letters and digits, case and look-alikes aside, spaces and punctuation
/// dropped (OCR splits "AssassinSSR" into "As sass jnSSR").
fn key(name: &str) -> Vec<char> {
    name.chars().filter(|c| c.is_alphanumeric()).map(fold).collect()
}

fn distance(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(x != y)).min(row[j] + 1).min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Edits allowed between two reads of one name.
fn slack(a: &[char], b: &[char]) -> usize {
    match a.len().min(b.len()) {
        n if n >= TWO_OFF_FROM => 2,
        n if n >= ONE_OFF_FROM => 1,
        _ => 0,
    }
}

/// Edits allowed between a read and a player known to be inside.
fn slack_inside(a: &[char], b: &[char]) -> usize {
    let (shorter, longer) = (a.len().min(b.len()), a.len().max(b.len()));
    if shorter < INSIDE_MIN_LETTERS { 0 } else { longer / INSIDE_ERRORS_PER_LETTER }
}

/// One way a name was read: its text, its key and how often.
#[derive(Clone)]
struct Spelling {
    text: String,
    key: Vec<char>,
    reads: u32,
}

/// One player and the ways their name was read.
#[derive(Clone)]
struct Player {
    spellings: Vec<Spelling>,
    inside: bool,
}

impl Player {
    /// The most-read spelling; the first to be read when several are level.
    fn name(&self) -> &str {
        let mut best = &self.spellings[0];
        for spelling in &self.spellings {
            if spelling.reads > best.reads {
                best = spelling;
            }
        }
        &best.text
    }

    /// How far a read is from the closest of the player's spellings, when within the edits `allowed`.
    fn distance_to(&self, key: &[char], allowed: fn(&[char], &[char]) -> usize) -> Option<usize> {
        self.spellings
            .iter()
            .filter_map(|s| {
                let d = distance(key, &s.key);
                (d <= allowed(key, &s.key)).then_some(d)
            })
            .min()
    }
}

/// The players of one shop session; forget them when the store closes.
#[derive(Clone, Default)]
pub struct NameBook {
    players: Vec<Player>,
}

impl NameBook {
    pub fn clear(&mut self) {
        self.players.clear();
    }

    /// A visitor came in.
    pub fn entered(&mut self, read: &str) -> String {
        self.settle(read, false, |p| p.inside = true)
    }

    /// A visitor left: one of those inside.
    pub fn left(&mut self, read: &str) -> String {
        self.settle(read, true, |p| p.inside = false)
    }

    /// A buyer: one of those inside, or at least a player seen.
    pub fn buyer(&mut self, read: &str) -> String {
        self.settle(read, true, |_| {})
    }

    /// The name to report for a read: the most-read spelling of the player it belongs to, or the read itself when it
    /// is nobody's yet (it becomes a player) or could be two different players.
    fn settle(&mut self, read: &str, from_inside: bool, update: impl FnOnce(&mut Player)) -> String {
        let read = read.trim();
        let key = key(read);
        if read == NO_NAME || key.len() < MIN_LETTERS {
            return read.to_string();
        }
        let exact = self.players.iter().position(|p| p.spellings.iter().any(|s| s.text == read));
        let mut ambiguous = false;
        let found = exact.or_else(|| {
            let mut candidates = Vec::new();
            if from_inside {
                candidates = self.nearest(&key, |p| p.inside, slack_inside);
            }
            if candidates.is_empty() {
                candidates = self.nearest(&key, |_| true, slack);
            }
            ambiguous = candidates.len() > 1;
            match candidates.as_slice() {
                [only] => Some(*only),
                _ => None,
            }
        });
        let index = match found {
            Some(index) => index,
            // Equally close to different players: reported as read, counted for none.
            None if ambiguous => return read.to_string(),
            None => {
                self.players.push(Player { spellings: Vec::new(), inside: false });
                self.players.len() - 1
            }
        };
        let player = &mut self.players[index];
        match player.spellings.iter_mut().find(|s| s.text == read) {
            Some(spelling) => spelling.reads += 1,
            None => player.spellings.push(Spelling { text: read.to_string(), key, reads: 1 }),
        }
        update(player);
        player.name().to_string()
    }

    /// The players passing `filter` closest to `key` within `allowed` edits; several when they are level.
    fn nearest(
        &self,
        key: &[char],
        filter: impl Fn(&Player) -> bool,
        allowed: fn(&[char], &[char]) -> usize,
    ) -> Vec<usize> {
        let scored: Vec<(usize, usize)> = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, p)| filter(p))
            .filter_map(|(i, p)| p.distance_to(key, allowed).map(|d| (i, d)))
            .collect();
        let best = scored.iter().map(|&(_, d)| d).min();
        scored.into_iter().filter(|&(_, d)| Some(d) == best).map(|(i, _)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::NameBook;

    #[test]
    fn a_name_read_many_ways_is_reported_as_the_one_read_most() {
        let mut book = NameBook::default();
        assert_eq!(book.entered("AssassinSSR"), "AssassinSSR");
        assert_eq!(book.buyer("As sass jnSSR"), "AssassinSSR");
        assert_eq!(book.left("AssassinSSR"), "AssassinSSR");
        assert_eq!(book.buyer("AssassjnSSR"), "AssassinSSR");
    }

    #[test]
    fn a_misread_seen_first_gives_way_to_the_right_one_read_more_often() {
        let mut book = NameBook::default();
        assert_eq!(book.entered("WebblWobbly"), "WebblWobbly");
        assert_eq!(book.entered("WebblyWobbly"), "WebblWobbly"); // one read each: the first stands
        assert_eq!(book.entered("WebblyWobbly"), "WebblyWobbly"); // now the most read
        assert_eq!(book.entered("WebblWobbly"), "WebblWobbly"); // level again: the first stands
    }

    #[test]
    fn whoever_leaves_is_one_of_those_inside() {
        let mut book = NameBook::default();
        book.entered("Bunily");
        book.entered("Zoroaz");
        // One letter wrong in a short name: only the players inside are near enough.
        assert_eq!(book.left("Burlil"), "Bunily");
        assert_eq!(book.left("Zoroaz"), "Zoroaz");
        // Nobody is inside any more: a stray short read is not Bunily.
        assert_eq!(book.left("Burlil"), "Burlil");
    }

    #[test]
    fn short_names_and_different_players_stay_apart() {
        let mut book = NameBook::default();
        for read in ["Zift", "Zift2", "Zoro", "Zoroaz", "Znew"] {
            assert_eq!(book.entered(read), read);
        }
        // Two players two letters apart: a read one letter from both names neither.
        let mut book = NameBook::default();
        book.entered("ShotsAB");
        book.entered("ShotsCD");
        assert_eq!(book.entered("ShotsAD"), "ShotsAD");
    }

    #[test]
    fn nameless_reads_are_left_alone() {
        let mut book = NameBook::default();
        assert_eq!(book.entered("?"), "?");
        assert_eq!(book.left(""), "");
        book.entered("Champi");
        assert_eq!(book.left("?"), "?");
    }
}
