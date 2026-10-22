import pytest

from maplelens.market import insights
from maplelens.market.models import Craft, CraftPart, Day, Item, NpcShop


def item(**kw) -> Item:
    return Item(name="X", **kw)


def test_ask_premium():
    assert insights.ask_premium(item(price=100, ask_median=150)) == pytest.approx(0.5)
    assert insights.ask_premium(item(price=None, ask_median=150)) is None


def test_range_position_is_log_scaled_and_clamped():
    assert insights.range_position(100, 10, 1000) == pytest.approx(0.5)
    assert insights.range_position(1, 10, 1000) == 0.0
    assert insights.range_position(5000, 10, 1000) == 1.0
    assert insights.range_position(100, 10, 10) is None


def test_market_pace():
    assert insights.market_pace(item(sell_through=0.6)) == "fast"
    assert insights.market_pace(item(sell_through=0.05)) == "slow"
    assert insights.market_pace(item(sell_through=0.3)) is None
    assert insights.market_pace(item(sell_through=0.6, low_data=True)) is None


def test_npc_floor_share_and_craft_margin():
    assert insights.npc_floor_share(item(price=12000, npc=NpcShop(None, 6000, None, 0))) == pytest.approx(0.5)
    craft = Craft("Woodcrafting", 1, 500, 400, 0.8, (CraftPart("Wood", 1, 299.5),))
    assert insights.craft_margin(item(price=0.5, craft=craft)) == pytest.approx(-0.375)


def test_volume_bars_scale_to_the_busiest_day():
    days = tuple(Day(f"2026-10-{15 + i}", 10, 9, 11, s) for i, s in enumerate((50, 100, 25)))
    assert insights.volume_bars(item(history=days)) == [0.5, 1.0, 0.25]
    assert insights.volume_bars(item()) == []
