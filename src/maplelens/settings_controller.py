"""Owns the current Settings: persists every change and keeps the Windows startup entry in step."""

import logging
from collections.abc import Callable
from dataclasses import replace
from pathlib import Path

from maplelens import settings_store, startup
from maplelens.settings_store import Settings

log = logging.getLogger(__name__)


class SettingsController:
    """`on_change(old, new)` is for whatever the app has to do besides saving (hotkey, overlays, ...)."""

    def __init__(self, path: Path, on_change: Callable[[Settings, Settings], None]) -> None:
        self._path, self._on_change = path, on_change
        self.current = settings_store.load(path)
        self._sync_startup(self.current.launch_at_startup)   # also repairs the entry if the exe was moved

    def update(self, new: Settings) -> None:
        if new == self.current:
            return
        old, self.current = self.current, new
        try:
            settings_store.save(new, self._path)
        except OSError:
            log.exception("could not save settings to %s", self._path)
        if new.launch_at_startup != old.launch_at_startup:
            self._sync_startup(new.launch_at_startup)
        self._on_change(old, new)

    def toggle_overlay(self) -> None:
        self.update(replace(self.current, overlay_enabled=not self.current.overlay_enabled))

    @staticmethod
    def _sync_startup(enabled: bool) -> None:
        try:
            startup.set_enabled(enabled)
        except OSError:
            log.exception("could not update the Windows startup entry")
