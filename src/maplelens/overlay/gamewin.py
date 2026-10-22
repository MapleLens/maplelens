"""Pieces of MapleStory's own windows (Character Info, Item List...): blue frame, white title bar, light
inner panel, pink label pills, beige value boxes, green buttons.

Every colour and size was measured pixel by pixel on a native screenshot of the Character Info window
(1x, no scaling), so the pieces line up with the real window when drawn beside it.
"""

from PIL import ImageDraw

from maplelens.overlay import theme
from maplelens.overlay.canvas import Canvas
from maplelens.overlay.theme import RGBA

BODY: RGBA = (156, 177, 200, 255)          # frame and bottom strip
FRAME_OUTER: RGBA = (209, 219, 226, 255)   # light line outside the frame
FRAME_DARK: RGBA = (112, 133, 155, 255)    # dark blue line just inside it
FRAME_MID: RGBA = (146, 171, 196, 255)
TITLE_FILL: RGBA = (255, 255, 255, 255)
PANEL: RGBA = (245, 245, 247, 255)         # light area the rows sit on
PANEL_EDGE: RGBA = (170, 185, 203, 255)
INK: RGBA = (36, 36, 36, 255)
INK_DIM: RGBA = (104, 108, 118, 255)
VALUE_FILL: RGBA = (226, 223, 212, 255)
VALUE_EDGE: RGBA = (208, 210, 202, 255)
PINK: RGBA = (204, 79, 130, 255)
PINK_EDGE: RGBA = (166, 70, 110, 255)
PINK_LIGHT: RGBA = (214, 109, 154, 255)
PINK_INK: RGBA = (255, 233, 246, 255)
GREEN_EDGE: RGBA = (12, 22, 4, 255)
GREEN_TOP: RGBA = (106, 173, 27)           # vertical gradient of the green buttons
GREEN_BOTTOM: RGBA = (156, 217, 69)
GREEN_SHINE: RGBA = (140, 203, 70, 255)
GREEN_INK: RGBA = (205, 236, 165, 255)
RED: RGBA = (214, 64, 64, 255)

BORDER = 6            # frame thickness: the title bar and panel start this far in
TITLE_H = 20
BUTTON_H = 20
STRIP_H = 3 + BUTTON_H + BORDER   # blue strip under the panel: gap, a button, the frame
PANEL_TOP = BORDER + TITLE_H + 2   # 2 px of blue between title bar and panel
PAD = BORDER + 6      # content starts this far from the card edge (6 px inside the panel)
PAD_V = 6             # panel top/bottom padding
ROW_H = 22
ROW_GAP = 4
_CAP = 0.36           # half of Tahoma's capital height / size: puts text's visual centre on a line


def _vgradient(d: ImageDraw.ImageDraw, x0: int, x1: int, y0: int, y1: int, top, bottom) -> None:
    span = max(y1 - y0, 1)
    for y in range(y0, y1 + 1):
        t = (y - y0) / span
        d.line((x0, y, x1, y), fill=tuple(round(a + (b - a) * t) for a, b in zip(top, bottom, strict=True)))


def window(cv: Canvas, height: int, title: str) -> None:
    """Blue frame, white title bar and the light panel above the bottom strip."""
    w = cv.width
    panel = (BORDER, PANEL_TOP, w - BORDER - 1, height - STRIP_H - 1)
    cv.overlay((0, 0, w, height), lambda d, s: (
        d.rounded_rectangle((0, 0, w * s - 1, height * s - 1), radius=5 * s, fill=FRAME_OUTER),
        d.rounded_rectangle((s, s, (w - 1) * s - 1, (height - 1) * s - 1), radius=5 * s, fill=FRAME_DARK),
        d.rounded_rectangle((2 * s, 2 * s, (w - 2) * s - 1, (height - 2) * s - 1), radius=4 * s, fill=FRAME_MID),
        d.rounded_rectangle((3 * s, 3 * s, (w - 3) * s - 1, (height - 3) * s - 1), radius=3 * s, fill=BODY),
        d.rounded_rectangle((BORDER * s, BORDER * s, (w - BORDER) * s - 1, (BORDER + TITLE_H) * s - 1),
                            radius=3 * s, fill=TITLE_FILL),
        d.rounded_rectangle((panel[0] * s, panel[1] * s, (panel[2] + 1) * s - 1, (panel[3] + 1) * s - 1),
                            radius=3 * s, fill=PANEL_EDGE),
    ), scale=3)
    inner = (panel[0] + 1, panel[1] + 1, panel[2] - 1, panel[3] - 1)
    cv.draw.rectangle(inner, fill=PANEL)
    _vgradient(cv.draw, inner[0], inner[2], inner[3] - 4, inner[3], PANEL[:3], (210, 214, 219))
    tracked(cv, PAD, BORDER + TITLE_H / 2, title, 9, INK)


