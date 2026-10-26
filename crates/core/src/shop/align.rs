//! Which entries of a list are new, from two snapshots of it.
//!
//! Neither list carries timestamps and identical entries are real (the same buyer buying 1000 arrows twice), so
//! entries are never deduplicated by content. Instead the new snapshot is slid against the old one and everything
//! that does not belong to the overlap is new. Ambiguity resolves to the smaller shift: missing a sale is
//! recoverable, counting one twice is not.

/// Entries that must still agree before one spoiled entry is forgiven.
const TOLERANT_OVERLAP: usize = 4;
/// One matching entry could be a coincidence; the same shape recurs all the time.
const MIN_OVERLAP: usize = 2;

fn misses<T>(a: &[T], b: &[T], same: &impl Fn(&T, &T) -> bool) -> usize {
    a.iter().zip(b).filter(|(x, y)| !same(x, y)).count()
}

/// The game draws its mouse pointer into the frame; over a list it ruins one entry.
fn allowed(overlap: usize, forgive: bool) -> usize {
    usize::from(forgive && overlap >= TOLERANT_OVERLAP)
}

fn front<T>(old: &[T], new: &[T], same: &impl Fn(&T, &T) -> bool, forgive: bool) -> Option<usize> {
    let need = old.len().min(MIN_OVERLAP);
    (0..(new.len() + 1).saturating_sub(need)).find(|&k| {
        let overlap = old.len().min(new.len() - k);
        misses(&new[k..k + overlap], &old[..overlap], same) <= allowed(overlap, forgive)
    })
}

fn back<T>(old: &[T], new: &[T], same: &impl Fn(&T, &T) -> bool, forgive: bool) -> Option<usize> {
    let need = old.len().min(MIN_OVERLAP);
    (0..(old.len() + 1).saturating_sub(need)).find_map(|k| {
        let overlap = old.len() - k;
        (overlap <= new.len() && misses(&old[k..], &new[..overlap], same) <= allowed(overlap, forgive))
            .then(|| new.len() - overlap)
    })
}

/// Count of entries added on top (newest first, like the Sold Items list); None when the lists don't align.
///
/// The visible page holds a fixed number of rows, so the oldest rows of `old` may have been pushed out of `new`.
/// Exact alignment is tried first; only then is one spoiled entry forgiven, and only given plenty of overlap.
pub fn new_at_front<T>(old: &[T], new: &[T], same: impl Fn(&T, &T) -> bool) -> Option<usize> {
    if old.is_empty() {
        return Some(new.len());
    }
    front(old, new, &same, false).or_else(|| front(old, new, &same, true))
}

/// Count of entries appended at the bottom (the chat log); None when the lists don't align.
///
/// A full log scrolls up as lines arrive, so the head of `old` may have scrolled out of `new`.
pub fn new_at_back<T>(old: &[T], new: &[T], same: impl Fn(&T, &T) -> bool) -> Option<usize> {
    back(old, new, &same, false).or_else(|| back(old, new, &same, true))
}
