//! The details part of the card, shown once you keep the pointer on an item for a moment.

use super::canvas::{Anchor, Canvas, Ink};
use super::glance::PAD;
use super::theme::{self, Rgba};
use crate::market::format::{format_mesos, group_thousands};
use crate::market::insights::{self, Pace};
use crate::market::models::Item;
use crate::market::time::{date_from_days, days_from_date, month_name, parse_iso, weekday_name};

const THREE_COLUMNS_MIN_W: f32 = 280.0;

/// Daily median with its low-high band, and sales volume as bars underneath.
pub fn week_chart(cv: &mut Canvas<'_>, y: f32, item: &Item) -> f32 {
    let days = &item.history;
    if days.len() < 2 {
        return y;
    }
    let (width, height) = (cv.width - 2.0 * PAD, 58.0);
    cv.text(PAD, y, &format!("LAST {} DAYS", days.len()), Ink::new(10.0, theme::TEXT_FAINT), Anchor::LA);
    let sold: i64 = days.iter().map(|d| d.sold).sum();
    let right = cv.width - PAD;
    cv.text(right, y, &format!("{} sold", group_thousands(sold)), Ink::new(10.0, theme::TEXT_DIM), Anchor::RA);
    let top = y + 15.0;
    let low = days.iter().map(|d| d.low).fold(f64::INFINITY, f64::min);
    let high = days.iter().map(|d| d.high).fold(f64::NEG_INFINITY, f64::max);
    let span = if high - low == 0.0 { 1.0 } else { high - low };
    let (volume, bar_room) = (insights::volume_bars(item), 16.0);
    let last = (days.len() - 1) as f32;
    let xs = |i: usize| PAD + (width - 16.0) * i as f32 / last + 8.0;
    let ys = |v: f64| top + (height - bar_room - 8.0 - (height - bar_room - 16.0) * ((v - low) / span) as f32) + 4.0;

    cv.rounded((PAD, top, width, height), 4.0, Some(theme::TRACK), None);
    for (i, share) in volume.iter().enumerate() {
        let bar = *share as f32 * (bar_room - 2.0);
        cv.rect(xs(i) - 4.0, top + height - 2.0 - bar, 8.0, bar, theme::VOLUME);
    }
    let mut band: Vec<(f32, f32)> = days.iter().enumerate().map(|(i, d)| (xs(i), ys(d.high))).collect();
    band.extend(days.iter().enumerate().rev().map(|(i, d)| (xs(i), ys(d.low))));
    cv.polygon(&band, theme::GOLD_SOFT);
    let line: Vec<(f32, f32)> = days.iter().enumerate().map(|(i, d)| (xs(i), ys(d.median))).collect();
    cv.polyline(&line, theme::GOLD, 2.0);
    for &(px, py) in &line {
        cv.circle(px, py, 2.0, theme::GOLD, None);
    }
    let (ex, ey) = line[line.len() - 1];
    cv.circle(ex, ey, 4.0, theme::GOLD, Some((theme::TEXT, 1.0)));
    for (i, day) in days.iter().enumerate() {
        // Weekday initials under the points.
        let initial = days_from_date(&day.date).map_or("", |d| &weekday_name(d)[..1]);
        cv.text(xs(i), top + height + 2.0, initial, Ink::new(10.0, theme::TEXT_FAINT), Anchor::MA);
    }
    top + height + 20.0
}

/// "Oct 7": the month and day of an ISO timestamp.
fn month_day(iso: &str) -> Option<String> {
    let days = (parse_iso(iso)? / 86400.0).floor() as i64;
    let (_, month, day) = date_from_days(days);
    Some(format!("{} {day}", month_name(month)))
}

/// Market depth: how many are for sale, how it sells, how much has changed hands.
pub fn stat_grid(cv: &mut Canvas<'_>, y: f32, item: &Item) -> f32 {
    let pace_colour = match insights::market_pace(item) {
        Some(Pace::Fast) => theme::UP,
        Some(Pace::Slow) => theme::WARN,
        None => theme::TEXT,
    };
    let nonzero = |v: Option<f64>| v.filter(|&x| x != 0.0);
    // What is on offer first: the shop price is the one you can act on.
    let cells: Vec<(&str, String, Rgba)> = [
        ("FOR SALE NOW", (item.asks != 0).then(|| group_thousands(item.asks)), theme::TEXT),
        ("SHOP PRICE", nonzero(item.ask_median).map(|v| format_mesos(Some(v))), theme::TEXT),
        ("SELL-THROUGH", item.sell_through.map(|s| format!("{:.0}%", s * 100.0)), pace_colour),
        ("TOP SALE", nonzero(item.top_sale).map(|v| format_mesos(Some(v))), theme::TEXT),
        ("TRADED", nonzero(Some(item.mesos_traded)).map(|v| format_mesos(Some(v))), theme::TEXT),
        ("SELLING SINCE", item.first_sold.as_deref().and_then(month_day), theme::TEXT),
    ]
    .into_iter()
    .filter_map(|(label, value, colour)| value.filter(|v| !v.is_empty()).map(|v| (label, v, colour)))
    .collect();
    // The longest labels need about 85 px.
    let columns = if cv.width >= THREE_COLUMNS_MIN_W { 3 } else { 2 };
    let column = (cv.width - 2.0 * PAD) / columns as f32;
    for (n, (label, value, colour)) in cells.iter().enumerate() {
        let (x, top) = (PAD + column * (n % columns) as f32, y + (n / columns) as f32 * 34.0);
        cv.text(x, top, label, Ink::new(10.0, theme::TEXT_FAINT), Anchor::LA);
        cv.text(x, top + 12.0, value, Ink::new(13.0, *colour).bold(), Anchor::LA);
    }
    y + cells.len().div_ceil(columns) as f32 * 34.0
}

/// Who it is for and how it is sold: classes, cash-shop price, scrolled sales.
pub fn tags(cv: &mut Canvas<'_>, y: f32, item: &Item) -> f32 {
    let mut chips: Vec<(String, Rgba)> = Vec::new();
    if !item.classes.is_empty() && item.classes.len() < 4 {
        chips.push((item.classes.join(" / "), theme::ORANGE));
    }
    if let Some(level) = item.level.filter(|&l| l != 0) {
        chips.push((format!("Lv.{level}"), theme::TEXT_DIM));
    }
    if let Some(cash) = &item.cash {
        let label = match cash.nx.filter(|&n| n != 0) {
            Some(nx) => format!("{} NX", group_thousands(nx)),
            None => "Cash Shop".into(),
        };
        chips.push((label, theme::TEXT_DIM));
    }
    if item.scrolled != 0 {
        chips.push((format!("{} scrolled sales", item.scrolled), theme::TEXT_DIM));
    }
    let (mut x, mut row) = (PAD, 0.0);
    for (text, colour) in &chips {
        let w = cv.measure(text, 11.0, false) + 14.0;
        if x + w > cv.width - PAD {
            x = PAD;
            row += 1.0;
        }
        cv.chip(x, y + row * 24.0, text, Ink::new(11.0, *colour), false, 20.0);
        x += w + 5.0;
    }
    if chips.is_empty() { y } else { y + (row + 1.0) * 24.0 + 2.0 }
}
