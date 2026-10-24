"""Background loop: read the world-map tooltip next to the cursor ("Victoria Road : Henesys")."""

import logging
import queue
import threading
import time
from dataclasses import dataclass
from pathlib import Path

import cv2

from maplelens import config
from maplelens.capture import game_window
from maplelens.capture.frame import ScreenGrabber, grab_normalised
from maplelens.geometry import Box, UiScale
from maplelens.mapinfo import resolve
from maplelens.mapinfo.models import Location
from maplelens.mapinfo.vote import Voter
from maplelens.vision import map_tooltip
from maplelens.vision.map_debug import DebugView
from maplelens.vision.ocr import TitleReader

log = logging.getLogger(__name__)



@dataclass(frozen=True)
class MapHover:
    """A world-map tooltip is showing next to the cursor."""

    location: Location
    offset: tuple[int, int]   # tooltip top-left relative to the cursor, screen px
    size: tuple[int, int]     # tooltip size, screen px
    bounds: Box               # game client area in screen coordinates
    scale: float = 1.0


class MapPipeline(threading.Thread):
    """Publishes `MapHover` (or None when there is no map tooltip) to `events`; the UI thread drains it."""

    def __init__(self, ui: UiScale | None = None, debug_view: bool = False) -> None:
        super().__init__(daemon=True, name="map-pipeline")
        self.events: queue.Queue[MapHover | None] = queue.Queue()
        self.ui = ui or UiScale()   # measured by the hover pipeline
        self.enabled = threading.Event()   # cleared by the overlay toggle: the thread then idles without grabbing
        self.enabled.set()
        self.item_showing = threading.Event()   # set while an item tooltip is up: no map is looked for then
        self._voter: Voter[tuple[Location, tuple[int, int], tuple[int, int]]] = Voter()
        self._last_reading = float("-inf")
        self._rejected: dict[str, float] = {}   # readings that matched no map, until when they are ignored
        self._debug_view = debug_view
        self._view: DebugView | None = None
        self._logged: list = []

    def run(self) -> None:
        grabber, reader = ScreenGrabber(), TitleReader()
        self._view = DebugView() if self._debug_view else None   # created here: cv2 windows belong to this thread
        hwnd: int | None = None
        while True:
            if hwnd is None or not game_window.is_alive(hwnd):
                hwnd = game_window.find_game_window()
            if hwnd is None or not self.enabled.is_set() or not game_window.is_foreground(hwnd):
                self.events.put(None)   # never draw over other apps
                time.sleep(config.GAME_LOST_RETRY_S if hwnd is None else config.POLL_BACKGROUND_S)
                continue
            try:
                self.events.put(self._scan(grabber, reader, game_window.client_box(hwnd)))
            except Exception:   # one bad frame (a window resize mid-grab) must never end the thread
                log.exception("map scan failed")
                self.events.put(None)
            time.sleep(config.MAP_POLL_S)

    def _scan(self, grabber: ScreenGrabber, reader: TitleReader, bounds: Box) -> MapHover | None:
        if self.item_showing.is_set():   # item tooltips (equips above all) are full of text that is not a map title
            if self._view:
                self._view.blank("item tooltip showing: map lookup paused")
            self._voter.clear()
            return None
        scale = self.ui.of(bounds)
        cx, cy = game_window.cursor_pos()
        x, y = cx + round(config.MAP_SCAN_OFFSET[0] * scale), cy + round(config.MAP_SCAN_OFFSET[1] * scale)
        w = min(round(config.MAP_SCAN_SIZE[0] * scale), bounds.right - x)
        h = min(round(config.MAP_SCAN_SIZE[1] * scale), bounds.bottom - y)
        if x < bounds.x or y < bounds.y or w < round(config.MAP_TOOLTIP_MIN_SIZE[0] * scale) or h <= 0:
            if self._view:
                self._view.blank("cursor too close to the edge of the game: nothing to scan")
            return self._reset()
        image = grab_normalised(grabber, Box(x, y, w, h), scale)
        mask = map_tooltip.text_mask(image)
        tint, rows = map_tooltip.tint_fraction(image), map_tooltip.text_rows(mask)
        if not map_tooltip.may_hold_tooltip(image, mask):
            self._show(image, mask, [], None, f"SKIP no tooltip colours/text: tint={tint:.3f} (need "
                       f"{map_tooltip.TINT_MIN_FRACTION}) text rows={rows} (need {map_tooltip.TEXT_ROWS_MIN})")
            return self._reset()
        lines = reader.read_lines(mask)
        found = map_tooltip.find_tooltip(lines)
        if log.isEnabledFor(logging.DEBUG) and lines != self._logged:
            self._logged = lines
            log.debug("map scan: lines=%s -> %s", [ln.text for ln in lines], found and found.location)
            _dump(image)
        min_w, min_h = config.MAP_TOOLTIP_MIN_SIZE
        if found is None or found.box.w < min_w or found.box.h < min_h:
            self._show(image, mask, lines, None, f"MISS no title in {len(lines)} OCR lines (tint={tint:.3f})")
            return self._reset()
        # Readings vary frame to frame ('Ground I' / 'Ground l/'); the voter settles them into one stable answer.
        box = found.box
        offset = (x - cx + round(box.x * scale), y - cy + round(box.y * scale))
        size = (round(box.w * scale), round(box.h * scale))
        self._last_reading = time.monotonic()
        reading = resolve.key(found.location)
        if self._rejected.get(reading, 0.0) > self._last_reading:   # this misreading already failed: wait for another
            return None
        answer = self._voter.add(reading, (found.location, offset, size))
        verdict = "VOTING (needs more agreeing readings)" if answer is None else f"OK {answer[0].name!r}"
        self._show(image, mask, lines, found, f"{verdict} read={found.location.name!r} tint={tint:.3f}")
        if answer is None:
            return None
        return MapHover(*answer, bounds, scale)

    def reject(self, location: Location) -> None:
        """The UI found no map for this reading (a misread): ignore it for a while so a better one can win."""
        self._rejected[resolve.key(location)] = time.monotonic() + config.MAP_REJECT_S
        self._voter.clear()

    def _show(self, image, mask, lines, found, verdict: str) -> None:
        if self._view:
            self._view.show(image, mask, lines, found, verdict)

    def _reset(self) -> None:
        if time.monotonic() - self._last_reading > config.MAP_FORGET_S:   # a single missed read is bridged
            self._voter.clear()
        return None


def _dump(image) -> None:
    """Debug aid: the scan window whenever it held tooltip-like colours (source of test fixtures)."""
    Path("debug").mkdir(exist_ok=True)
    cv2.imwrite("debug/map_scan.png", image)
