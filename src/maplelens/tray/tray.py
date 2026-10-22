"""The notification-area icon: right-click menu (Settings, Exit), double-click opens settings."""

import ctypes
from collections.abc import Callable
from ctypes import wintypes

from maplelens.tray.icon import load_icon
from maplelens.tray.winclass import create_window, user32

shell32 = ctypes.windll.shell32

_WM_APP_TRAY = 0x8000 + 1   # private message the shell sends us about mouse events on the icon
_WM_LBUTTONDBLCLK, _WM_RBUTTONUP, _WM_HOTKEY, _WM_NULL = 0x203, 0x205, 0x312, 0
_NIM_ADD, _NIM_DELETE = 0, 2
_NIF_MESSAGE, _NIF_ICON, _NIF_TIP = 0x1, 0x2, 0x4
_MF_STRING, _MF_SEPARATOR, _MF_CHECKED = 0, 0x800, 0x8
_TPM_RIGHTBUTTON, _TPM_RETURNCMD, _TPM_BOTTOMALIGN = 0x2, 0x100, 0x20
_ID_SETTINGS, _ID_EXIT, _ID_TOGGLE = 1, 2, 3
_HOTKEY_ID = 1
_MOD_ALT, _MOD_CONTROL, _MOD_SHIFT, _MOD_NOREPEAT = 0x1, 0x2, 0x4, 0x4000
_HOTKEYF_SHIFT, _HOTKEYF_CONTROL, _HOTKEYF_ALT = 0x1, 0x2, 0x4
_TIP = "MapleLens"


class _NotifyIconData(ctypes.Structure):
    _fields_ = [("cbSize", wintypes.DWORD), ("hWnd", wintypes.HWND), ("uID", wintypes.UINT),
                ("uFlags", wintypes.UINT), ("uCallbackMessage", wintypes.UINT), ("hIcon", wintypes.HICON),
                ("szTip", wintypes.WCHAR * 128), ("dwState", wintypes.DWORD), ("dwStateMask", wintypes.DWORD),
                ("szInfo", wintypes.WCHAR * 256), ("uVersion", wintypes.UINT), ("szInfoTitle", wintypes.WCHAR * 64),
                ("dwInfoFlags", wintypes.DWORD), ("guidItem", ctypes.c_byte * 16), ("hBalloonIcon", wintypes.HICON)]


shell32.Shell_NotifyIconW.argtypes = [wintypes.DWORD, ctypes.POINTER(_NotifyIconData)]
user32.CreatePopupMenu.restype = wintypes.HMENU
user32.AppendMenuW.argtypes = [wintypes.HMENU, wintypes.UINT, ctypes.c_size_t, wintypes.LPCWSTR]
user32.TrackPopupMenu.argtypes = [wintypes.HMENU, wintypes.UINT, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                  wintypes.HWND, wintypes.LPVOID]
user32.DestroyMenu.argtypes = [wintypes.HMENU]
user32.SetForegroundWindow.argtypes = [wintypes.HWND]
user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.GetCursorPos.argtypes = [ctypes.POINTER(wintypes.POINT)]
user32.RegisterWindowMessageW.argtypes = [wintypes.LPCWSTR]
user32.RegisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int, wintypes.UINT, wintypes.UINT]
user32.UnregisterHotKey.argtypes = [wintypes.HWND, ctypes.c_int]


def register_modifiers(packed: int) -> int:
    """Hotkey-control flags (HOTKEYF_*) to RegisterHotKey flags (MOD_*): the two use different bit values."""
    flags = packed >> 8
    return (_MOD_ALT * bool(flags & _HOTKEYF_ALT) | _MOD_CONTROL * bool(flags & _HOTKEYF_CONTROL)
            | _MOD_SHIFT * bool(flags & _HOTKEYF_SHIFT) | _MOD_NOREPEAT)


class TrayIcon:
    """Messages arrive while the main loop pumps them, so the callbacks run on the main thread."""

    def __init__(self, on_settings: Callable[[], None], on_exit: Callable[[], None],
                 on_toggle: Callable[[], None]) -> None:
        self._on_settings, self._on_exit, self._on_toggle = on_settings, on_exit, on_toggle
        self.overlay_enabled = True   # the check mark in front of "Show overlays"
        self._hotkey = False
        self._icon = load_icon()
        # The shell broadcasts this when explorer restarts and the tray is rebuilt: our icon must be re-added.
        self._taskbar_created = user32.RegisterWindowMessageW("TaskbarCreated")
        self._hwnd, self._proc = create_window("MapleLensTray", "MapleLens", self._on_message, 0, (0, 0, 0, 0))
        self._add()

    def set_hotkey(self, packed: int) -> bool:
        """Registers the global toggle hotkey (0 clears it). False if another program already owns the combo."""
        if self._hotkey:
            user32.UnregisterHotKey(self._hwnd, _HOTKEY_ID)
            self._hotkey = False
        if not packed & 0xFF:
            return True
        self._hotkey = bool(user32.RegisterHotKey(self._hwnd, _HOTKEY_ID, register_modifiers(packed),
                                                  packed & 0xFF))
        return self._hotkey

    def close(self) -> None:
        self.set_hotkey(0)
        shell32.Shell_NotifyIconW(_NIM_DELETE, ctypes.byref(self._data()))
        user32.DestroyWindow(self._hwnd)

    def _data(self) -> _NotifyIconData:
        data = _NotifyIconData()
        data.cbSize, data.hWnd, data.uID = ctypes.sizeof(_NotifyIconData), self._hwnd, 1
        data.uFlags = _NIF_MESSAGE | _NIF_ICON | _NIF_TIP
        data.uCallbackMessage, data.hIcon, data.szTip = _WM_APP_TRAY, self._icon, _TIP
        return data

    def _add(self) -> None:
        shell32.Shell_NotifyIconW(_NIM_ADD, ctypes.byref(self._data()))

    def _on_message(self, hwnd: int, msg: int, wparam: int, lparam: int) -> int | None:
        if msg == _WM_APP_TRAY:
            if lparam == _WM_RBUTTONUP:
                self._show_menu()
            elif lparam == _WM_LBUTTONDBLCLK:
                self._on_settings()
            return 0
        if msg == _WM_HOTKEY:
            self._on_toggle()
            return 0
        if msg == self._taskbar_created:
            self._add()
            return 0
        return None

    def _show_menu(self) -> None:
        menu = user32.CreatePopupMenu()
        user32.AppendMenuW(menu, _MF_STRING | _MF_CHECKED * self.overlay_enabled, _ID_TOGGLE, "Show overlays")
        user32.AppendMenuW(menu, _MF_STRING, _ID_SETTINGS, "Settings")
        user32.AppendMenuW(menu, _MF_SEPARATOR, 0, None)
        user32.AppendMenuW(menu, _MF_STRING, _ID_EXIT, "Exit")
        point = wintypes.POINT()
        user32.GetCursorPos(ctypes.byref(point))
        # Windows only dismisses a tray menu on an outside click if our window is foreground when it opens.
        user32.SetForegroundWindow(self._hwnd)
        choice = user32.TrackPopupMenu(menu, _TPM_RETURNCMD | _TPM_RIGHTBUTTON | _TPM_BOTTOMALIGN,
                                       point.x, point.y, 0, self._hwnd, None)
        user32.PostMessageW(self._hwnd, _WM_NULL, 0, 0)   # documented workaround for the menu sticking around
        user32.DestroyMenu(menu)
        if choice == _ID_TOGGLE:
            self._on_toggle()
        elif choice == _ID_SETTINGS:
            self._on_settings()
        elif choice == _ID_EXIT:
            self._on_exit()
