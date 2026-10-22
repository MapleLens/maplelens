from datetime import UTC, datetime
from pathlib import Path

import pytest

from maplelens.market.catalog import Catalog
from maplelens.market.sample import SAMPLE_ITEMS
from maplelens.market.snapshot import load_snapshot
from maplelens.overlay import theme
from maplelens.overlay.card import CARD_WIDTH, render_card

NOW = datetime(2026, 10, 22, 0, 0, tzinfo=UTC)
SNAPSHOT = Path(__file__).parent.parent / "data" / "list.json"


def test_card_is_translucent_like_the_game_tooltip():
    card = render_card(SAMPLE_ITEMS[0], generated_at="2026-10-21T23:45:49+00:00", now=NOW)
    assert card.width == round(CARD_WIDTH * theme.CARD_SCALE) and 300 < card.height < 900
    # a spot in the panel with no content: the fill, as see-through as the game's box (not opaque, not clear)
    assert card.getpixel((card.width - 30, 40))[3] == theme.PANEL_FILL[3]
    assert card.getpixel((card.width // 2, card.height // 2))[3] > 100


def test_unknown_item_gets_a_small_notice():
    unknown, known = render_card(None, "Mystery Item", now=NOW), render_card(SAMPLE_ITEMS[0], now=NOW)
    assert unknown.height < known.height / 2


def test_item_without_sales_still_renders():
    from maplelens.market.models import Item, NpcShop

    card = render_card(Item(name="Unsold", category="Etc", npc=NpcShop(None, 5, None, 0)), now=NOW)
    assert card.height > 60


@pytest.mark.skipif(not SNAPSHOT.exists(), reason="needs a data/list.json you are permitted to use")
def test_every_kind_of_real_item_renders():
    items = load_snapshot(SNAPSHOT).items
    for item in items[::7]:   # a spread of categories, with and without craft / npc / cash data
        card = render_card(item, now=NOW)
        assert card.width == round(CARD_WIDTH * theme.CARD_SCALE) and card.height < 1000, item.name
    assert Catalog(items).lookup("Iron Ore") is not None


def test_glance_card_is_a_compact_version_of_the_full_card():
    item = SAMPLE_ITEMS[0]
    glance, full = render_card(item, now=NOW, expanded=False), render_card(item, now=NOW, expanded=True)
    assert glance.width == full.width
    assert glance.height < full.height * 0.65   # the glance card is what you see first, so it must stay small
