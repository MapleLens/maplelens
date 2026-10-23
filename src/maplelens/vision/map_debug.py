"""Debug-only OpenCV window: the scan area the way the map pipeline sees it."""

import cv2
import numpy as np

from maplelens.vision.map_tooltip import MapTooltip
from maplelens.vision.ocr import TextLine

WINDOW = "MapleLens map debug"
_GREEN, _RED, _YELLOW = (80, 255, 80), (60, 60, 255), (0, 220, 255)


class DebugView:
    """Shows scan image (OCR lines green, accepted tooltip red) above the text mask, with the verdict on top.

    cv2 windows belong to the thread that made them, so `show` must always be called from the same thread.
    """

    def __init__(self) -> None:
        self._placed = False

    def show(self, image: np.ndarray, mask: np.ndarray, lines: list[TextLine], found: MapTooltip | None,
             verdict: str) -> None:
        top = image.copy()
        for line in lines:
            cv2.rectangle(top, (line.x, line.y), (line.x + line.w, line.y + line.h), _GREEN, 1)
            cv2.putText(top, line.text[:40], (line.x, max(10, line.y - 3)), cv2.FONT_HERSHEY_PLAIN, 0.9, _GREEN, 1)
        if found is not None:
            b = found.box
            cv2.rectangle(top, (b.x, b.y), (b.right, b.bottom), _RED, 2)
        bottom = cv2.cvtColor(mask.astype(np.uint8) * 255, cv2.COLOR_GRAY2BGR)
        banner = np.zeros((22, image.shape[1], 3), np.uint8)
        cv2.putText(banner, verdict, (4, 15), cv2.FONT_HERSHEY_SIMPLEX, 0.45, _YELLOW, 1)
        self._display(np.vstack([banner, top, bottom]))

    def blank(self, verdict: str, width: int = 560) -> None:
        """Nothing was grabbed this time (cursor too close to the game's edge)."""
        banner = np.zeros((60, width, 3), np.uint8)
        cv2.putText(banner, verdict, (4, 30), cv2.FONT_HERSHEY_SIMPLEX, 0.45, _YELLOW, 1)
        self._display(banner)

    def _display(self, frame: np.ndarray) -> None:
        if not self._placed:   # the game covers the screen: stay on top, in a corner, so the window is seen
            cv2.namedWindow(WINDOW, cv2.WINDOW_AUTOSIZE)
            cv2.moveWindow(WINDOW, 20, 20)
            cv2.setWindowProperty(WINDOW, cv2.WND_PROP_TOPMOST, 1)
            self._placed = True
        cv2.imshow(WINDOW, frame)
        cv2.waitKey(1)
