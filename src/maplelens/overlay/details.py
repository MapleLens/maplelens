"""The details part of the card, shown once you keep the pointer on an item for a moment."""

from datetime import datetime

from maplelens.market import insights
from maplelens.market.format import format_mesos
from maplelens.market.models import Item
from maplelens.overlay import theme
from maplelens.overlay.canvas import Canvas
from maplelens.overlay.glance import PAD

_THREE_COLUMNS_MIN_W = 280


def week_chart(cv: Canvas, y: int, item: Item) -> int:
    """Daily median with its low-high band, and sales volume as bars underneath."""
    days = item.history
    if len(days) < 2:
        return y
    width, height = cv.width - 2 * PAD, 54
    cv.text(PAD, y, f"LAST {len(days)} DAYS", 10, theme.TEXT_FAINT)
    cv.text(cv.width - PAD, y, f"{sum(d.sold for d in days):,} sold", 10, theme.TEXT_DIM, anchor="ra")
    top = y + 15
    low, high = min(d.low for d in days), max(d.high for d in days)
    span = (high - low) or 1
    volume, bar_room = insights.volume_bars(item), 12

    def xs(i: int, s: int) -> float:
        return (width - 16) * i / (len(days) - 1) * s + 8 * s

    def ys(v: float, s: int) -> float:
        return (height - bar_room - 8 - (height - bar_room - 16) * (v - low) / span) * s + 4 * s

    def chart(d, s):
        d.rounded_rectangle((0, 0, width * s - 1, height * s - 1), radius=4 * s, fill=theme.TRACK)
        for i, share in enumerate(volume):
            x = xs(i, s)
            d.rectangle((x - 4 * s, (height - 2) * s - share * (bar_room - 2) * s, x + 4 * s, (height - 2) * s),
                        fill=theme.VOLUME)
        band = [(xs(i, s), ys(day.high, s)) for i, day in enumerate(days)]
        band += [(xs(i, s), ys(day.low, s)) for i, day in reversed(list(enumerate(days)))]
        d.polygon(band, fill=theme.GOLD_SOFT)
        line = [(xs(i, s), ys(day.median, s)) for i, day in enumerate(days)]
        d.line(line, fill=theme.GOLD, width=2 * s, joint="curve")
        for px, py in line:
            d.ellipse((px - 2 * s, py - 2 * s, px + 2 * s, py + 2 * s), fill=theme.GOLD)
        ex, ey = line[-1]
        d.ellipse((ex - 4 * s, ey - 4 * s, ex + 4 * s, ey + 4 * s), fill=theme.GOLD, outline=theme.TEXT, width=s)

    cv.overlay((PAD, top, width, height), chart, scale=3)
    for i, day in enumerate(days):   # weekday initials
        cv.text(PAD + 8 + (width - 16) * i / (len(days) - 1), top + height + 2,
                datetime.fromisoformat(day.date).strftime("%a")[0], 10, theme.TEXT_FAINT, anchor="ma")
    return top + height + 20


def stat_grid(cv: Canvas, y: int, item: Item) -> int:
    """Market depth: how many are for sale, how it sells, how much has changed hands."""
    first = (datetime.fromisoformat(item.first_sold.replace("Z", "+00:00")).strftime("%b %d").replace(" 0", " ")
             if item.first_sold else None)
    pace = insights.market_pace(item)
    pace_colour = theme.UP if pace == "fast" else theme.WARN if pace == "slow" else theme.TEXT
    cells: list[tuple[str, str | None, theme.RGBA]] = [
        ("SELL-THROUGH", f"{item.sell_through * 100:.0f}%" if item.sell_through is not None else None, pace_colour),
        ("FOR SALE NOW", f"{item.asks:,}" if item.asks else None, theme.TEXT),
        ("SHOP PRICE", format_mesos(item.ask_median) if item.ask_median else None, theme.TEXT),
        ("TOP SALE", format_mesos(item.top_sale) if item.top_sale else None, theme.TEXT),
        ("TRADED", format_mesos(item.mesos_traded) if item.mesos_traded else None, theme.TEXT),
        ("SELLING SINCE", first, theme.TEXT),
    ]
    cells = [c for c in cells if c[1]]
    columns = 3 if cv.width >= _THREE_COLUMNS_MIN_W else 2   # the longest labels need about 85 px
    column = (cv.width - 2 * PAD) / columns
    for n, (label, value, colour) in enumerate(cells):
        x, top = PAD + column * (n % columns), y + (n // columns) * 34
        cv.text(x, top, label, 10, theme.TEXT_FAINT)
        cv.text(x, top + 12, value, 13, colour, bold=True)
    return y + ((len(cells) + columns - 1) // columns) * 34


def tags(cv: Canvas, y: int, item: Item) -> int:
    """Who it is for and how it is sold: classes, cash-shop price, scrolled sales."""
    chips: list[tuple[str, theme.RGBA]] = []
    if item.classes and len(item.classes) < 4:
        chips.append((" / ".join(item.classes), theme.ORANGE))
    if item.level:
        chips.append((f"Lv.{item.level}", theme.TEXT_DIM))
    if item.cash:
        chips.append((f"{item.cash.nx:,} NX" if item.cash.nx else "Cash Shop", theme.TEXT_DIM))
    if item.scrolled:
        chips.append((f"{item.scrolled} scrolled sales", theme.TEXT_DIM))
    x, row = PAD, 0
    for text, colour in chips:
        w = cv.measure(text, 11) + 14
        if x + w > cv.width - PAD:
            x, row = PAD, row + 1
        cv.chip(x, y + row * 24, text, colour, size=11, height=20)
        x += w + 5
    return y + (row + 1) * 24 + 2 if chips else y


def sources(cv: Canvas, y: int, item: Item) -> int:
    """Where the item comes from and what NPCs or crafters make of it."""
    lines: list[str] = []
    if item.npc and item.npc.sell:
        share = insights.npc_floor_share(item)
        extra = f" ({share * 100:.0f}% of market)" if share and share >= 0.02 else ""
        lines.append(f"NPC buys it for {format_mesos(item.npc.sell)}{extra}")
    if item.npc and item.npc.buy:
        where = f" at {item.npc.shop}" if item.npc.shop else ""
        lines.append(f"NPC sells it for {format_mesos(item.npc.buy)}{where}")
    if item.craft:
        margin = insights.craft_margin(item)
        parts = ", ".join(f"{p.count}× {p.name}" for p in item.craft.parts[:3])
        each = f" · {format_mesos(item.craft.per_item)} each" if item.craft.per_item else ""
        tail = f" → {margin * 100:+.0f}% vs sale" if margin is not None else ""
        level = f" Lv.{item.craft.level}" if item.craft.level else ""
        lines.append(f"Craft ({item.craft.discipline}{level}): {parts}{each}{tail}")
    names = {"mob": "monster drop", "npc": "NPC shop", "craft": "crafting", "quest": "quest reward"}
    if item.obtain:
        lines.append("From: " + ", ".join(names.get(o, o) for o in item.obtain))
    for text in lines:
        for n, wrapped in enumerate(cv.wrap(text, 11, cv.width - 2 * PAD - 12)):
            if n == 0:
                cv.draw.rectangle((PAD, y + 6, PAD + 3, y + 9), fill=theme.GOLD)
            cv.text(PAD + 10, y, wrapped, 11, theme.TEXT_DIM)
            y += 16
        y += 2
    return y
