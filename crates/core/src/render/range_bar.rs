//! The price range bar: where the typical price sits among every sale seen, and where shops list it now.
//!
//! ```text
//!     ▾ in shops 118                    <- median shop listing: blue (shops, not sales); amber when well above
//!                                          what it actually sells for
//!     ━━━━━━━|███━━━━━━━━━━━━━━━━━━     <- groove = lowest..highest sale, gold = middle half, white = typical
//!     40   usually 99 – 119       900   <- the band's label sits under the band, not mid-card
//! ```
//!
//! Everything is on the same log scale (`insights::range_position`), so the marks can be compared by eye.

use super::canvas::{Anchor, Canvas, Ink};
use super::theme::{self, Rgba};
use crate::market::format::format_mesos;
use crate::market::insights;
use crate::market::models::Item;

pub const PAD: f32 = 12.0;
const BAR_H: f32 = 8.0;
/// Room above the bar for the listing marker.
const ASK_ROW: f32 = 14.0;
/// Keeps the band label clear of the low/high numbers at the ends.
const LABEL_GAP: f32 = 16.0;
const SIZE: f32 = 10.0;

/// Draw the bar block at `y` and return the y below it (unchanged when the item has no range).
pub fn range_bar(cv: &mut Canvas<'_>, y: f32, item: &Item) -> f32 {
    let at = |value: Option<f64>| insights::range_position(value, item.low, item.high).map(|v| v as f32);
    let Some(mid) = at(item.price) else { return y };
    let listing = at(item.ask_median);
    let width = cv.width - 2.0 * PAD;
    let top = y + 2.0 + if listing.is_some() { ASK_ROW } else { 0.0 };
    let band = (at(item.p25), at(item.p75));
    track(cv, top, width, band);
    let tick = PAD + mid * width;
    cv.rect(tick - 1.0, top - 3.0, 3.0, BAR_H + 6.0, theme::TEXT);
    if let (Some(listing), Some(ask)) = (listing, item.ask_median) {
        listing_marker(cv, PAD + listing * width, top, ask, listing_colour(item));
    }
    scale_labels(cv, top + BAR_H + 6.0, width, item, band);
    top + 34.0
}

fn track(cv: &mut Canvas<'_>, top: f32, width: f32, band: (Option<f32>, Option<f32>)) {
    cv.rounded((PAD, top, width, BAR_H), 4.0, Some(theme::TRACK), None);
    if let (Some(lo), Some(hi)) = band {
        // A minimum width so a tight band still reads as a band, not a second tick.
        let x0 = PAD + lo * width;
        let x1 = (PAD + hi * width).max(x0 + 5.0);
        cv.rounded((x0, top, x1 - x0, BAR_H), 4.0, Some(theme::with_alpha(theme::GOLD_SOFT, 200)), None);
    }
}

/// Blue normally; amber when shops ask well above what it sells for.
pub fn listing_colour(item: &Item) -> Rgba {
    match insights::ask_premium(item) {
        Some(premium) if premium >= insights::ASK_PREMIUM_NOTABLE => theme::WARN,
        _ => theme::BULLET,
    }
}

/// Down-arrow pointing into the bar at `x`, labelled on whichever side has room.
pub fn listing_marker(cv: &mut Canvas<'_>, x: f32, top: f32, ask: f64, colour: Rgba) {
    cv.arrow((x - 3.5).round(), top - 11.0, false, colour, 7.0);
    let label = format!("in shops {}", format_mesos(Some(ask)));
    if x + 8.0 + cv.measure(&label, SIZE, false) <= cv.width - PAD {
        cv.text(x + 8.0, top - 15.0, &label, Ink::new(SIZE, colour), Anchor::LA);
    } else {
        cv.text(x - 8.0, top - 15.0, &label, Ink::new(SIZE, colour), Anchor::RA);
    }
}

/// Lowest and highest sale at the ends; "usually p25 – p75" centred under the gold band where it fits.
fn scale_labels(cv: &mut Canvas<'_>, y: f32, width: f32, item: &Item, band: (Option<f32>, Option<f32>)) {
    let (low, high) = (format_mesos(item.low), format_mesos(item.high));
    cv.text(PAD, y, &low, Ink::new(SIZE, theme::TEXT_FAINT), Anchor::LA);
    let right_edge = cv.width - PAD;
    cv.text(right_edge, y, &high, Ink::new(SIZE, theme::TEXT_FAINT), Anchor::RA);
    let (Some(lo), Some(hi)) = band else { return };
    let (Some(p25), Some(p75)) = (item.p25.filter(|&v| v != 0.0), item.p75.filter(|&v| v != 0.0)) else { return };
    let text = format!("usually {} – {}", format_mesos(Some(p25)), format_mesos(Some(p75)));
    let half = cv.measure(&text, SIZE, false) / 2.0;
    let left = PAD + cv.measure(&low, SIZE, false) + LABEL_GAP + half;
    let right = cv.width - PAD - cv.measure(&high, SIZE, false) - LABEL_GAP - half;
    let centre = PAD + (lo + hi) / 2.0 * width;
    cv.text(centre.max(left).min(right), y, &text, Ink::new(SIZE, theme::TEXT), Anchor::MA);
}
