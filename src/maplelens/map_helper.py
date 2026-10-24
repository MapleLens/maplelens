"""The map helper on the UI thread: hover a world-map location, get a card of its monsters and NPCs."""

import logging
import queue
import time
from collections import OrderedDict

from PIL import Image

from maplelens import config
from maplelens.capture import game_window
from maplelens.geometry import Box, UiScale
from maplelens.map_pipeline import MapHover, MapPipeline
from maplelens.mapinfo import resolve
from maplelens.mapinfo.icons import IconCache, default_dir
from maplelens.mapinfo.service import MapResult, MapService, Status
from maplelens.mapinfo.store import MapStore
from maplelens.mapinfo.store import default_dir as store_dir
from maplelens.overlay.layered import LayeredWindow
from maplelens.overlay.map_card import SPINNER_FRAMES, render_loading_card, render_map_card
from maplelens.overlay.placement import place_card

log = logging.getLogger(__name__)
_CARD_CACHE_SIZE = 48   # the loading ring's frames take a dozen slots
_SPINNER_FPS = 12


class MapHelper:
    """Owns the map pipeline, lookups and window; the app calls `tick()` on the main thread."""

    def __init__(self, ui: UiScale | None = None) -> None:
        self._pipeline = MapPipeline(ui, debug_view=log.isEnabledFor(logging.DEBUG))   # --debug opens the OpenCV window
        self._service = MapService(IconCache(default_dir()), MapStore(store_dir()))
        self._window = LayeredWindow()
        self._hover: MapHover | None = None
        self._seen_at = 0.0
        self._shown: tuple | None = None   # which card the window holds
        self._logged: tuple | None = None
        self._cards: OrderedDict[tuple, Image.Image] = OrderedDict()

    def start(self) -> None:
        self._service.start()
        self._pipeline.start()

    def set_enabled(self, enabled: bool) -> None:
        if enabled:
            self._pipeline.enabled.set()
        else:
            self._pipeline.enabled.clear()
            self.hide()

    def set_item_showing(self, showing: bool) -> None:
        """An item tooltip is up: the map helper stands down (no scanning, no lookups, no card)."""
        if showing:
            self._pipeline.item_showing.set()
            self.hide()
        else:
            self._pipeline.item_showing.clear()

    def hide(self) -> None:
        self._hover = self._shown = None
        self._window.hide()

    def close(self) -> None:
        self._window.close()

    def drain(self) -> None:
        """Throw away what the idle pipeline keeps posting while the overlay is switched off."""
        while not self._pipeline.events.empty():
            self._pipeline.events.get_nowait()

    def tick(self) -> None:
        while True:   # only the newest pipeline result matters
            try:
                event = self._pipeline.events.get_nowait()
            except queue.Empty:
                break
            if event is not None:   # misses are judged by time below, not by a single scan
                self._hover, self._seen_at = event, time.monotonic()
        hover = self._hover
        if hover is None or time.monotonic() - self._seen_at > config.MAP_HIDE_GRACE_S:
            self.hide()
            return
        result = self._service.get(hover.location)
        if log.isEnabledFor(logging.DEBUG) and (hover.location, result.status) != self._logged:
            self._logged = (hover.location, result.status)
            log.debug("map hover %s -> %s offset=%s size=%s", hover.location, result.status.value, hover.offset,
                      hover.size)
        if result.status is Status.MISSING:   # unknown text is noise, not a map: show nothing
            self._shown = None
            self._window.hide()
            self._hover = None
            self._pipeline.reject(hover.location)   # a misread of a real title may be followed by a better one
            return
        card, key = self._card_for(hover, result)
        cx, cy = game_window.cursor_pos()
        tooltip = Box(cx + hover.offset[0], cy + hover.offset[1], *hover.size)
        x, y = place_card(tooltip, card.size, round(config.CARD_GAP_PX * hover.scale), hover.bounds, compare=False)
        if self._shown != key or not self._window.visible:
            self._window.set_image(card, x, y)
            log.debug("map card shown at %s,%s size=%s", x, y, card.size)
            self._shown = key
        else:
            self._window.move(x, y)

    def _card_for(self, hover: MapHover, result: MapResult) -> tuple[Image.Image, tuple]:
        """The card for what the lookup has so far, and the key that says when the window must be redrawn."""
        loading = result.status is not Status.FOUND
        # The ring turns while the lookup runs; the failed notice and the finished card stand still.
        frame = int(time.monotonic() * _SPINNER_FPS) % SPINNER_FRAMES if result.status is Status.LOADING else 0
        key = (resolve.key(hover.location), result.status, result.icons, frame, hover.scale)
        if key not in self._cards:
            if loading:
                card = render_loading_card(hover.location.name, frame, failed=result.status is Status.FAILED,
                                           ui_scale=hover.scale)
            else:
                card = render_map_card(result.info, self._service.icons.get, ui_scale=hover.scale,
                                   max_size=_room(hover.bounds))
            self._cards[key] = card
            if len(self._cards) > _CARD_CACHE_SIZE:
                self._cards.popitem(last=False)
        self._cards.move_to_end(key)
        return self._cards[key], key


def _room(bounds: Box) -> tuple[int, int]:
    """The most a card may take: half the game's width (it sits beside the tooltip) and nearly all its height."""
    return bounds.w // 2, round(bounds.h * 0.96)
