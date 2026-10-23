"""Real-window test: the overlay must land where asked and stay hidden when told to."""

import pytest
import win32gui
from PIL import Image

from maplelens.overlay.layered import LayeredWindow, pump_messages


@pytest.fixture
def window():
    win = LayeredWindow()
    yield win
    win.close()


def rect(win):
    pump_messages()
    return win32gui.GetWindowRect(win._hwnd)


def test_places_moves_resizes_and_hides(window):
    window.set_image(Image.new("RGBA", (200, 100), (58, 62, 120, 205)), 600, 400)
    assert rect(window) == (600, 400, 800, 500)
    assert win32gui.IsWindowVisible(window._hwnd)
    window.move(700, 450)
    assert rect(window) == (700, 450, 900, 550)
    window.set_image(Image.new("RGBA", (300, 150), (255, 0, 0, 128)), 100, 120)
    assert rect(window) == (100, 120, 400, 270)
    window.hide()
    assert not win32gui.IsWindowVisible(window._hwnd)
    window.set_image(Image.new("RGBA", (50, 50), (0, 0, 0, 255)), 10, 10)   # shown again after a hide
    assert win32gui.IsWindowVisible(window._hwnd)


def test_overlay_is_invisible_to_screen_capture(window):
    """Detection grabs the screen: if it saw the card it would read the card instead of the game."""
    import mss
    import numpy as np
    box = {"left": 400, "top": 300, "width": 200, "height": 100}
    with mss.mss() as grabber:
        before = np.array(grabber.grab(box))
        window.set_image(Image.new("RGBA", (200, 100), (255, 0, 255, 255)), 400, 300)
        pump_messages()
        import time
        time.sleep(0.3)
        after = np.array(grabber.grab(box))
    magenta = (after[..., 2] == 255) & (after[..., 1] == 0) & (after[..., 0] == 255)
    assert magenta.mean() < 0.5, "the overlay shows up in screen captures"
    assert before.shape == after.shape
