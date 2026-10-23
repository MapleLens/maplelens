"""Where an item comes from and what NPCs or crafters make of it (a details-card section)."""

from maplelens.market import insights
from maplelens.market.format import format_mesos
from maplelens.market.models import Craft, Item
from maplelens.overlay import theme
from maplelens.overlay.canvas import Canvas
from maplelens.overlay.glance import PAD

# An NPC price this small next to the market (scrolls an NPC buys for 1 meso) says nothing useful.
_NPC_FLOOR_NOTABLE = 0.02
_INDENT = 10
_LINE = 16
_OBTAIN_NAMES = {"mob": "monster drop", "npc": "NPC shop", "craft": "crafting", "quest": "quest reward"}


def sources(cv: Canvas, y: int, item: Item) -> int:
    share = insights.npc_floor_share(item)
    if item.npc and item.npc.sell and (share is None or share >= _NPC_FLOOR_NOTABLE):
        extra = f" ({share * 100:.0f}% of market)" if share else ""
        y = _bullet(cv, y, f"NPC buys it for {format_mesos(item.npc.sell)}{extra}")
    if item.npc and item.npc.buy:
        where = f" at {item.npc.shop}" if item.npc.shop else ""
        y = _bullet(cv, y, f"NPC sells it for {format_mesos(item.npc.buy)}{where}")
    if item.craft:
        y = _craft(cv, y, item, item.craft)
    if item.obtain:
        y = _bullet(cv, y, "From: " + ", ".join(_OBTAIN_NAMES.get(o, o) for o in item.obtain))
    return y


def _craft(cv: Canvas, y: int, item: Item, craft: Craft) -> int:
    """Craft cost on one line with a coloured verdict at its end, the recipe dimmer underneath."""
    level = f" Lv.{craft.level}" if craft.level else ""
    head = f"Craft ({craft.discipline}{level})"
    if craft.per_item:
        head += f": {format_mesos(craft.per_item)} each"
    margin = insights.craft_margin(item, craft)
    _marker(cv, y)
    width = cv.text(PAD + _INDENT, y, head, 11, theme.TEXT_DIM)
    if margin is not None:   # selling a crafted one at the typical price
        verdict = f"{abs(margin) * 100:.0f}% {'profit' if margin >= 0 else 'loss'}"
        cv.text(PAD + _INDENT + width + 6, y, verdict, 11, theme.UP if margin >= 0 else theme.DOWN, bold=True)
    recipe = ", ".join(f"{p.count}× {p.name}" for p in craft.parts[:3])
    y += _LINE
    for line in cv.wrap(recipe, 10, cv.width - 2 * PAD - _INDENT):
        cv.text(PAD + _INDENT, y, line, 10, theme.TEXT_FAINT)
        y += _LINE - 2
    return y + 2


def _bullet(cv: Canvas, y: int, text: str) -> int:
    _marker(cv, y)
    for line in cv.wrap(text, 11, cv.width - 2 * PAD - _INDENT - 2):
        cv.text(PAD + _INDENT, y, line, 11, theme.TEXT_DIM)
        y += _LINE
    return y + 2


def _marker(cv: Canvas, y: int) -> None:
    cv.draw.rectangle((PAD, y + 6, PAD + 3, y + 9), fill=theme.GOLD)
