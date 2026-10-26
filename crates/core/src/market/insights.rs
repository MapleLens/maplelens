//! Numbers derived from an item's market data that make the raw figures easier to act on (pure).

use super::models::{Craft, Item};

/// Shops listing 20%+ above sales turns the range bar's "in shops" marker amber.
pub const ASK_PREMIUM_NOTABLE: f64 = 0.2;
/// Half of all listings sell.
pub const FAST_SELL_THROUGH: f64 = 0.5;
pub const SLOW_SELL_THROUGH: f64 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Fast,
    Slow,
}

fn positive(v: Option<f64>) -> Option<f64> {
    v.filter(|&x| x != 0.0)
}

/// How far current asking prices sit above recent sale prices (0.5 = asks are 50% higher).
pub fn ask_premium(item: &Item) -> Option<f64> {
    Some(positive(item.ask_median)? / positive(item.price)? - 1.0)
}

/// Where `value` sits between `low` and `high` on a log scale (0..1). Prices span orders of magnitude, so a
/// linear bar would pin every typical price to the left edge.
pub fn range_position(value: Option<f64>, low: Option<f64>, high: Option<f64>) -> Option<f64> {
    let (value, low, high) = (positive(value)?, positive(low)?, positive(high)?);
    if low <= 0.0 || high <= low {
        return None;
    }
    Some(((value / low).ln() / (high / low).ln()).clamp(0.0, 1.0))
}

/// Fast / slow from the share of listings that actually sell; None when unknown or average.
pub fn market_pace(item: &Item) -> Option<Pace> {
    let sell_through = item.sell_through?;
    if item.low_data {
        return None;
    }
    if sell_through >= FAST_SELL_THROUGH {
        Some(Pace::Fast)
    } else if sell_through < SLOW_SELL_THROUGH {
        Some(Pace::Slow)
    } else {
        None
    }
}

/// NPC sell value as a share of the market price (how little of the price is guaranteed).
pub fn npc_floor_share(item: &Item) -> Option<f64> {
    Some(positive(item.npc.as_ref()?.sell)? / positive(item.price)?)
}

/// Profit per item from crafting it and selling at the typical price (-0.4 = lose 40%).
pub fn craft_margin(item: &Item, craft: Option<&Craft>) -> Option<f64> {
    let craft = craft.or(item.craft.as_ref())?;
    Some(positive(item.price)? / positive(craft.per_item)? - 1.0)
}

/// Daily sales counts scaled 0..1 for the chart's volume bars.
pub fn volume_bars(item: &Item) -> Vec<f64> {
    let peak = item.history.iter().map(|d| d.sold).max().unwrap_or(0);
    if peak == 0 { Vec::new() } else { item.history.iter().map(|d| d.sold as f64 / peak as f64).collect() }
}
