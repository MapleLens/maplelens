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
