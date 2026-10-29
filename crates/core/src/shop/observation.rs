//! One reading of the shop window, and when two readings agree.

use super::lines::{self, Line};
use super::soldlist::{self, Row};
use crate::image::RgbImage;

/// Mean abs pixel difference below which the meso counter reads the same.
const BALANCE_SAME: f64 = 2.0;

/// One reading of the shop. Each part only counts once two consecutive frames agree on it, which filters the
/// game's mouse pointer sweeping over a row and mid-redraw frames. Parts are judged separately: a busy chat must
/// not delay a sale.
#[derive(Clone, Debug)]
pub struct Observation {
    pub sold: Option<Vec<Row>>,
    /// The list shows its newest end.
    pub sold_live: bool,
    pub balance: Option<RgbImage>,
    /// Where the meso counter is (reference pixels), to read it from the unscaled frame.
    pub balance_area: Option<super::layout::Area>,
    pub chat: Option<Vec<Line>>,
    /// The log shows its newest end.
    pub chat_live: bool,
}

impl Observation {
    pub(super) fn sold_agrees(&self, other: &Self) -> bool {
        self.sold_live == other.sold_live
            && lists_agree(&self.sold, &other.sold, soldlist::same)
            && balance_same(&self.balance, &other.balance)
    }

    pub(super) fn chat_agrees(&self, other: &Self) -> bool {
        self.chat_live == other.chat_live && lists_agree(&self.chat, &other.chat, lines::same)
    }

    /// What was seen, for a log line: "sold 4 rows, chat 14 lines" ("covered" or "scrolled back" where so).
    pub(super) fn describe(&self) -> String {
        let part = |name: &str, count: Option<usize>, unit: &str, live: bool| match count {
            None => format!("{name} not read (closed or covered)"),
            Some(n) if live => format!("{name} {n} {unit}"),
            Some(n) => format!("{name} {n} {unit} scrolled back"),
        };
        let sold = part(
            "sold",
            self.sold.as_ref().map(|rows| rows.iter().filter(|r| !r.is_total).count()),
            "rows",
            self.sold_live,
        );
        format!("{sold}, {}", part("chat", self.chat.as_ref().map(Vec::len), "lines", self.chat_live))
    }
}

fn lists_agree<T>(a: &Option<Vec<T>>, b: &Option<Vec<T>>, same: fn(&T, &T) -> bool) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y)),
        (a, b) => a.is_none() && b.is_none(),
    }
}

pub(super) fn balance_same(a: &Option<RgbImage>, b: &Option<RgbImage>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            let diff: u64 = a.data.iter().zip(&b.data).map(|(&x, &y)| x.abs_diff(y) as u64).sum();
            (a.width, a.height) == (b.width, b.height) && (diff as f64) < BALANCE_SAME * a.data.len().max(1) as f64
        }
        (a, b) => a.is_none() && b.is_none(),
    }
}
