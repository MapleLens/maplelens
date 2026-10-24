"""Background loop: mouse position -> tooltip measurement -> item title -> market item."""

import logging
import queue
import threading
import time
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

import cv2
import numpy as np

from maplelens import config
from maplelens.capture import game_window
from maplelens.capture.frame import ScreenGrabber
from maplelens.geometry import Box, UiScale, ui_scale
from maplelens.market.catalog import Catalog, clean_title
from maplelens.market.models import Item
from maplelens.vision.hud import find_bar
from maplelens.vision.locate import candidate_tops
from maplelens.vision.ocr import TitleReader
from maplelens.vision.tooltip import measure_width, tooltip_present

log = logging.getLogger(__name__)

_WIDTH_MEMORY = 256   # titles whose measured tooltip width is remembered


@dataclass(frozen=True)
class Hover:
    """An item tooltip is showing under the mouse."""

    title: str
    item: Item | None
    tooltip_size: tuple[int, int]
    compare: bool   # equipment compare: a second tooltip (the worn item) sits to the right of the first
    bounds: Box     # game client area in screen coordinates
    scale: float = 1.0   # UI size relative to the measured resolution; tooltip_size is already in screen pixels
    pushed_up_height: int | None = None   # screen px; set when the game slid the tooltip up to fit on screen
    crafted: bool = False   # a player-crafted item: the tooltip has an extra crafter-name line on top


def tooltip_size(item: Item | None, measured_width: int | None = None) -> tuple[int, int]:
    """Width and height of the tooltip for `item`.

    The width is measured on screen when that was possible (it is what placement depends on and it does not
    need the item to be known); the height comes from the per-kind table in config.
    """
    narrow = min(config.TOOLTIP_WIDTHS)
    equipment = item.is_equip if item is not None else measured_width == narrow
    kind = "Equip" if equipment else (item.category if item is not None else None)
    width, height = config.TOOLTIP_SIZES.get(kind, config.TOOLTIP_DEFAULT_SIZE)
    return (measured_width or width), height


def _crafted_height(height: int, crafted: bool) -> int:
    return height + config.TOOLTIP_CRAFTED_SHIFT if crafted else height


def _looks_like_title(text: str) -> bool:
    """Item names are short phrases; UI hint tooltips are sentences ("Double-click to ...") or OCR noise."""
    text = text.strip()
    if len(text) < 3 or not text[0].isupper() or text.endswith(".") or "click" in text.lower():
        return False
    return sum(c.isalnum() or c in " '-:" for c in text) >= 0.9 * len(text)


def _grab_normalised(grabber: ScreenGrabber, area: Box, scale: float) -> np.ndarray:
    """Grab `area`, then bring it to the resolution the vision constants were measured at."""
    image = grabber.grab(area)
    if abs(scale - 1) > 0.01:
        image = cv2.resize(image, None, fx=1 / scale, fy=1 / scale,
                           interpolation=cv2.INTER_CUBIC if scale < 1 else cv2.INTER_AREA)
    return image


def _dump_capture(image, title: str) -> None:
    """Debug aid: keep the raw capture of each new tooltip in debug/ (source of test fixtures)."""
    if log.isEnabledFor(logging.DEBUG):
        Path("debug").mkdir(exist_ok=True)
        name = "".join(c for c in title if c.isalnum()) or "untitled"
        cv2.imwrite(f"debug/{name}.png", image)


