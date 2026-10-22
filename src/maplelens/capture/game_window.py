"""Locate the MapleStory window and its client area on screen."""

import ctypes

import psutil
import win32api
import win32gui
import win32process

from maplelens.config import GAME_PROCESS
from maplelens.geometry import Box

# Without this, Windows reports scaled coordinates on HiDPI displays and the overlay lands in the wrong place.
ctypes.windll.user32.SetProcessDPIAware()


def _exe_name(hwnd: int) -> str:
    try:
        return psutil.Process(win32process.GetWindowThreadProcessId(hwnd)[1]).name().lower()
    except psutil.Error:
        return ""


def find_game_window() -> int | None:
    """Handle of the visible MapleStory window, or None when the game is not running."""
    found: list[int] = []

    def visit(hwnd: int, _: object) -> None:
        if win32gui.IsWindowVisible(hwnd) and _exe_name(hwnd) == GAME_PROCESS:
            found.append(hwnd)

    win32gui.EnumWindows(visit, None)
    return found[0] if found else None


def is_alive(hwnd: int) -> bool:
    return bool(win32gui.IsWindow(hwnd))


def client_box(hwnd: int) -> Box:
    """Game client area (no title bar or borders) in screen coordinates."""
    _, _, w, h = win32gui.GetClientRect(hwnd)
    x, y = win32gui.ClientToScreen(hwnd, (0, 0))
    return Box(x, y, w, h)


def is_foreground(hwnd: int) -> bool:
    """True while the game is the window the user is working in (not minimised, not behind another app)."""
    return win32gui.GetForegroundWindow() == hwnd and not win32gui.IsIconic(hwnd)


def cursor_pos() -> tuple[int, int]:
    return win32api.GetCursorPos()
