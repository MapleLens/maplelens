"""Screen capture."""

import cv2
import mss
import numpy as np

from maplelens.geometry import Box


class ScreenGrabber:
    """Reuses one mss handle; mss objects are bound to the thread that created them."""

    def __init__(self) -> None:
        self._mss = mss.mss()

    def grab(self, area: Box) -> np.ndarray:
        """BGR capture of a screen-space rectangle (contiguous and writable)."""
        shot = self._mss.grab({"left": area.x, "top": area.y, "width": area.w, "height": area.h})
        bgra = np.frombuffer(shot.bgra, np.uint8).reshape(shot.height, shot.width, 4)
        return cv2.cvtColor(bgra, cv2.COLOR_BGRA2BGR)
