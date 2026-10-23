from dataclasses import replace
from datetime import UTC, datetime
from pathlib import Path

import pytest

from maplelens.market.catalog import Catalog
from maplelens.market.sample import SAMPLE_ITEMS
from maplelens.market.snapshot import load_snapshot
from maplelens.overlay import glance, theme
from maplelens.overlay.canvas import Canvas
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


def test_card_scales_with_game_resolution():
    full = render_card(SAMPLE_ITEMS[0], now=NOW)
    small = render_card(SAMPLE_ITEMS[0], now=NOW, ui_scale=0.5)
    assert small.width == round(full.width / 2)


def test_compare_card_is_horizontal():
    item = SAMPLE_ITEMS[0]
    normal, wide = render_card(item, now=NOW), render_card(item, now=NOW, wide=True)
    assert wide.width > normal.width * 1.5 and wide.height < normal.height * 0.75
    # nothing to expand on a glance card: it stays the compact one
    assert render_card(item, now=NOW, expanded=False, wide=True).width == normal.width


def test_ask_marker_shows_on_glance_card_and_adds_a_row():
    item = SAMPLE_ITEMS[0]
    with_ask = render_card(item, now=NOW, expanded=False)
    without = render_card(replace(item, ask_median=None), now=NOW, expanded=False)
    assert with_ask.height > without.height


def test_ask_marker_label_flips_left_at_the_right_edge():
    cv = Canvas(CARD_WIDTH, 80)
    glance._ask_marker(cv, CARD_WIDTH - glance.PAD, 40, 900)
    left, _, right, _ = cv.image.getchannel("A").getbbox()
    assert right <= CARD_WIDTH - glance.PAD + 4 and left < CARD_WIDTH - 60   # label sits left of the arrow
