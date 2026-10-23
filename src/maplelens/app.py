"""Wires the detection pipeline to the overlay window on the UI thread."""

import ctypes
import logging
import queue
import time
from collections import OrderedDict
from dataclasses import replace
from pathlib import Path

from PIL import Image

from maplelens import config, settings_store
from maplelens.capture import game_window
from maplelens.geometry import Box
from maplelens.info_pipeline import InfoPipeline, InfoWindow
from maplelens.market.catalog import Catalog
from maplelens.market.refresh import SnapshotRefresh
from maplelens.market.sample import SAMPLE_ITEMS
from maplelens.market.snapshot import SnapshotError, load_snapshot
from maplelens.overlay.card import render_card
from maplelens.overlay.layered import LayeredWindow, pump_messages
from maplelens.overlay.placement import place_beside_window, place_card
from maplelens.overlay.rank_card import render_rank_card
from maplelens.pipeline import Hover, HoverPipeline
from maplelens.ranking.service import RankResult, RankService
from maplelens.settings_controller import SettingsController
from maplelens.settings_store import Settings
from maplelens.tray.icon import load_icon
from maplelens.tray.settings import SettingsWindow
from maplelens.tray.tray import TrayIcon

log = logging.getLogger(__name__)
_CARD_CACHE_SIZE = 64
_RANK_CARD_CACHE_SIZE = 16


def build_catalog(snapshot: Path) -> Catalog:
    try:
        data = load_snapshot(snapshot)
        catalog = Catalog(data.items, data.generated_at)
        print(f"Loaded {len(catalog)} items from {snapshot}")
    except SnapshotError as exc:
        print(f"{exc}\nUsing {len(SAMPLE_ITEMS)} built-in demo items. "
              "Cached prices will be used when available.")
        catalog = Catalog(SAMPLE_ITEMS)
    return catalog


