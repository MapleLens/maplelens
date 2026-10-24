"""The map card shown beside a world-map tooltip: who lives there, with their sprites (pure, testable).

Every monster and NPC is listed. When one column would be taller than the game window, the lists flow into
more columns (the fewest that fit), so the card is as narrow as it can be without cutting anything off.
"""

import math
from collections.abc import Callable

from PIL import Image

from maplelens.mapinfo.models import MapInfo, Mob, Npc
from maplelens.overlay import theme
from maplelens.overlay.canvas import Canvas
from maplelens.overlay.glance import PAD

COL_W = 266        # design width of one column of rows (1x; the card is scaled by theme.CARD_SCALE)
_COL_GAP = 18
_SLOT = 36         # square the sprite is fitted into
_MOB_ROW_H, _NPC_ROW_H = 42, 38
_HEADER_H, _SECTION_H, _FOOTER_H = 76, 36, 28   # header block, a section's title row (with its gap), footer
_BAR_W, _BAR_H = 58, 5
_MAX_COLUMNS = 6
_COMFORT = 0.75   # a card taller than this share of the window gets another column: tall slivers read badly
LOADING_WIDTH = 290
SPINNER_FRAMES = 12   # one turn of the loading ring; the UI steps through them for the animation

Icons = Callable[[str, int], Image.Image | None]


def _rows(count: int, cols: int) -> int:
    return math.ceil(count / cols)


def _size(info: MapInfo, cols: int) -> tuple[int, int]:
    """Design size of the card with `cols` columns."""
    height = _HEADER_H + _FOOTER_H
    if info.mobs:
        height += _SECTION_H + _rows(len(info.mobs), cols) * _MOB_ROW_H
    if info.npcs:
        height += _SECTION_H + _rows(len(info.npcs), cols) * _NPC_ROW_H
    if not info.mobs and not info.npcs:
        height += 28
    return 2 * PAD + cols * COL_W + (cols - 1) * _COL_GAP, height


def choose_columns(info: MapInfo, max_size: tuple[float, float]) -> int:
    """Fewest columns that make a comfortably short card within `max_size` (design px).

    If even the widest allowed layout is taller than that, it is still the widest one that fits across.
    """
    most = max(len(info.mobs), len(info.npcs), 1)
    best = 1
    for cols in range(1, min(_MAX_COLUMNS, most) + 1):
        width, height = _size(info, cols)
        if width > max_size[0]:
            break
        best = cols
        if height <= max_size[1] * _COMFORT:
            break
    return best


def render_map_card(info: MapInfo, icons: Icons, *, ui_scale: float = 1.0,
                    max_size: tuple[int, int] | None = None) -> Image.Image:
    """The card for `info`; `max_size` (screen px) is the room the game window leaves for it."""
    factor = theme.CARD_SCALE * ui_scale
    cols = choose_columns(info, (max_size[0] / factor, max_size[1] / factor)) if max_size else 1
    width, height = _size(info, cols)
    card = Canvas(width, height)
    card.frame(height)
    y = _header(card, 8, info)
    if info.mobs:
        y = _section(card, y, "MONSTERS", f"{len(info.mobs)} kinds", info.mobs, cols, _MOB_ROW_H,
                     lambda cv, x, row_y, mob: _mob_row(cv, x, row_y, mob, info.spawns, icons))
    if info.npcs:
        y = _section(card, y, "NPCS", f"{len(info.npcs)} here", info.npcs, cols, _NPC_ROW_H,
                     lambda cv, x, row_y, npc: _npc_row(cv, x, row_y, npc, icons))
    if not info.mobs and not info.npcs:
        card.text(PAD, y + 8, "No monsters or NPCs listed", 12, theme.TEXT_DIM)
    _footer(card, height - _FOOTER_H)
    return card.image.resize((round(width * factor), round(height * factor)), Image.BICUBIC)


def _section(cv: Canvas, y: int, title: str, note: str, items: tuple, cols: int, row_h: int,
             draw_row: Callable[[Canvas, int, int, object], None]) -> int:
    """A titled list that flows top to bottom through `cols` balanced columns; returns the y below it."""
    cv.divider(y, PAD)
    cv.text(PAD, y + 12, title, 10, theme.TEXT_FAINT, bold=True)
    cv.text(cv.width - PAD, y + 12, note, 10, theme.TEXT_FAINT, anchor="ra")
    top = y + _SECTION_H - 8
    per_col = _rows(len(items), cols)
    for i, item in enumerate(items):
        col, row = divmod(i, per_col)
        draw_row(cv, PAD + col * (COL_W + _COL_GAP), top + row * row_h, item)
    return top + per_col * row_h + 8


def _header(cv: Canvas, y: int, info: MapInfo) -> int:
    """Title block; returns the y where the first section starts (its height is `_HEADER_H`)."""
    cv.overlay((PAD, y + 8, 7, 7), lambda d, s: d.ellipse((0, 0, 7 * s - 1, 7 * s - 1), fill=theme.BULLET), scale=4)
    cv.text(PAD + 13, y + 2, "Map", 11, theme.TEXT_DIM, bold=True)
    cv.chip(cv.width - PAD, y + 1, "Town" if info.is_town else info.street, right_align=True,
            fg=theme.GOLD if info.is_town else theme.TEXT_DIM, bold=info.is_town)
    cv.text(PAD, y + 22, _fit_text(cv, info.name, 17, True, cv.width - 2 * PAD), 17, theme.TEXT, bold=True)
    parts = [info.street] if info.is_town else []
    if info.mobs:
        low, high = info.levels
        parts += [f"Lv {low}" if low == high else f"Lv {low}–{high}", f"{info.spawns} spawn points"]
    if info.npcs:
        parts.append(f"{len(info.npcs)} NPCs")
    cv.text(PAD, y + 46, " · ".join(parts), 11, theme.TEXT_DIM)
    return _HEADER_H


