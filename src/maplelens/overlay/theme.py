"""Colours, fonts and metrics sampled from MapleStory's own item tooltip, so overlays look native."""

from functools import cache
from pathlib import Path

from PIL import ImageFont

RGBA = tuple[int, int, int, int]

# The tooltip fill is see-through: measured over dark and bright backdrops it resolves to roughly
# 80% of (58,62,120). A near-black base at higher alpha keeps the card dark over bright scenes, where the
# bluer fill washed out to a pale box next to the game's own tooltip.
PANEL_FILL: RGBA = (34, 37, 80, 215)
EDGE: RGBA = (118, 120, 150, 235)         # outer border line
EDGE_INNER: RGBA = (160, 162, 190, 130)   # soft inner line
DIVIDER: RGBA = (200, 204, 235, 60)
CHIP_FILL: RGBA = (36, 34, 62, 150)       # the dark class pills of equipment tooltips
CHIP_EDGE: RGBA = (150, 152, 190, 140)
TRACK: RGBA = (20, 22, 60, 150)           # dark groove of bars and charts

BULLET: RGBA = (110, 193, 255, 255)       # blue dot before titles in the game
TEXT: RGBA = (255, 255, 255, 255)
TEXT_DIM: RGBA = (226, 228, 246, 255)      # labels: the game prints "REQ LEV" etc. near-white
TEXT_FAINT: RGBA = (150, 155, 186, 255)    # like the greyed-out zero requirements (about 105,110,126 on the fill)
GOLD: RGBA = (255, 211, 77, 255)
GOLD_SOFT: RGBA = (255, 211, 77, 70)
UP: RGBA = (138, 230, 138, 255)
DOWN: RGBA = (255, 140, 140, 255)
WARN: RGBA = (255, 176, 96, 255)
ORANGE: RGBA = (255, 158, 46, 255)        # "Untradable", usable classes and stat bullets in the game
VOLUME: RGBA = (120, 170, 255, 120)

# The game draws its UI about 1.5x bigger than a 1x design at this resolution: measured on a native tooltip,
# body text has a 13 px capital height (Tahoma 12 px at 1x is 8.6) and a 450 px wide tooltip is 300 at 1x.
# The card is drawn at 1x and scaled by this, so it ends up as wide as a tooltip with the same text size.
CARD_SCALE = 1.5

_FONT_DIR = Path("C:/Windows/Fonts")


@cache
def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    """Tahoma, the game's UI face, at `size` px; falls back to Pillow's built-in font if it is missing."""
    try:
        return ImageFont.truetype(str(_FONT_DIR / ("tahomabd.ttf" if bold else "tahoma.ttf")), size)
    except OSError:
        return ImageFont.load_default(size)
