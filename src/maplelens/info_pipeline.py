"""Background loop: find the Character Info window and read whose it is."""

import logging
import queue
import threading
import time
from dataclasses import dataclass

import cv2

from maplelens import config
from maplelens.capture import game_window
from maplelens.capture.frame import ScreenGrabber
from maplelens.geometry import Box, ui_scale
from maplelens.vision import info_window
from maplelens.vision.ocr import TitleReader

log = logging.getLogger(__name__)


@dataclass(frozen=True)
class InfoWindow:
    """The Character Info window is open on screen."""

    name: str | None   # whose it is; None while the name plate is unreadable
    box: Box           # the window in screen coordinates
    bounds: Box        # game client area in screen coordinates
    scale: float = 1.0   # UI size relative to the measured resolution


class InfoPipeline(threading.Thread):
    """Publishes `InfoWindow` (or None when there is none to decorate) to `events`; the UI thread drains it."""

    def __init__(self) -> None:
        super().__init__(daemon=True, name="info-pipeline")
        self.events: queue.Queue[InfoWindow | None] = queue.Queue()
        self.enabled = threading.Event()   # cleared by the overlay toggle: the thread then idles without grabbing
        self.enabled.set()

    def run(self) -> None:
        grabber, reader = ScreenGrabber(), TitleReader()
        hwnd: int | None = None
        window: Box | None = None
        while True:
            if hwnd is None or not game_window.is_alive(hwnd):
                hwnd, window = game_window.find_game_window(), None
            if hwnd is None or not self.enabled.is_set() or not game_window.is_foreground(hwnd):
                window = None
                self.events.put(None)   # never draw over other apps
                time.sleep(config.GAME_LOST_RETRY_S if hwnd is None else config.POLL_BACKGROUND_S)
                continue
            bounds = game_window.client_box(hwnd)
            scale = ui_scale(bounds)
            if window is not None and not self._still_there(grabber, window, scale):
                window = None
            if window is None:
                window = self._search(grabber, bounds, scale)
            if window is None:
                self.events.put(None)
                time.sleep(config.INFO_SEARCH_S)
                continue
            plate = grabber.grab(info_window.name_box(window, scale))
            if abs(scale - 1) > 0.01:   # the name thresholds and OCR upscale are tuned for measured-size pixels
                plate = cv2.resize(plate, None, fx=1 / scale, fy=1 / scale, interpolation=cv2.INTER_AREA)
            name = info_window.clean_name(reader.read_mask(info_window.name_mask(plate)))
            self.events.put(InfoWindow(name, window, bounds, scale))
            time.sleep(config.INFO_POLL_S)

    @staticmethod
    def _still_there(grabber: ScreenGrabber, window: Box, scale: float) -> bool:
        title = grabber.grab(info_window.title_box(window, scale))
        return info_window.is_window(cv2.cvtColor(title, cv2.COLOR_BGR2GRAY), scale)

    @staticmethod
    def _search(grabber: ScreenGrabber, bounds: Box, scale: float) -> Box | None:
        gray = cv2.cvtColor(grabber.grab(bounds), cv2.COLOR_BGR2GRAY)
        found = info_window.find_window(gray, scale)
        if found is None:
            return None
        size = (round(v * scale) for v in config.INFO_WINDOW_SIZE)
        return Box(bounds.x + found[0], bounds.y + found[1], *size)
