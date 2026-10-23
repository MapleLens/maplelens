"""Composes the price card for an item as one RGBA image (no windowing here, so it is testable).

Two sizes, one design language:
- **glance** (default): price, trend, week sparkline, where the price sits in its range, quick signals.
- **full** (after a moment on the same item): everything in glance plus the week chart, market depth,
  tags and sources, each group under a divider.
"""

from collections.abc import Callable
from datetime import UTC, datetime
from functools import partial

from PIL import Image

from maplelens.market.format import format_ago
from maplelens.market.models import Item
from maplelens.overlay import details, glance, origin, theme
from maplelens.overlay.canvas import Canvas

CARD_WIDTH = 300   # design width at 1x; the rendered card is CARD_WIDTH * theme.CARD_SCALE
# The horizontal card (equipment compare, where it sits under two tooltips) adds a column for the details; the
# total is about as wide as two equipment tooltips side by side (743 px at 1.5x).
SIDE_WIDTH = 200
_MAX_HEIGHT = 700
_GROUP_GAP = 14   # room around the divider between groups

Section = Callable[[Canvas, int, Item], int]
Group = tuple[Section, ...]


def render_card(item: Item | None, title: str = "", generated_at: str | None = None, now: datetime | None = None,
                *, expanded: bool = True, ui_scale: float = 1.0, wide: bool = False) -> Image.Image:
    """The card for `item`, or a small notice when the item has no market data."""
    now = now or datetime.now(UTC)
    wide = wide and expanded and item is not None and item.price is not None
    width = CARD_WIDTH + SIDE_WIDTH if wide else CARD_WIDTH
    body = Canvas(CARD_WIDTH, _MAX_HEIGHT)
    y = glance.header(body, 8, item)
    if item is None:
        y = _notice(f"No price data for {title or 'this item'}", "Not in the MesoWatch list yet.")(body, y, item)
    else:
        y = _stack(body, y, item, _groups(item, now, expanded=expanded, wide=wide))
        if wide:
            body, y = _with_side_column(body, y, item)
    height = _footer(body, y + 2, format_ago(generated_at, now) if generated_at else None) + 2

    card = Canvas(width, height)
    card.frame(height)
    card.image.alpha_composite(body.image.crop((0, 0, width, height)))
    factor = theme.CARD_SCALE * ui_scale
    scaled = (round(width * factor), round(height * factor))
    return card.image.resize(scaled, Image.BICUBIC)   # soft like the game's own, itself upscaled, text


def _groups(item: Item, now: datetime, *, expanded: bool, wide: bool) -> list[Group]:
    """What the main column shows, as groups of sections; a divider separates groups that drew anything."""
    about: Group = (details.tags, origin.sources)
    if item.price is None:
        return [(_notice("No sales seen yet", "Nobody has been seen selling it."),), *([about] if expanded else [])]
    price: Group = (glance.hero, glance.range_bar, partial(glance.signals, now=now))
    if not expanded:
        return [price]
    if wide:   # the chart and market depth go to the side column
        return [price, about]
    return [price, (details.week_chart, details.stat_grid), about]


def _stack(cv: Canvas, y: int, item: Item, groups: list[Group]) -> int:
    drawn = False
    for group in groups:
        start = y + _GROUP_GAP if drawn else y
        end = start
        for section in group:
            end = section(cv, end, item)
        if end == start:   # nothing to show: no divider either
            continue
        if drawn:
            cv.divider(y + _GROUP_GAP // 2, glance.PAD)
        y, drawn = end, True
    return y


def _with_side_column(left: Canvas, left_bottom: int, item: Item) -> tuple[Canvas, int]:
    """`left` plus a column to its right with the week chart and market depth; returns it and the lower bottom."""
    side = Canvas(SIDE_WIDTH, _MAX_HEIGHT)
    y = details.week_chart(side, 14, item)
    y = details.stat_grid(side, y, item)
    body = Canvas(CARD_WIDTH + SIDE_WIDTH, _MAX_HEIGHT)
    body.image.alpha_composite(left.image)
    body.image.alpha_composite(side.image, (CARD_WIDTH, 0))
    bottom = max(left_bottom, y)
    body.draw.line((CARD_WIDTH, 12, CARD_WIDTH, bottom - 4), fill=theme.DIVIDER)
    return body, bottom


def _notice(headline: str, hint: str) -> Section:
    def section(cv: Canvas, y: int, _item: Item | None) -> int:
        cv.text(glance.PAD, y + 2, headline, 13, theme.TEXT, bold=True)
        cv.text(glance.PAD, y + 22, hint, 11, theme.TEXT_DIM)
        return y + 40
    return section


def _footer(cv: Canvas, y: int, updated: str | None) -> int:
    cv.divider(y + 2, glance.PAD)
    text = f"meso.watch · updated {updated}" if updated else "meso.watch"
    cv.text(glance.PAD, y + 8, text, 10, theme.TEXT_FAINT)
    cv.text(cv.width - glance.PAD, y + 8, "MapleLens", 10, theme.TEXT_FAINT, anchor="ra")
    return y + 24