class HoverPipeline(threading.Thread):
    """Publishes `Hover` (or None when no tooltip is showing) to `events`; the UI thread drains it."""

    def __init__(self, catalog: Catalog, ui: UiScale | None = None) -> None:
        super().__init__(daemon=True, name="hover-pipeline")
        self.ui = ui or UiScale()
        self._bar_checked_at = float("-inf")
        self.events: queue.Queue[Hover | None] = queue.Queue()
        self.enabled = threading.Event()   # cleared by the overlay toggle: the thread then idles without grabbing
        self.enabled.set()
        self._catalog = catalog
        self.catalog_updates: queue.Queue[Catalog] = queue.Queue()
        self._compare_seen_at = float("-inf")
        self._widths: dict[str, int] = {}
        self._unknown_title: str | None = None
        self._unknown_streak = 0

    def run(self) -> None:
        grabber, reader = ScreenGrabber(), TitleReader()
        hwnd: int | None = None
        last_title, last_item, last_crafted = None, None, False
        stats: Counter = Counter()
        logged = time.monotonic()
        while True:
            try:
                self._catalog = self.catalog_updates.get_nowait()
                last_title, last_item, last_crafted = None, None, False
            except queue.Empty:
                pass
            if time.monotonic() - logged > 2:
                log.debug("stats/2s %s", dict(stats))
                stats.clear()
                logged = time.monotonic()
            if hwnd is None or not game_window.is_alive(hwnd):
                hwnd = game_window.find_game_window()
            if hwnd is None:
                self.events.put(None)
                time.sleep(config.GAME_LOST_RETRY_S)
                continue
            if not self.enabled.is_set() or not game_window.is_foreground(hwnd):
                stats["background"] += 1
                self.events.put(None)   # never draw over other apps
                time.sleep(config.POLL_BACKGROUND_S)
                continue

            bounds = game_window.client_box(hwnd)
            self._measure_ui(grabber, bounds)
            scale = self.ui.of(bounds)
            area = self._tooltip_area(bounds, scale)
            if area is None:   # no room below the mouse for even the top of a tooltip: it can only be pushed up
                if not self._publish_pushed_up(grabber, reader, bounds, scale):
                    stats["miss_area"] += 1
                    self.events.put(None)
                time.sleep(config.POLL_S)
                continue

            image = _grab_normalised(grabber, area, scale)
            title = reader.read(image, config.TOOLTIP_TITLE_WIDTH)
            if title != last_title:
                last_title = title
                self._compare_seen_at = float("-inf")   # a new item: forget the old item's compare tooltip
                # Longer than a title = mis-read (title + description), not worth matching.
                last_item, last_crafted = self._lookup(reader, image, title)
                log.debug("ocr=%r -> %s", title, last_item and last_item.name)
                if last_item:
                    _dump_capture(image, title)
            # A recognised item name at the spot where tooltips appear is the best proof a tooltip is
            # there: the blue fill is unreliable because it changes with whatever is behind the
            # translucent box. The colour check only has to carry unknown items.
            # Other blue-ish UI (friend list, menus) passes the colour check and OCR reads names off it, so an
            # unknown title must also have the tooltip border at one of the game's fixed widths.
            if last_item is None and self._publish_pushed_up(grabber, reader, bounds, scale):
                time.sleep(config.POLL_S)
                continue
            if last_item is None:
                self._unknown_streak = self._unknown_streak + 1 if title == self._unknown_title else 1
                self._unknown_title = title
            else:
                self._unknown_title, self._unknown_streak = None, 0
            # The game's other tooltips ("Double-click to send whispers" in the friend list) look just like item
            # tooltips; their OCR text is short, noisy and different every frame, an item title is stable.
            if last_item is None and not (tooltip_present(image)
                                          and measure_width(image, config.TOOLTIP_WIDTHS) is not None
                                          and self._unknown_streak >= config.UNKNOWN_TITLE_FRAMES
                                          and _looks_like_title(title)):
                stats["miss"] += 1
                self.events.put(None)
                time.sleep(config.POLL_S)
                continue

            stats["ok"] += 1
            width = self._tooltip_width(image, title)
            size = tooltip_size(last_item, width)
            size = (round(size[0] * scale), round(_crafted_height(size[1], last_crafted) * scale))
            # Show the matched name, not the raw OCR text: its per-frame variants must not repaint the card.
            shown = last_item.name if last_item else clean_title(title)
            equipment = last_item.is_equip if last_item else width == min(config.TOOLTIP_WIDTHS)
            compare = self._has_compare(reader, image, equipment, time.monotonic())
            self.events.put(Hover(shown, last_item, size, compare, bounds, scale, crafted=last_crafted))
            time.sleep(config.POLL_S)

    def _measure_ui(self, grabber: ScreenGrabber, bounds: Box) -> None:
        """Re-measure the UI scale off the status bar now and then (a grab of the bottom of the client)."""
        now = time.monotonic()
        if now - self._bar_checked_at < config.STATUS_BAR_CHECK_S:
            return
        self._bar_checked_at = now
        height = min(bounds.h, round(config.STATUS_BAR_STRIP_HEIGHT * ui_scale(bounds)))
        bar = find_bar(grabber.grab(Box(bounds.x, bounds.bottom - height, bounds.w, height)))
        if bar is None:
            return   # hidden (Cash Shop, a cut scene): keep the last measurement
        scale = bar.width / config.STATUS_BAR_WIDTH
        if abs(scale - self.ui.of(bounds)) > 0.005:
            log.info("UI scale %.3f (status bar %d px wide, client %dx%d)", scale, bar.width, bounds.w, bounds.h)
        self.ui.set(bounds, scale)

    def _lookup(self, reader: TitleReader, image: np.ndarray, title: str) -> tuple[Item | None, bool]:
        """The item named by `title` (the one-line OCR read of the tooltip in `image`), and whether it is crafted."""
        # Longer than a title = mis-read (title + description), not worth matching.
        item = self._catalog.lookup(title) if len(title) <= config.MAX_TITLE_CHARS else None
        if item is not None:
            return item, False
        # Crafted before wrapped: the wrapped read's two rows reach down into a crafted item's title and would
        # match it without knowing the tooltip is taller.
        crafted = reader.read(image, config.TOOLTIP_TITLE_WIDTH, top=config.TOOLTIP_CRAFTED_SHIFT)
        item = self._catalog.lookup(crafted) if len(crafted) <= config.MAX_TITLE_CHARS else None
        if item is not None:
            return item, True
        wrapped = reader.read(image, config.TOOLTIP_TITLE_WIDTH, lines=2)   # long names wrap onto a second line
        item = self._catalog.lookup(wrapped) if len(wrapped) <= config.MAX_TITLE_CHARS else None
        return item, False

    def _publish_pushed_up(self, grabber: ScreenGrabber, reader: TitleReader, bounds: Box, scale: float) -> bool:
        """Look for a known item's tooltip the game slid up to fit on screen; publish it and say whether found.

        The game keeps a tooltip inside the client area, so one taller than the room under the mouse has its
        bottom on the client's bottom edge and its top somewhere above the usual spot. Only a recognised item
        counts here: stray UI text must never be mistaken for a tooltip we had to go looking for.
        """
        cx, cy = game_window.cursor_pos()
        x = cx + round(config.TOOLTIP_OFFSET[0] * scale)
        usual_top = cy + round(config.TOOLTIP_OFFSET[1] * scale)
        if usual_top + round(config.TOOLTIP_MAX_HEIGHT * scale) <= bounds.bottom or x < bounds.x:
            return False   # even the tallest tooltip fits below the mouse: nothing was pushed
        w = min(round(config.TOOLTIP_CAPTURE_SIZE[0] * scale), bounds.right - x)
        top = max(bounds.y, bounds.bottom - round(config.TOOLTIP_MAX_HEIGHT * scale))
        if w < round(config.TOOLTIP_TITLE_WIDTH * scale) or top >= bounds.bottom:
            return False
        strip = _grab_normalised(grabber, Box(x, top, w, bounds.bottom - top), scale)
        rows = candidate_tops(strip)
        log.debug("pushed-up scan: usual_top=%d bottom=%d candidates=%s", usual_top, bounds.bottom, rows)
        for row in rows:
            screen_top = top + round(row * scale)
            if screen_top > usual_top:
                continue   # only ever pushed up, never down
            image = strip[row : row + config.TOOLTIP_CAPTURE_SIZE[1]]
            title = reader.read(image, config.TOOLTIP_TITLE_WIDTH)
            item, crafted = self._lookup(reader, image, title)
            if item is None:
                continue
            width = self._tooltip_width(image, title)
            size = tooltip_size(item, width)
            height = bounds.bottom - screen_top
            compare = self._has_compare(reader, image, item.is_equip, time.monotonic())
            self.events.put(Hover(item.name, item, (round(size[0] * scale), height), compare, bounds, scale, height,
                                  crafted))
            return True
        return False

    def _tooltip_width(self, image: np.ndarray, title: str) -> int | None:
        """Measured width, remembered per title so one unsure frame keeps the last good answer."""
        width = measure_width(image, config.TOOLTIP_WIDTHS)
        if width is not None:
            if len(self._widths) > _WIDTH_MEMORY:
                self._widths.clear()
            self._widths[title] = width
        return self._widths.get(title)

    def _has_compare(self, reader: TitleReader, image: np.ndarray, equipment: bool, now: float) -> bool:
        """Is a second tooltip (the worn item) showing next to an equipment tooltip?

        Only a title that matches a known item counts: in a shop the strip next to the tooltip holds
        arbitrary UI text, and treating that as a compare tooltip flipped the card between sides. A single
        misread is bridged for a moment so a real compare tooltip does not flicker either.
        """
        if equipment:
            second = image[:, config.TOOLTIP_COMPARE_OFFSET_X :]
            for top in (0, config.TOOLTIP_CRAFTED_SHIFT):   # the worn item may be crafted too
                if self._catalog.lookup(reader.read(second, second.shape[1], top=top)) is not None:
                    self._compare_seen_at = now
                    break
        return now - self._compare_seen_at < config.COMPARE_GRACE_S

    @staticmethod
    def _tooltip_area(bounds: Box, scale: float = 1.0) -> Box | None:
        """Where the top of the tooltip would be if one is showing, or None if that is outside the game."""
        cx, cy = game_window.cursor_pos()
        x, y = cx + round(config.TOOLTIP_OFFSET[0] * scale), cy + round(config.TOOLTIP_OFFSET[1] * scale)
        w, h = (round(v * scale) for v in config.TOOLTIP_CAPTURE_SIZE)
        w = min(w, bounds.right - x)   # near the right edge there is no room for a compare tooltip anyway
        inside = (bounds.x <= x and bounds.y <= y and w >= round(config.TOOLTIP_TITLE_WIDTH * scale)
                  and y + h <= bounds.bottom)
        return Box(x, y, w, h) if inside else None