def tracked(cv: Canvas, x: int, y: int, text: str, size: int, fill: RGBA) -> None:
    """The window titles are blocky, hard-edged, wide capitals: bold, unsmoothed and letter-spaced."""
    font = theme.font(size, True)
    cv.draw.fontmode = "1"
    try:
        for char in text:
            cv.draw.text((x, y + round(_CAP * size)), char, font=font, fill=fill, anchor="ls")
            x += round(cv.draw.textlength(char, font=font)) + 1
    finally:
        cv.draw.fontmode = "L"


def centred(cv: Canvas, cx: float, cy: float, text: str, size: int, fill: RGBA = theme.TEXT) -> None:
    """Text whose capital letters are centred on (cx, cy); Pillow's own "middle" anchor sits too low."""
    cv.text(cx, cy + _CAP * size, text, size, fill, anchor="ms")


def pill(cv: Canvas, box: tuple[int, int, int, int], text: str, size: int = 11) -> None:
    """Pink label (LEVEL, JOB...): dark pink outline, lighter inner line, flat fill, pale pink text."""
    x, y, w, h = box
    cv.overlay(box, lambda d, s: (
        d.rounded_rectangle((0, 0, w * s - 1, h * s - 1), radius=3 * s, fill=PINK_EDGE),
        d.rounded_rectangle((s, s, (w - 1) * s - 1, (h - 1) * s - 1), radius=2 * s, fill=PINK_LIGHT),
        d.rounded_rectangle((2 * s, 2 * s, (w - 2) * s - 1, (h - 2) * s - 1), radius=2 * s, fill=PINK),
    ), scale=3)
    centred(cv, x + w / 2, y + h / 2, text, size, PINK_INK)


def button(cv: Canvas, box: tuple[int, int, int, int], text: str, size: int = 11) -> None:
    """Green button (REQUEST PARTY...): near-black outline, lit top, light-to-bright vertical gradient."""
    x, y, w, h = box
    cv.overlay(box, lambda d, s: d.rounded_rectangle((0, 0, w * s - 1, h * s - 1), radius=3 * s, fill=GREEN_EDGE),
               scale=3)
    _vgradient(cv.draw, x + 1, x + w - 2, y + 2, y + h - 3, GREEN_TOP, GREEN_BOTTOM)
    cv.draw.line((x + 1, y + 1, x + w - 2, y + 1), fill=GREEN_SHINE)
    cv.draw.line((x + 1, y + 2, x + 1, y + h - 3), fill=GREEN_SHINE)
    centred(cv, x + w / 2, y + h / 2, text, size, GREEN_INK)


def value_box(cv: Canvas, box: tuple[int, int, int, int]) -> None:
    x, y, w, h = box
    cv.overlay(box, lambda d, s: (
        d.rounded_rectangle((0, 0, w * s - 1, h * s - 1), radius=2 * s, fill=VALUE_EDGE),
        d.rounded_rectangle((s, s, (w - 1) * s - 1, (h - 1) * s - 1), radius=2 * s, fill=VALUE_FILL),
    ), scale=3)


def row(cv: Canvas, y: int, label: str, value: str, extra: str | None = None, *, height: int = ROW_H,
        value_size: int = 14) -> int:
    """`LABEL | value` the way Character Info lists LEVEL / JOB / FAME; returns the y below it."""
    label_w = 86
    pill(cv, (PAD, y, label_w, height), label)
    box_x = PAD + label_w + 2
    box_w = cv.width - PAD - box_x
    value_box(cv, (box_x, y, box_w, height))
    cv.text(box_x + 7, y + height / 2 + _CAP * value_size, value, value_size, INK, anchor="ls")
    if extra:
        cv.text(box_x + box_w - 7, y + height / 2, extra, 11, INK_DIM, anchor="rm")
    return y + height + ROW_GAP
