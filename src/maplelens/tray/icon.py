"""Loading the app icon from the bundled .ico."""

import ctypes
from ctypes import wintypes
from pathlib import Path

user32 = ctypes.windll.user32
user32.LoadImageW.restype = wintypes.HANDLE
user32.LoadImageW.argtypes = [wintypes.HINSTANCE, wintypes.LPCWSTR, wintypes.UINT, ctypes.c_int, ctypes.c_int,
                              wintypes.UINT]

ICON_PATH = Path(__file__).resolve().parent.parent / "assets" / "maplelens.ico"
_IMAGE_ICON, _LR_LOADFROMFILE = 1, 0x10
_SM_CXSMICON = 49


def load_icon(size: int | None = None) -> int:
    """HICON from the .ico; the default size is the system's small icon (tray / title bar)."""
    size = size or user32.GetSystemMetrics(_SM_CXSMICON)   # DPI aware: 16 at 100 %, 20 at 125 %, ...
    return user32.LoadImageW(None, str(ICON_PATH), _IMAGE_ICON, size, size, _LR_LOADFROMFILE) or 0
