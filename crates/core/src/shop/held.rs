//! Events whose text is still being read, kept in the order they happened.
//!
//! The browser's OCR answers a moment after it is asked. A look whose text was not in used to be undone and tried
//! again, lists and all; while the counter and the Sold Items page kept changing (one buyer took six items in twelve
//! seconds) every look was undone until the page had moved on by more rows than it shows, could no longer be aligned
//! with the last one believed, and four sales were lost. Now the lists move on at once and only the events wait, each
//! with the glyphs it was first read from, so OCR is asked about the same pixels until it answers.

use super::ShopEvent;
use crate::vision::Mask;

/// One line of text as first seen: the shop font's reading, final when it knew every letter; otherwise its reading
/// with '?' gaps (if any) and the glyphs OCR is asked to read.
#[derive(Clone, Debug)]
pub(super) struct LineRead {
    pub exact: Option<String>,
    pub glyphs: Option<Mask>,
}

impl LineRead {
    pub fn known(text: String) -> Self {
        Self { exact: Some(text), glyphs: None }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Held {
    /// Needs no text (the store opening, a list lost).
    Event(ShopEvent),
    /// The rows new in one look, oldest first (item, buyer, quantity lines), and the counter when it moved: a lone
    /// sale takes its change. With no rows it only keeps the counter's value current.
    Sales { rows: Vec<Vec<LineRead>>, counter: Option<LineRead>, ambiguous: bool },
    /// The counter moved while Sold Items was closed.
    Unseen { counter: Option<LineRead> },
    /// A new chat line.
    Chat(LineRead),
}
