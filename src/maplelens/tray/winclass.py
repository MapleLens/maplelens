"""Registering a window class with a Python window procedure, shared by the tray and settings windows."""

import ctypes
from collections.abc import Callable
from ctypes import wintypes

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

LRESULT = ctypes.c_ssize_t
WNDPROC = ctypes.WINFUNCTYPE(LRESULT, wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)
WindowProc = Callable[[int, int, int, int], int | None]   # (hwnd, msg, wparam, lparam) -> result, None = default


class _WndClass(ctypes.Structure):
    _fields_ = [("style", wintypes.UINT), ("lpfnWndProc", WNDPROC), ("cbClsExtra", ctypes.c_int),
                ("cbWndExtra", ctypes.c_int), ("hInstance", wintypes.HINSTANCE), ("hIcon", wintypes.HICON),
                ("hCursor", wintypes.HANDLE), ("hbrBackground", wintypes.HBRUSH), ("lpszMenuName", wintypes.LPCWSTR),
                ("lpszClassName", wintypes.LPCWSTR)]


user32.DestroyWindow.argtypes = [wintypes.HWND]
user32.DefWindowProcW.restype = LRESULT
user32.DefWindowProcW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.RegisterClassW.argtypes = [ctypes.POINTER(_WndClass)]
user32.LoadCursorW.restype = wintypes.HANDLE
user32.LoadCursorW.argtypes = [wintypes.HINSTANCE, ctypes.c_void_p]
user32.CreateWindowExW.restype = wintypes.HWND
user32.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
                                   *[ctypes.c_int] * 4, wintypes.HWND, wintypes.HMENU, wintypes.HINSTANCE,
                                   wintypes.LPVOID]
kernel32.GetModuleHandleW.restype = wintypes.HINSTANCE
kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]

_COLOR_BTNFACE = 15   # matches the Static labels placed on the window
_IDC_ARROW = 32512


def create_window(class_name: str, title: str, proc: WindowProc, style: int, rect: tuple[int, int, int, int],
                  icon: int = 0, parent: int = 0) -> tuple[int, WNDPROC]:
    """Returns (hwnd, c_proc). The caller must keep `c_proc` alive as long as the window exists."""

    def dispatch(hwnd: int, msg: int, wparam: int, lparam: int) -> int:
        result = proc(hwnd, msg, wparam, lparam)
        return user32.DefWindowProcW(hwnd, msg, wparam, lparam) if result is None else result

    c_proc = WNDPROC(dispatch)
    instance = kernel32.GetModuleHandleW(None)
    cls = _WndClass(0, c_proc, 0, 0, instance, icon, user32.LoadCursorW(None, _IDC_ARROW), _COLOR_BTNFACE + 1,
                    None, class_name)
    user32.RegisterClassW(ctypes.byref(cls))
    hwnd = user32.CreateWindowExW(0, class_name, title, style, *rect, parent, None, instance, None)
    if not hwnd:
        raise OSError(f"could not create the {class_name} window")
    return hwnd, c_proc
