//! Settles noisy OCR readings of one tooltip into a single stable answer.

use std::collections::VecDeque;

use crate::market::catalog::ratio;

/// Readings of one title differ by a few glyphs; different titles score far lower.
const SIMILAR: f64 = 80.0;

/// Keeps the last few (reading, payload) pairs; answers with the payload of the most typical reading.
///
/// The answer is held while enough recent readings still agree with it, so one odd frame cannot swap the location
/// or resize the card; a new tooltip takes over once `need` readings of it have been seen.
pub struct Voter<T: Clone> {
    recent: VecDeque<(String, T)>,
    size: usize,
    need: usize,
    keep: usize,
    held: Option<(String, T)>,
}

impl<T: Clone> Default for Voter<T> {
    fn default() -> Self {
        Self { recent: VecDeque::new(), size: 7, need: 3, keep: 2, held: None }
    }
}

impl<T: Clone> Voter<T> {
    pub fn clear(&mut self) {
        self.recent.clear();
        self.held = None;
    }

    pub fn is_empty(&self) -> bool {
        self.recent.is_empty()
    }

    pub fn add(&mut self, reading: &str, payload: T) -> Option<T> {
        if self.recent.len() == self.size {
            self.recent.pop_front();
        }
        self.recent.push_back((reading.to_string(), payload));
        if let Some((held, payload)) = &self.held
            && self.support(held) >= self.keep
        {
            return Some(payload.clone());
        }
        // Ties go to the earliest reading, like Python's max().
        let best = first_max(self.recent.iter(), |r| (self.support(&r.0) as f64, self.score(&r.0)))?.0.clone();
        if self.support(&best) < self.need {
            self.held = None;
            return None;
        }
        // The medoid of the agreeing readings: the one closest to all the others.
        let held = first_max(self.recent.iter().filter(|r| same(&r.0, &best)), |r| (0.0, self.score(&r.0)))?.clone();
        let answer = held.1.clone();
        self.held = Some(held);
        Some(answer)
    }

    fn support(&self, reading: &str) -> usize {
        self.recent.iter().filter(|r| same(reading, &r.0)).count()
    }

    fn score(&self, reading: &str) -> f64 {
        self.recent.iter().map(|r| ratio(reading, &r.0)).sum()
    }
}

fn first_max<'a, T: 'a>(items: impl Iterator<Item = &'a T>, key: impl Fn(&T) -> (f64, f64)) -> Option<&'a T> {
    let mut best: Option<(&T, (f64, f64))> = None;
    for item in items {
        let k = key(item);
        if best.as_ref().is_none_or(|(_, b)| k > *b) {
            best = Some((item, k));
        }
    }
    best.map(|(item, _)| item)
}

fn same(a: &str, b: &str) -> bool {
    ratio(a, b) >= SIMILAR
}
