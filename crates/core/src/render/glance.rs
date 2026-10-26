//! The glance card: what matters in the second you hover an item. Each part takes (canvas, y, item) and
//! returns the y below itself.
//!
//! Reading order, top to bottom: which item/category -> the price (big), with where it is heading on the
//! right -> where it sits in its range (`range_bar`) -> quick signals (how fast it sells, how many sales, how
//! recent).

use std::sync::OnceLock;

use super::canvas::{Anchor, Canvas, Ink};
use super::theme;
use crate::image::decode_png_rgba;
use crate::market::format::{format_ago, format_mesos, group_thousands};
use crate::market::insights::{self, Pace};
use crate::market::models::Item;

pub const PAD: f32 = 12.0;
const SPARK_W: f32 = 84.0;
const SPARK_H: f32 = 24.0;
/// Under half a percent reads as noise, so it shows as "steady".
const FLAT_TREND: f64 = 0.005;

fn coin() -> &'static (usize, usize, Vec<u8>) {
    static COIN: OnceLock<(usize, usize, Vec<u8>)> = OnceLock::new();
    COIN.get_or_init(|| decode_png_rgba(include_bytes!("../../assets/meso.png")).unwrap_or((1, 1, vec![0; 4])))
}

pub fn header(cv: &mut Canvas<'_>, y: f32, item: Option<&Item>) -> f32 {
    cv.circle(PAD + 3.5, y + 9.5, 3.5, theme::BULLET, None);
    cv.text(PAD + 13.0, y + 2.0, "MesoWatch", Ink::new(11.0, theme::TEXT_DIM).bold(), Anchor::LA);
    let Some(item) = item else { return y + 26.0 };
    let mut right = cv.width - PAD;
    if item.low_data {
        right -= cv.chip(right, y, "LOW DATA", Ink::new(10.0, theme::WARN).bold(), true, 16.0) + 4.0;
    }
    if let Some(category) = &item.category {
        cv.chip(right, y, category, Ink::new(10.0, theme::TEXT_DIM), true, 16.0);
    }
    y + 26.0
}

/// The price, large, on the left; the trend pill over a sparkline of the week on the right, so a long price
/// never runs into them.
pub fn hero(cv: &mut Canvas<'_>, y: f32, item: &Item) -> f32 {
    // "~": few sales, treat as rough.
    let text = format!("{}{}", if item.low_data { "~" } else { "" }, format_mesos(item.price));
    cv.text(PAD, y, &text, Ink::new(30.0, theme::GOLD).bold(), Anchor::LA);
    let (w, h, rgba) = coin();
    let coin_x = PAD + cv.measure(&text, 30.0, true) + 6.0;
    cv.image(coin_x, y + 10.0, *w as f32, *h as f32, (*w, *h, rgba));
    let mut basis = if item.price_basis.as_deref() == Some("24h") {
        "median of the last 24 h"
    } else {
        "median of the last 10 sales"
    }
    .to_string();
    if item.unit_basis == "set" {
        basis.push_str(" · per set");
    }
    cv.text(PAD, y + 40.0, &basis, Ink::new(11.0, theme::TEXT_DIM), Anchor::LA);
    let right = cv.width - PAD;
    match item.trend {
        Some(trend) if trend.abs() >= FLAT_TREND => {
            let colour = if trend > 0.0 { theme::UP } else { theme::DOWN };
            let label = format!("{:.1}%", trend.abs() * 100.0);
            let x = right - cv.pill_width(&label, 11.0, true);
            cv.pill(x, y + 3.0, &label, colour, Some(trend > 0.0));
        }
        Some(_) => {
            cv.chip(right, y + 5.0, "steady", Ink::new(10.0, theme::TEXT_DIM), true, 16.0);
        }
        None => {}
    }
    sparkline(cv, right - SPARK_W, y + 27.0, item);
    y + 60.0
}

fn sparkline(cv: &mut Canvas<'_>, x: f32, y: f32, item: &Item) {
    let values = item.medians();
    if values.len() < 2 {
        return;
    }
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = if high - low == 0.0 { 1.0 } else { high - low };
    let last = (values.len() - 1) as f32;
    let points: Vec<(f32, f32)> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            (
                x + (SPARK_W - 8.0) * i as f32 / last + 4.0,
                y + (SPARK_H - 6.0 - (SPARK_H - 12.0) * ((v - low) / span) as f32) + 2.0,
            )
        })
        .collect();
    let mut area = points.clone();
    area.push((points[points.len() - 1].0, y + SPARK_H));
    area.push((points[0].0, y + SPARK_H));
    cv.polygon(&area, theme::with_alpha(theme::GOLD_SOFT, 40));
    cv.polyline(&points, theme::GOLD, 2.0);
    let (ex, ey) = points[points.len() - 1];
    cv.circle(ex, ey, 3.5, theme::GOLD, Some((theme::TEXT, 1.0)));
}

/// Quick facts as chips: how it sells, how many sales ever, how recent the last one is.
pub fn signals(cv: &mut Canvas<'_>, y: f32, item: &Item, now: f64) -> f32 {
    let mut parts: Vec<(String, theme::Rgba)> = Vec::new();
    match insights::market_pace(item) {
        Some(Pace::Fast) => parts.push(("Fast seller".into(), theme::UP)),
        Some(Pace::Slow) => parts.push(("Slow seller".into(), theme::WARN)),
        None => {}
    }
    if item.sold != 0 {
        parts.push((format!("{} sold", group_thousands(item.sold)), theme::TEXT_DIM));
    }
    if item.last_sold.is_some() {
        parts.push((format!("last sale {}", format_ago(item.last_sold.as_deref(), now)), theme::TEXT_DIM));
    }
    let mut x = PAD;
    for (text, colour) in &parts {
        x += cv.chip(x, y, text, Ink::new(11.0, *colour), false, 20.0) + 5.0;
    }
    if parts.is_empty() { y } else { y + 24.0 }
}