def _mob_row(cv: Canvas, x: int, y: int, mob: Mob, total: int, icons: Icons) -> None:
    _sprite(cv, x, y, icons("mob", mob.id))
    text_x = x + _SLOT + 10
    right = x + COL_W
    cv.chip(text_x + cv.text(text_x, y + 1, _fit_text(cv, mob.name, 12, True, 120), 12, theme.TEXT, bold=True) + 6,
            y + 0, f"Lv {mob.level}", size=10, height=15)
    cv.text(text_x, y + 20, f"EXP {mob.exp:,} · HP {mob.hp:,}", 10, theme.TEXT_DIM)
    share = mob.count / total if total else 0
    cv.text(right, y, f"×{mob.count}", 14, theme.GOLD, bold=True, anchor="ra")
    bar_x, bar_y = right - _BAR_W, y + 24
    cv.overlay((bar_x, bar_y, _BAR_W, _BAR_H), lambda d, s: d.rounded_rectangle(
        (0, 0, _BAR_W * s - 1, _BAR_H * s - 1), radius=2 * s, fill=theme.TRACK), scale=3)
    filled = max(3, round(_BAR_W * share))
    cv.overlay((bar_x, bar_y, filled, _BAR_H), lambda d, s: d.rounded_rectangle(
        (0, 0, filled * s - 1, _BAR_H * s - 1), radius=2 * s, fill=theme.GOLD), scale=3)
    cv.text(bar_x - 5, bar_y + _BAR_H / 2, f"{share * 100:.0f}%", 10, theme.TEXT_FAINT, anchor="rm")


def _npc_row(cv: Canvas, x: int, y: int, npc: Npc, icons: Icons) -> None:
    _sprite(cv, x, y, icons("npc", npc.id))
    text_x, room = x + _SLOT + 10, COL_W - _SLOT - 10
    cv.text(text_x, y + 2, _fit_text(cv, npc.name, 12, True, room), 12, theme.TEXT, bold=True)
    if npc.role:
        cv.text(text_x, y + 20, _fit_text(cv, npc.role, 10, False, room), 10, theme.TEXT_DIM)


def _sprite(cv: Canvas, x: int, y: int, image: Image.Image | None) -> None:
    """The sprite centred in a dark slot (an empty slot while the download is still running)."""
    cv.overlay((x, y, _SLOT, _SLOT), lambda d, s: d.rounded_rectangle(
        (0, 0, _SLOT * s - 1, _SLOT * s - 1), radius=6 * s, fill=theme.TRACK, outline=theme.CHIP_EDGE), scale=3)
    if image is None:
        return
    inner = _SLOT - 6
    factor = min(inner / image.width, inner / image.height, 1.0)   # never blow a small sprite up: it would blur
    if factor < 1:
        image = image.resize((max(1, round(image.width * factor)), max(1, round(image.height * factor))),
                             Image.LANCZOS)
    cv.image.alpha_composite(image, (x + (_SLOT - image.width) // 2, y + (_SLOT - image.height) // 2))


def _fit_text(cv: Canvas, text: str, size: int, bold: bool, width: int) -> str:
    if cv.measure(text, size, bold) <= width:
        return text
    while text and cv.measure(text + "…", size, bold) > width:
        text = text[:-1]
    return text.rstrip() + "…"


def _footer(cv: Canvas, y: int) -> None:
    cv.divider(y + 2, PAD)
    cv.text(PAD, y + 9, "maplestory.io · classic-era data", 10, theme.TEXT_FAINT)
    cv.text(cv.width - PAD, y + 9, "MapleLens", 10, theme.TEXT_FAINT, anchor="ra")


def render_loading_card(name: str, frame: int, *, failed: bool = False, ui_scale: float = 1.0) -> Image.Image:
    """Placeholder while the map is being looked up: a turning ring (`frame` picks the angle) under the name."""
    height = 92
    body = Canvas(LOADING_WIDTH, height)
    body.frame(height)
    body.overlay((PAD, 12, 7, 7), lambda d, s: d.ellipse((0, 0, 7 * s - 1, 7 * s - 1), fill=theme.BULLET), scale=4)
    body.text(PAD + 13, 10, "Map", 11, theme.TEXT_DIM, bold=True)
    body.text(PAD, 30, _fit_text(body, name, 15, True, LOADING_WIDTH - 2 * PAD), 15, theme.TEXT, bold=True)
    headline = "Can't reach maplestory.io, retrying…" if failed else "Looking up monsters and NPCs…"
    if failed:
        body.text(PAD, 60, headline, 11, theme.WARN)
    else:
        _spinner(body, PAD, 56, frame)
        body.text(PAD + 26, 69, headline, 11, theme.TEXT_DIM, anchor="lm")
    factor = theme.CARD_SCALE * ui_scale
    return body.image.resize((round(LOADING_WIDTH * factor), round(height * factor)), Image.BICUBIC)


def _spinner(cv: Canvas, x: int, y: int, frame: int, size: int = 20) -> None:
    """A ring with a bright arc that advances 30 degrees per frame."""
    start = frame * 360 / SPINNER_FRAMES

    def draw(d, s):
        box = (2 * s, 2 * s, (size - 2) * s - 1, (size - 2) * s - 1)
        d.ellipse(box, outline=theme.TRACK, width=3 * s)
        d.arc(box, start, start + 100, fill=theme.GOLD, width=3 * s)

    cv.overlay((x, y, size, size), draw, scale=4)
