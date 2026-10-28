//! Composes the price card for an item as one RGBA image (no windowing here, so it is testable).
//!
//! Two sizes, one design language:
//! - **glance** (default): price, trend, week sparkline, where the price sits in its range, quick signals.
//! - **full** (after a moment on the same item): everything in glance plus the week chart, market depth,
//!   tags and sources, each group under a divider.

use super::canvas::{Anchor, Canvas, Ink};
use super::font::Fonts;
use super::map_card::fit_text;
use super::theme::{self, Rgba};
use super::{RgbaImage, details, glance, origin, range_bar};
use crate::market::format::format_ago;
use crate::market::models::Item;

/// Design width at 1x; the rendered card is CARD_WIDTH * theme::CARD_SCALE * ui scale.
pub const CARD_WIDTH: f32 = 300.0;
/// The horizontal card (equipment compare, where it sits under two tooltips) adds a column for the details;
/// the total is about as wide as two equipment tooltips side by side (743 px at 1.5x).
pub const SIDE_WIDTH: f32 = 200.0;
const MAX_HEIGHT: f32 = 700.0;
/// Room around the divider between groups.
const GROUP_GAP: f32 = 14.0;

/// What a card shows and how.
#[derive(Clone, Copy, Debug)]
pub struct CardSpec<'a> {
    pub item: Option<&'a Item>,
    /// The title as read, for the "no price data" notice.
    pub title: &'a str,
    /// When the price data was exported (ISO-8601), shown as its age.
    pub generated_at: Option<&'a str>,
    /// Unix seconds.
    pub now: f64,
    pub expanded: bool,
    pub ui_scale: f32,
    /// Equipment compare: lay the details out to the side instead of below.
    pub wide: bool,
    /// Credit line on the left of the footer ("meso.watch", or "demo data").
    pub source: &'a str,
}

#[derive(Clone, Copy)]
enum Section {
    Hero,
    RangeBar,
    Signals,
    WeekChart,
    StatGrid,
    Tags,
    Sources,
    Notice(&'static str, &'static str),
}

fn draw(section: Section, cv: &mut Canvas<'_>, y: f32, item: &Item, now: f64) -> f32 {
    match section {
        Section::Hero => glance::hero(cv, y, item),
        Section::RangeBar => range_bar::range_bar(cv, y, item),
        Section::Signals => glance::signals(cv, y, item, now),
        Section::WeekChart => details::week_chart(cv, y, item),
        Section::StatGrid => details::stat_grid(cv, y, item),
        Section::Tags => details::tags(cv, y, item),
        Section::Sources => origin::sources(cv, y, item),
        Section::Notice(headline, hint) => notice(cv, y, headline, hint),
    }
}

/// A card opened where the game does not name the item (a drop in the map drops panel): its name is the card's title,
/// with a line about it in a colour of its own under it, and where that comes from (faint; "" for none).
#[derive(Clone, Copy, Debug)]
pub struct Heading<'a> {
    pub name: &'a str,
    pub note: &'a str,
    pub colour: Rgba,
    pub source: &'a str,
}

/// The card for `spec.item`, or a small notice when the item has no market data.
pub fn render_card(fonts: &Fonts, spec: &CardSpec<'_>) -> RgbaImage {
    compose(fonts, spec, None)
}

/// The same card under the item's name instead of the data's.
pub fn render_named_card(fonts: &Fonts, spec: &CardSpec<'_>, heading: &Heading<'_>) -> RgbaImage {
    compose(fonts, spec, Some(heading))
}

fn compose(fonts: &Fonts, spec: &CardSpec<'_>, heading: Option<&Heading<'_>>) -> RgbaImage {
    let wide = spec.wide && spec.expanded && spec.item.is_some_and(|i| i.price.is_some());
    let width = if wide { CARD_WIDTH + SIDE_WIDTH } else { CARD_WIDTH };
    let factor = theme::CARD_SCALE * spec.ui_scale;
    let mut body = Canvas::new(fonts, width, MAX_HEIGHT, factor);
    body.width = CARD_WIDTH; // sections lay out in the main column
    let mut y = match heading {
        Some(heading) => named_header(&mut body, 8.0, spec.item, heading),
        None => glance::header(&mut body, 8.0, spec.item),
    };
    match spec.item {
        None => {
            let headline =
                format!("No price data for {}", if spec.title.is_empty() { "this item" } else { spec.title });
            y = notice(&mut body, y, &headline, "Not in the MesoWatch list yet.");
        }
        Some(item) => {
            y = stack(&mut body, y, item, &groups(item, spec.expanded, wide), spec.now);
            if wide {
                y = side_column(&mut body, y, item);
            }
        }
    }
    body.width = width;
    let updated = spec.generated_at.map(|g| format_ago(Some(g), spec.now));
    let height = footer(&mut body, y + 2.0, spec.source, updated.as_deref()) + 2.0;

    let mut card = Canvas::new(fonts, width, height, factor);
    card.frame(height);
    card.composite(&body, 0.0, 0.0);
    RgbaImage::from_canvas(&card)
}

