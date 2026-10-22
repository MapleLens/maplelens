"""A per-pixel-alpha, click-through, always-on-top Win32 window.

tkinter can only fade a whole window; the game's tooltip is translucent while its text stays solid.
That needs a layered window fed an RGBA bitmap (UpdateLayeredWindow), so this talks to Win32 directly.
"""

import ctypes
from ctypes import wintypes

from PIL import Image

from maplelens.geometry import Box

user32, gdi32 = ctypes.windll.user32, ctypes.windll.gdi32

_WS_POPUP = 0x80000000
_WS_EX_TOPMOST, _WS_EX_TOOLWINDOW, _WS_EX_LAYERED = 0x8, 0x80, 0x80000
_WS_EX_TRANSPARENT, _WS_EX_NOACTIVATE = 0x20, 0x08000000   # clicks fall through; never takes focus
_SW_HIDE, _SW_SHOWNOACTIVATE = 0, 4
_SWP_NOSIZE, _SWP_NOACTIVATE, _SWP_SHOWWINDOW = 0x1, 0x10, 0x40
_HWND_TOPMOST = wintypes.HWND(-1)
_ULW_ALPHA, _AC_SRC_OVER, _AC_SRC_ALPHA = 2, 0, 1
_BI_RGB, _DIB_RGB_COLORS = 0, 0


class _BitmapInfoHeader(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
                ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
                ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG),
                ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD),
                ("biClrImportant", wintypes.DWORD)]


class _BlendFunction(ctypes.Structure):
    _fields_ = [("BlendOp", ctypes.c_ubyte), ("BlendFlags", ctypes.c_ubyte),
                ("SourceConstantAlpha", ctypes.c_ubyte), ("AlphaFormat", ctypes.c_ubyte)]


class _Msg(ctypes.Structure):
    _fields_ = [("hwnd", wintypes.HWND), ("message", wintypes.UINT), ("wParam", wintypes.WPARAM),
                ("lParam", wintypes.LPARAM), ("time", wintypes.DWORD), ("pt", wintypes.POINT)]


def _declare() -> None:
    """Explicit signatures: ctypes would otherwise pass 64-bit handles as 32-bit ints."""
    user32.CreateWindowExW.restype = wintypes.HWND
    user32.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
                                       *[ctypes.c_int] * 4, wintypes.HWND, wintypes.HMENU, wintypes.HINSTANCE,
                                       wintypes.LPVOID]
    user32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, *[ctypes.c_int] * 4, wintypes.UINT]
    user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
    user32.DestroyWindow.argtypes = [wintypes.HWND]
    user32.GetDC.restype = wintypes.HDC
    user32.GetDC.argtypes = [wintypes.HWND]
    user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
    user32.UpdateLayeredWindow.argtypes = [wintypes.HWND, wintypes.HDC, ctypes.POINTER(wintypes.POINT),
                                           ctypes.POINTER(wintypes.SIZE), wintypes.HDC,
                                           ctypes.POINTER(wintypes.POINT), wintypes.COLORREF,
                                           ctypes.POINTER(_BlendFunction), wintypes.DWORD]
    user32.PeekMessageW.argtypes = [ctypes.POINTER(_Msg), wintypes.HWND, wintypes.UINT, wintypes.UINT,
                                    wintypes.UINT]
    user32.DispatchMessageW.argtypes = [ctypes.POINTER(_Msg)]
    gdi32.CreateCompatibleDC.restype = wintypes.HDC
    gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
    gdi32.CreateDIBSection.restype = wintypes.HBITMAP
    gdi32.CreateDIBSection.argtypes = [wintypes.HDC, ctypes.c_void_p, wintypes.UINT,
                                       ctypes.POINTER(ctypes.c_void_p), wintypes.HANDLE, wintypes.DWORD]
    gdi32.SelectObject.restype = wintypes.HGDIOBJ
    gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]
    gdi32.DeleteObject.argtypes = [wintypes.HGDIOBJ]
    gdi32.DeleteDC.argtypes = [wintypes.HDC]


_declare()


def pump_messages() -> None:
    """Handle pending window messages so Windows does not consider the overlay unresponsive."""
    msg = _Msg()
    while user32.PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
        user32.DispatchMessageW(ctypes.byref(msg))


class LayeredWindow:
    def __init__(self) -> None:
        style = _WS_EX_TOPMOST | _WS_EX_TOOLWINDOW | _WS_EX_LAYERED | _WS_EX_TRANSPARENT | _WS_EX_NOACTIVATE
        # "Static" is a built-in window class, so no window procedure has to be registered.
        self._hwnd = user32.CreateWindowExW(style, "Static", "MapleLens", _WS_POPUP, 0, 0, 1, 1,
                                            None, None, None, None)
        if not self._hwnd:
            raise OSError("could not create the overlay window")
        self._rect: Box | None = None
        self._visible = False

    @property
    def rect(self) -> Box | None:
        return self._rect

    @property
    def visible(self) -> bool:
        return self._visible

    def set_image(self, image: Image.Image, x: int, y: int) -> None:
        """Show `image` (RGBA, straight alpha) with its top-left at screen (x, y)."""
        width, height = image.size
        # The window wants premultiplied BGRA.
        r, g, b, a = image.split()
        premult = Image.merge("RGBA", [Image.composite(c, Image.new("L", c.size, 0), a) for c in (b, g, r)] + [a])
        header = _BitmapInfoHeader(ctypes.sizeof(_BitmapInfoHeader), width, -height, 1, 32, _BI_RGB, 0, 0, 0, 0, 0)
        screen_dc, memory_dc = user32.GetDC(None), gdi32.CreateCompatibleDC(None)
        bits = ctypes.c_void_p()
        bitmap = gdi32.CreateDIBSection(memory_dc, ctypes.byref(header), _DIB_RGB_COLORS, ctypes.byref(bits), None, 0)
        try:
            ctypes.memmove(bits, premult.tobytes(), width * height * 4)
            previous = gdi32.SelectObject(memory_dc, bitmap)
            blend = _BlendFunction(_AC_SRC_OVER, 0, 255, _AC_SRC_ALPHA)
            user32.UpdateLayeredWindow(self._hwnd, screen_dc, ctypes.byref(wintypes.POINT(x, y)),
                                       ctypes.byref(wintypes.SIZE(width, height)), memory_dc,
                                       ctypes.byref(wintypes.POINT(0, 0)), 0, ctypes.byref(blend), _ULW_ALPHA)
            gdi32.SelectObject(memory_dc, previous)
        finally:
            gdi32.DeleteObject(bitmap)
            gdi32.DeleteDC(memory_dc)
            user32.ReleaseDC(None, screen_dc)
        self._rect = Box(x, y, width, height)
        self._show()

    def move(self, x: int, y: int) -> None:
        """Reposition without touching the bitmap (cheap: this runs every frame while the mouse moves)."""
        if self._rect is None or (x, y) == (self._rect.x, self._rect.y):
            return
        user32.SetWindowPos(self._hwnd, _HWND_TOPMOST, x, y, 0, 0, _SWP_NOSIZE | _SWP_NOACTIVATE)
        self._rect = Box(x, y, self._rect.w, self._rect.h)
        self._show()

    def hide(self) -> None:
        if self._visible:
            user32.ShowWindow(self._hwnd, _SW_HIDE)
            self._visible = False

    def close(self) -> None:
        user32.DestroyWindow(self._hwnd)

    def _show(self) -> None:
        if not self._visible:
            user32.ShowWindow(self._hwnd, _SW_SHOWNOACTIVATE)
            self._visible = True
