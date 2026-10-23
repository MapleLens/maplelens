"""Numbers derived from an item's market data that make the raw figures easier to act on (pure)."""

import math

from maplelens.market.models import Craft, Item

ASK_PREMIUM_NOTABLE = 0.2     # shops listing 20%+ above sales turns the range bar's "in shops" marker amber
FAST_SELL_THROUGH = 0.5       # half of all listings sell
SLOW_SELL_THROUGH = 0.15


def ask_premium(item: Item) -> float | None:
    """How far current asking prices sit above recent sale prices (0.5 = asks are 50% higher)."""
    if not item.price or not item.ask_median:
        return None
    return item.ask_median / item.price - 1


def range_position(value: float | None, low: float | None, high: float | None) -> float | None:
    """Where `value` sits between `low` and `high` on a log scale (0..1). Prices span orders of magnitude,
    so a linear bar would pin every typical price to the left edge."""
    if not value or not low or not high or low <= 0 or high <= low:
        return None
    return min(max(math.log(value / low) / math.log(high / low), 0.0), 1.0)


def market_pace(item: Item) -> str | None:
    """'fast' / 'slow' from the share of listings that actually sell; None when unknown or average."""
    if item.sell_through is None or item.low_data:
        return None
    if item.sell_through >= FAST_SELL_THROUGH:
        return "fast"
    return "slow" if item.sell_through < SLOW_SELL_THROUGH else None


def npc_floor_share(item: Item) -> float | None:
    """NPC sell value as a share of the market price (how little of the price is guaranteed)."""
    if not item.npc or not item.npc.sell or not item.price:
        return None
    return item.npc.sell / item.price


def craft_margin(item: Item, craft: Craft | None = None) -> float | None:
    """Profit per item from crafting it and selling at the typical price (-0.4 = lose 40%)."""
    craft = craft or item.craft
    if not craft or not craft.per_item or not item.price:
        return None
    return item.price / craft.per_item - 1


def volume_bars(item: Item) -> list[float]:
    """Daily sales counts scaled 0..1 for the chart's volume bars."""
    peak = max((day.sold for day in item.history), default=0)
    return [day.sold / peak for day in item.history] if peak else []