fn named_header(cv: &mut Canvas<'_>, y: f32, item: Option<&Item>, heading: &Heading<'_>) -> f32 {
    cv.circle(glance::PAD + 3.5, y + 9.5, 3.5, theme::BULLET, None);
    let left = glance::PAD + 13.0;
    let right = item.map_or(cv.width - glance::PAD, |item| glance::chips(cv, y, item));
    let name = fit_text(cv, heading.name, 13.0, true, right - 6.0 - left);
    cv.text(left, y + 1.0, &name, Ink::new(13.0, theme::TEXT).bold(), Anchor::LA);
    if heading.note.is_empty() {
        return y + 26.0;
    }
    let room = cv.width - glance::PAD - left;
    let note = fit_text(cv, heading.note, 10.0, false, room);
    cv.text(left, y + 21.0, &note, Ink::new(10.0, heading.colour), Anchor::LA);
    if heading.source.is_empty() {
        return y + 40.0;
    }
    let source = fit_text(cv, heading.source, 9.0, false, room);
    cv.text(left, y + 35.0, &source, Ink::new(9.0, theme::TEXT_FAINT), Anchor::LA);
    y + 52.0
}

/// What the main column shows, as groups of sections; a divider separates groups that drew anything.
fn groups(item: &Item, expanded: bool, wide: bool) -> Vec<Vec<Section>> {
    let about = vec![Section::Tags, Section::Sources];
    if item.price.is_none() {
        let mut groups = vec![vec![Section::Notice("No sales seen yet", "Nobody has been seen selling it.")]];
        if expanded {
            groups.push(about);
        }
        return groups;
    }
    let price = vec![Section::Hero, Section::RangeBar, Section::Signals];
    match (expanded, wide) {
        (false, _) => vec![price],
        (true, true) => vec![price, about], // the chart and market depth go to the side column
        (true, false) => vec![price, vec![Section::WeekChart, Section::StatGrid], about],
    }
}

fn stack(cv: &mut Canvas<'_>, mut y: f32, item: &Item, groups: &[Vec<Section>], now: f64) -> f32 {
    let mut drawn = false;
    for group in groups {
        let start = if drawn { y + GROUP_GAP } else { y };
        let mut end = start;
        for &section in group {
            end = draw(section, cv, end, item, now);
        }
        if end == start {
            continue; // nothing to show: no divider either
        }
        if drawn {
            cv.divider(y + (GROUP_GAP / 2.0).floor(), glance::PAD);
        }
        y = end;
        drawn = true;
    }
    y
}

/// The week chart and market depth in a column right of the main one, behind a vertical rule; returns the
/// lower of the two columns' bottoms.
fn side_column(body: &mut Canvas<'_>, left_bottom: f32, item: &Item) -> f32 {
    let mut side = Canvas::new(body.fonts, SIDE_WIDTH, MAX_HEIGHT, body.factor);
    let y = details::week_chart(&mut side, 14.0, item);
    let y = details::stat_grid(&mut side, y, item);
    body.composite(&side, CARD_WIDTH, 0.0);
    let bottom = left_bottom.max(y);
    body.rect(CARD_WIDTH, 12.0, 1.0, bottom - 4.0 - 12.0 + 1.0, theme::DIVIDER);
    bottom
}

fn notice(cv: &mut Canvas<'_>, y: f32, headline: &str, hint: &str) -> f32 {
    cv.text(glance::PAD, y + 2.0, headline, Ink::new(13.0, theme::TEXT).bold(), Anchor::LA);
    cv.text(glance::PAD, y + 22.0, hint, Ink::new(11.0, theme::TEXT_DIM), Anchor::LA);
    y + 40.0
}

fn footer(cv: &mut Canvas<'_>, y: f32, source: &str, updated: Option<&str>) -> f32 {
    cv.divider(y + 2.0, glance::PAD);
    let text = match updated {
        Some(updated) => format!("{source} · updated {updated}"),
        None => source.to_string(),
    };
    cv.text(glance::PAD, y + 8.0, &text, Ink::new(10.0, theme::TEXT_FAINT), Anchor::LA);
    let right = cv.width - glance::PAD;
    cv.text(right, y + 8.0, "MapleLens", Ink::new(10.0, theme::TEXT_FAINT), Anchor::RA);
    y + 24.0
}
