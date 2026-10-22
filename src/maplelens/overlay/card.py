"""Composes the price card for an item as one RGBA image (no windowing here, so it is testable).

Two sizes, one design language:
- **glance** (default): price, trend, week sparkline, where the price sits in its range, quick signals.
- **full** (after a moment on the same item): everything in glance plus the week chart, market depth,
  tags and sources.
"""

from datetime import UTC, datetime

from PIL import Image

from maplelens.market.format import format_ago
from maplelens.market.models import Item
from maplelens.overlay import details, glance, theme
from maplelens.overlay.canvas import Canvas

CARD_WIDTH = 300   # design width at 1x; the rendered card is CARD_WIDTH * theme.CARD_SCALE
_MAX_HEIGHT = 700


def render_card(item: Item | None, title: str = "", generated_at: str | None = None, now: datetime | None = None,
                *, expanded: bool = True) -> Image.Image:
    """The card for `item`, or a small notice when the item has no market data."""
    now = now or datetime.now(UTC)
    body = Canvas(CARD_WIDTH, _MAX_HEIGHT)
    y = glance.header(body, 8, item)
    if item is None:
        y = _notice(body, y, f"No price data for {title or 'this item'}", "Not in the MesoWatch list yet.")
    elif item.price is None:
        y = _notice(body, y, "No sales seen yet", "Nobody has been seen selling it.")
        if expanded:
            y = details.tags(body, y, item)
            y = details.sources(body, y, item)
    else:
        y = glance.hero(body, y, item)
        y = glance.range_bar(body, y, item, show_listing=expanded)
        y = glance.signals(body, y + 4, item, now)
        if expanded:
            body.divider(y + 2, glance.PAD)
            y = details.week_chart(body, y + 12, item)
            y = details.stat_grid(body, y, item)
            y = details.tags(body, y + 2, item)
            y = details.sources(body, y, item)
    age = format_ago(generated_at, now).removesuffix(" ago") if generated_at else None
    height = _footer(body, y + 2, age) + 2

    card = Canvas(CARD_WIDTH, height)
    card.frame(height)
    card.image.alpha_composite(body.image.crop((0, 0, CARD_WIDTH, height)))
    scaled = (round(CARD_WIDTH * theme.CARD_SCALE), round(height * theme.CARD_SCALE))
    return card.image.resize(scaled, Image.BICUBIC)   # soft like the game's own, itself upscaled, text


def _notice(cv: Canvas, y: int, headline: str, hint: str) -> int:
    cv.text(glance.PAD, y + 2, headline, 13, theme.TEXT, bold=True)
    cv.text(glance.PAD, y + 22, hint, 11, theme.TEXT_DIM)
    return y + 46


def _footer(cv: Canvas, y: int, data_age: str | None) -> int:
    text = f"meso.watch · data {data_age} old" if data_age else "meso.watch"
    cv.text(glance.PAD, y + 4, text, 10, theme.TEXT_FAINT)
    cv.text(cv.width - glance.PAD, y + 4, "MapleLens", 10, theme.TEXT_FAINT, anchor="ra")
    return y + 20