class PriceOverlayApp:
    """Hover an inventory item -> a price card appears next to its tooltip."""

    def __init__(self, catalog: Catalog, *, auto_refresh: bool = False) -> None:
        self._catalog = catalog
        self._refresh = SnapshotRefresh(config.DEFAULT_SNAPSHOT) if auto_refresh else None
        self._window = LayeredWindow()
        self._pipeline = HoverPipeline(catalog)
        self._hover: Hover | None = None
        self._seen_at = 0.0                 # when a tooltip was last detected
        self._shown: tuple[str, bool] | None = None   # which card (title, expanded) the window holds
        self._since = 0.0                   # when the pointer arrived on the current item
        self._cards: OrderedDict[tuple[str, bool, float, bool], Image.Image] = OrderedDict()
        # Rank panel beside the Character Info window: its own window, pipeline and lookups.
        self._rank_window = LayeredWindow()
        self._info_pipeline = InfoPipeline()
        self._ranks = RankService()
        self._info: InfoWindow | None = None
        self._info_seen_at = 0.0
        self._rank_shown: tuple[str, RankResult, float] | None = None
        self._rank_cards: OrderedDict[tuple[str, RankResult, float], Image.Image] = OrderedDict()
        self._quit = False
        self._controls = SettingsController(settings_store.default_path(), self._on_settings_changed)
        self._settings = SettingsWindow(load_icon(), self._controls.update, self._capture_hotkey)
        self._tray = TrayIcon(lambda: self._settings.show(self._controls.current), self._request_quit,
                              self._toggle_overlay)

    def run(self) -> None:
        if self._refresh is not None:
            self._refresh.start()
        self._apply_overlay_enabled(self._controls.current.overlay_enabled)
        self._register_hotkey(self._controls.current.toggle_hotkey)
        self._pipeline.start()
        self._info_pipeline.start()
        self._ranks.start()
        # Windows' default timer granularity is ~15.6 ms, which makes sleep() coarse and jittery.
        ctypes.windll.winmm.timeBeginPeriod(1)
        try:
            while not self._quit:
                pump_messages()
                self._tick()
                time.sleep(config.UI_TICK_S)
        except KeyboardInterrupt:
            pass
        finally:
            if self._refresh is not None:
                self._refresh.stop_event.set()
            ctypes.windll.winmm.timeEndPeriod(1)
            self._tray.close()
            self._settings.close()
            self._window.close()
            self._rank_window.close()

    def _request_quit(self) -> None:
        self._quit = True

    def _toggle_overlay(self) -> None:
        self._controls.toggle_overlay()
        self._settings.sync(self._controls.current)   # the window may be open while the hotkey is pressed

    def _on_settings_changed(self, old: Settings, new: Settings) -> None:
        if new.overlay_enabled != old.overlay_enabled:
            self._apply_overlay_enabled(new.overlay_enabled)
        if new.toggle_hotkey != old.toggle_hotkey:
            self._register_hotkey(new.toggle_hotkey)

    def _apply_overlay_enabled(self, enabled: bool) -> None:
        for pipeline in (self._pipeline, self._info_pipeline):
            if enabled:
                pipeline.enabled.set()
            else:
                pipeline.enabled.clear()
        self._tray.overlay_enabled = enabled
        if not enabled:
            self._hide_everything()

    def _register_hotkey(self, packed: int) -> None:
        if not self._tray.set_hotkey(packed):
            log.warning("hotkey %#06x is taken by another program", packed)

    def _capture_hotkey(self, capturing: bool) -> None:
        """While the hotkey box has focus the combo must reach it, not fire the toggle."""
        self._tray.set_hotkey(0 if capturing else self._controls.current.toggle_hotkey)

    def _hide_everything(self) -> None:
        self._hover = self._info = self._shown = self._rank_shown = None
        self._window.hide()
        self._rank_window.hide()

    def _tick(self) -> None:
        if not self._controls.current.overlay_enabled:
            for pipeline in (self._pipeline, self._info_pipeline):   # the idle threads keep posting "nothing"
                while not pipeline.events.empty():
                    pipeline.events.get_nowait()
            return
        try:
            if self._refresh is not None:
                try:
                    catalog = self._refresh.events.get_nowait()
                except queue.Empty:
                    pass
                else:
                    self._catalog = catalog
                    self._pipeline.catalog_updates.put(catalog)
                    self._cards.clear()
                    self._hover, self._shown = None, None
            self._follow_mouse()
        except Exception:   # a UI glitch must never kill the overlay loop
            log.exception("overlay tick failed")
        try:
            self._follow_info_window()
        except Exception:
            log.exception("rank panel tick failed")

    def _follow_mouse(self) -> None:
        """Runs every tick: the card is placed from the live mouse position, never from stale detections."""
        while True:   # only the newest pipeline result matters
            try:
                event = self._pipeline.events.get_nowait()
            except queue.Empty:
                break
            if event is not None:   # misses are judged by time below, not by a single frame
                self._hover, self._seen_at = event, time.monotonic()
        hover = self._hover
        if hover is not None:
            hover = replace(hover, item=self._catalog.lookup(hover.title))
        if hover is None or time.monotonic() - self._seen_at > config.HIDE_GRACE_S:
            self._hover, self._shown = None, None
            self._window.hide()
            return

        now = time.monotonic()
        if self._shown is None or self._shown[0] != hover.title:
            self._since = now   # a new item: start with the glance card
        # Progressive disclosure: the glance card first, the full one once the pointer stays on the item.
        expanded = now - self._since >= self._controls.current.expand_delay_s
        card = self._card_for(hover, expanded)
        cx, cy = game_window.cursor_pos()
        tooltip_y = cy + round(config.TOOLTIP_OFFSET[1] * hover.scale)
        if hover.pushed_up_height is not None:   # the game keeps a tall tooltip's bottom inside the client area
            tooltip_y = min(tooltip_y, hover.bounds.bottom - hover.pushed_up_height)
        tooltip = Box(cx + round(config.TOOLTIP_OFFSET[0] * hover.scale), tooltip_y, *hover.tooltip_size)
        gap = round(config.CARD_GAP_PX * hover.scale)
        x, y = place_card(tooltip, card.size, gap, hover.bounds, compare=hover.compare)
        if self._shown != (hover.title, expanded) or not self._window.visible:
            self._window.set_image(card, x, y)
            self._shown = (hover.title, expanded)
        else:
            self._window.move(x, y)

    def _card_for(self, hover: Hover, expanded: bool) -> Image.Image:
        """Rendering takes a few ms, so each item's card is drawn once and kept."""
        key = (hover.title, expanded, hover.scale, hover.compare)
        if key in self._cards:
            self._cards.move_to_end(key)
        else:
            self._cards[key] = render_card(hover.item, hover.title, self._catalog.generated_at, expanded=expanded,
                                           ui_scale=hover.scale, wide=hover.compare)
            if len(self._cards) > _CARD_CACHE_SIZE:
                self._cards.popitem(last=False)
        return self._cards[key]

    def _follow_info_window(self) -> None:
        """Keeps the rank panel glued to the Character Info window, which the player can drag around."""
        while True:
            try:
                event = self._info_pipeline.events.get_nowait()
            except queue.Empty:
                break
            if event is not None:
                self._info, self._info_seen_at = event, time.monotonic()
        info = self._info
        if info is None or info.name is None or time.monotonic() - self._info_seen_at > config.INFO_HIDE_GRACE_S:
            self._info, self._rank_shown = None, None
            self._rank_window.hide()
            return
        result = self._ranks.get(info.name)
        card = self._rank_card_for(info.name, result, info.scale)
        pos = place_beside_window(info.box, card.size, round(config.INFO_CARD_GAP_PX * info.scale), info.bounds)
        if pos is None:   # no room inside the game: show nothing rather than float outside it
            self._rank_shown = None
            self._rank_window.hide()
        elif self._rank_shown != (info.name, result, info.scale) or not self._rank_window.visible:
            self._rank_window.set_image(card, *pos)
            self._rank_shown = (info.name, result, info.scale)
        else:
            self._rank_window.move(*pos)

    def _rank_card_for(self, name: str, result: RankResult, scale: float) -> Image.Image:
        key = (name, result, scale)
        if key in self._rank_cards:
            self._rank_cards.move_to_end(key)
        else:
            self._rank_cards[key] = render_rank_card(result, ui_scale=scale)
            if len(self._rank_cards) > _RANK_CARD_CACHE_SIZE:
                self._rank_cards.popitem(last=False)
        return self._rank_cards[key]
