"""Tunable constants. Visual styling lives in `overlay/theme.py`, not here."""

from pathlib import Path

GAME_PROCESS = "maplestory.exe"
DEFAULT_SNAPSHOT = Path("data/list.json")
# Hourly updates keep prices current without polling the source aggressively.
SNAPSHOT_REFRESH_S = 3600
# Bound network delays and back off after transient failures.
SNAPSHOT_TIMEOUT_S = 30
SNAPSHOT_RETRY_S = 60

# The game draws an item tooltip's fill with its top-left corner at the mouse + this offset
# (measured by hovering items of every inventory tab: always +1,+32 give or take 2 px).
TOOLTIP_OFFSET = (1, 32)
# Equipment tooltips can have a second one (the item you wear) to their right: its left edge is this far
# from the first one's. Measured on a real compare screenshot (title reads fine anywhere in 368-380).
TOOLTIP_COMPARE_OFFSET_X = 373
# A misread of the compare tooltip title is bridged for this long.
COMPARE_GRACE_S = 0.4
# Equipment is the narrowest tooltip (370 px) and titles are read from this much of the left side, so
# the compare tooltip starting 373 px in never leaks into the first title.
TOOLTIP_TITLE_WIDTH = 370
# Only the top of the tooltips is captured: both titles, the presence check and the border rows that tell the width.
TOOLTIP_CAPTURE_SIZE = (TOOLTIP_COMPARE_OFFSET_X + TOOLTIP_TITLE_WIDTH, 120)

# Tooltip size (width, height) per kind of item, measured by hovering items of every tab.
# Heights vary a little inside a tab (equipment 323-398, use 178-277) with the description length;
# tune these if the card overlaps or floats away from the tooltip. Unknown items use the default.
TOOLTIP_SIZES = {   # keys: "Equip" (any equipment, incl. Cash Shop hats/pants) or a MesoWatch category
    "Equip": (370, 324),
    "Scrolls": (450, 178),
    "Use": (450, 178),
    "Stars": (450, 209),
    "Etc": (450, 178),
    "Cash Shop": (450, 277),
}
TOOLTIP_DEFAULT_SIZE = (450, 178)
# The game slides a tooltip up when it would run off the bottom of the client area. This is taller than any
# tooltip measured (an equipment-shaped cash ring with a long description: ~490), and bounds how far up to look.
TOOLTIP_MAX_HEIGHT = 560
# The only widths the game draws item tooltips at: equipment is narrow, everything else wide.
TOOLTIP_WIDTHS = (370, 450)

# A screen grab alone takes ~9 ms (GDI floor), so the detection loop barely sleeps.
POLL_S = 0.001
POLL_BACKGROUND_S = 0.1   # game not in front: nothing to do, check again soon
GAME_LOST_RETRY_S = 1.0

# OCR reads longer than this are a mis-read (e.g. title + description), not an item title.
MAX_TITLE_CHARS = 40

# An unknown title must read identically this many frames in a row before a "no price data" card shows: the
# game's hint tooltips (friend list) OCR differently every frame, an item name does not.
UNKNOWN_TITLE_FRAMES = 5
# Minimum fuzzy-match score (0-100) to accept an OCR'd name as a known item.
MIN_MATCH_SCORE = 88

# Gap between the tooltip and the card.
CARD_GAP_PX = 6
# Keep the pointer on the same item this long and the glance card grows into the full one.
DWELL_EXPAND_S = 1.2
# A single missed frame (mouse moving faster than the tooltip, OCR hiccup) must not flash the card off.
HIDE_GRACE_S = 0.15
UI_TICK_S = 0.006         # > 120 Hz polling so a 60 fps display never waits on us

# --- Rank card beside the Character Info window (classicrank.com) ---
# Offsets are from the window's top-left, measured on a native screenshot of the window.
INFO_TITLE_OFFSET = (9, 9)           # where the "CHARACTER INFO" title text starts
INFO_NAME_BOX = (21, 200, 160, 20)   # inside the grey name plate under the portrait (x, y, w, h)
INFO_WINDOW_SIZE = (493, 280)
# Searching the whole game for the window is the expensive part, so it only runs while the window is
# not known; once found it is re-checked in place, which is a tiny grab.
INFO_SEARCH_S = 0.25
INFO_POLL_S = 0.05
INFO_HIDE_GRACE_S = 0.3
INFO_CARD_GAP_PX = 4
# Rank lookups: the API caches for 30 s and allows 30 requests/minute per IP without a key.
RANK_TTL_S = 90.0
RANK_MISSING_TTL_S = 300.0   # unknown names (often an OCR slip) are not worth hammering
RANK_REQUEST_GAP_S = 2.0

# --- Map helper: hover a location on the world map (maplestory.io data and icons) ---
MAP_API = "https://maplestory.io/api/GMS/40"   # v40 is the classic-era data set (the Hill East of Henesys: 49 spawns)
MAP_REQUEST_TIMEOUT_S = 20   # maplestory.io is sometimes slow (a request took >8 s)
MAP_RETRY_S = 30.0          # after a failed lookup
# World-map tooltips are not at a fixed offset from the cursor (two samples differ by ~20 px vertically), so
# the tooltip box is located inside this window to the right of / below the cursor, once per hover (screen px
# at the measured resolution; origin is the cursor).
MAP_SCAN_OFFSET = (8, 8)
MAP_SCAN_SIZE = (560, 190)
MAP_TOOLTIP_MIN_SIZE = (120, 20)   # smaller blue boxes are not a tooltip
MAP_POLL_S = 0.06   # OCR of the scan window is the cost, not the grab; cached when the picture does not change
MAP_HIDE_GRACE_S = 0.8   # OCR scans are slower and noisier than item scans: bridge a few missed ones
MAP_STALE_S = 2.0   # a lookup nobody asked about for this long is dropped from the queue
MAP_FORGET_S = 0.6   # no tooltip read for this long: the next one is a new hover, start voting afresh
MAP_REJECT_S = 10.0   # a reading that matched no map is ignored this long (OCR then tries other readings)
