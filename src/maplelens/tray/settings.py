"""The settings window: standard Win32 controls that apply as soon as they change."""

import ctypes
from collections.abc import Callable
from ctypes import wintypes
from dataclasses import replace

from maplelens.settings_store import EXPAND_DELAY_RANGE_S, Settings
from maplelens.tray.winclass import create_window, user32

_WS_CAPTION, _WS_SYSMENU, _WS_CHILD, _WS_VISIBLE, _WS_TABSTOP = 0xC00000, 0x80000, 0x40000000, 0x10000000, 0x10000
_BS_AUTOCHECKBOX, _TBS_AUTOTICKS, _WS_EX_CLIENTEDGE = 0x3, 0x1, 0x200
_WM_CLOSE, _WM_SETFONT, _WM_COMMAND, _WM_HSCROLL, _WM_USER = 0x10, 0x30, 0x111, 0x114, 0x400
_BM_GETCHECK, _BM_SETCHECK = 0xF0, 0xF1
_EN_CHANGE, _EN_SETFOCUS, _EN_KILLFOCUS = 0x300, 0x100, 0x200
_HKM_SETHOTKEY, _HKM_GETHOTKEY, _HKM_SETRULES = _WM_USER + 1, _WM_USER + 2, _WM_USER + 3
_TBM_GETPOS, _TBM_SETPOS, _TBM_SETRANGE = _WM_USER, _WM_USER + 5, _WM_USER + 6
_HKCOMB_NONE, _HKCOMB_S, _HOTKEYF_CTRL_ALT = 0x1, 0x2, 0x2 | 0x4
_ICC_BAR_CLASSES, _ICC_HOTKEY_CLASS = 0x4, 0x40
_SW_HIDE, _SW_SHOW = 0, 5
_DEFAULT_GUI_FONT = 17
_SIZE = (440, 300)
_STEPS_PER_S = 10   # the slider works in tenths of a second

_ID_STARTUP, _ID_OVERLAY, _ID_HOTKEY, _ID_CLEAR, _ID_DELAY, _ID_DELAY_TEXT = 101, 102, 103, 104, 105, 106


class _InitCommonControls(ctypes.Structure):
    _fields_ = [("dwSize", wintypes.DWORD), ("dwICC", wintypes.DWORD)]


user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
user32.SetForegroundWindow.argtypes = [wintypes.HWND]
user32.SendMessageW.restype = ctypes.c_ssize_t
user32.SendMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.SetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPCWSTR]
user32.GetDlgItem.restype = wintypes.HWND
user32.GetDlgItem.argtypes = [wintypes.HWND, ctypes.c_int]
user32.GetSystemMetrics.argtypes = [ctypes.c_int]
ctypes.windll.gdi32.GetStockObject.restype = wintypes.HANDLE


def delay_text(seconds: float) -> str:
    return "instantly" if seconds <= 0 else f"after {seconds:.1f} s"


class SettingsWindow:
    """Created lazily on first `show()`; closing the window hides it so it can be reopened.

    `on_change` gets the new Settings after every edit; `on_capture(True/False)` brackets the hotkey box having
    focus, so the app can release the global hotkey while the player types a new one.
    """

    def __init__(self, icon: int, on_change: Callable[[Settings], None],
                 on_capture: Callable[[bool], None]) -> None:
        self._icon, self._on_change, self._on_capture = icon, on_change, on_capture
        self._hwnd = 0
        self._proc = None   # keeps the ctypes callback alive
        self._current = Settings()
        self._loading = False   # filling the controls must not count as the user editing them

    def show(self, settings: Settings) -> None:
        if not self._hwnd:
            self._create()
        self._current = settings
        self._load(settings)
        user32.ShowWindow(self._hwnd, _SW_SHOW)
        user32.SetForegroundWindow(self._hwnd)

    def sync(self, settings: Settings) -> None:
        """Refresh the controls after a change made elsewhere (hotkey, tray menu); no-op while never opened."""
        self._current = settings
        if self._hwnd:
            self._load(settings)

    def close(self) -> None:
        if self._hwnd:
            user32.DestroyWindow(self._hwnd)
            self._hwnd = 0

    def _item(self, control_id: int) -> int:
        return user32.GetDlgItem(self._hwnd, control_id)

    def _child(self, cls: str, text: str, style: int, rect: tuple[int, int, int, int], control_id: int = 0,
               ex_style: int = 0) -> int:
        x, y, w, h = rect
        hwnd = user32.CreateWindowExW(ex_style, cls, text, _WS_CHILD | _WS_VISIBLE | style, x, y, w, h,
                                      self._hwnd, control_id, None, None)
        user32.SendMessageW(hwnd, _WM_SETFONT, ctypes.windll.gdi32.GetStockObject(_DEFAULT_GUI_FONT), 1)
        return hwnd

    def _create(self) -> None:
        ctypes.windll.comctl32.InitCommonControlsEx(
            ctypes.byref(_InitCommonControls(ctypes.sizeof(_InitCommonControls), _ICC_BAR_CLASSES | _ICC_HOTKEY_CLASS)))
        w, h = _SIZE
        x, y = (user32.GetSystemMetrics(0) - w) // 2, (user32.GetSystemMetrics(1) - h) // 2   # screen centre
        self._hwnd, self._proc = create_window("MapleLensSettings", "MapleLens settings", self._on_message,
                                               _WS_CAPTION | _WS_SYSMENU, (x, y, w, h), self._icon)
        self._child("Button", "Launch MapleLens when Windows starts", _BS_AUTOCHECKBOX | _WS_TABSTOP,
                    (20, 20, 380, 22), _ID_STARTUP)
        self._child("Button", "Show price overlays", _BS_AUTOCHECKBOX | _WS_TABSTOP, (20, 52, 380, 22), _ID_OVERLAY)
        self._child("Static", "Hotkey to show / hide the overlays:", 0, (20, 92, 380, 20))
        hotkey = self._child("msctls_hotkey32", "", _WS_TABSTOP, (20, 114, 230, 24), _ID_HOTKEY, _WS_EX_CLIENTEDGE)
        # No bare keys and no Shift-only combos: they would fire while typing in the game.
        user32.SendMessageW(hotkey, _HKM_SETRULES, _HKCOMB_NONE | _HKCOMB_S, _HOTKEYF_CTRL_ALT)
        self._child("Button", "Clear", _WS_TABSTOP, (260, 113, 70, 26), _ID_CLEAR)
        self._child("Static", "Expand the price card to full details:", 0, (20, 160, 380, 20))
        slider = self._child("msctls_trackbar32", "", _TBS_AUTOTICKS | _WS_TABSTOP, (20, 182, 290, 32), _ID_DELAY)
        top = round(EXPAND_DELAY_RANGE_S[1] * _STEPS_PER_S)
        user32.SendMessageW(slider, _TBM_SETRANGE, 1, top << 16)
        self._child("Static", "", 0, (320, 188, 100, 20), _ID_DELAY_TEXT)

    def _load(self, settings: Settings) -> None:
        self._loading = True
        try:
            user32.SendMessageW(self._item(_ID_STARTUP), _BM_SETCHECK, settings.launch_at_startup, 0)
            user32.SendMessageW(self._item(_ID_OVERLAY), _BM_SETCHECK, settings.overlay_enabled, 0)
            user32.SendMessageW(self._item(_ID_HOTKEY), _HKM_SETHOTKEY, settings.toggle_hotkey, 0)
            user32.SendMessageW(self._item(_ID_DELAY), _TBM_SETPOS, 1, round(settings.expand_delay_s * _STEPS_PER_S))
            user32.SetWindowTextW(self._item(_ID_DELAY_TEXT), delay_text(settings.expand_delay_s))
        finally:
            self._loading = False

    def _read(self) -> Settings:
        delay = user32.SendMessageW(self._item(_ID_DELAY), _TBM_GETPOS, 0, 0) / _STEPS_PER_S
        user32.SetWindowTextW(self._item(_ID_DELAY_TEXT), delay_text(delay))
        return replace(
            self._current,
            launch_at_startup=bool(user32.SendMessageW(self._item(_ID_STARTUP), _BM_GETCHECK, 0, 0)),
            overlay_enabled=bool(user32.SendMessageW(self._item(_ID_OVERLAY), _BM_GETCHECK, 0, 0)),
            toggle_hotkey=user32.SendMessageW(self._item(_ID_HOTKEY), _HKM_GETHOTKEY, 0, 0) & 0xFFFF,
            expand_delay_s=delay)

    def _edited(self) -> None:
        if self._loading:
            return
        self._current = self._read()
        self._on_change(self._current)

    def _on_message(self, hwnd: int, msg: int, wparam: int, lparam: int) -> int | None:
        if msg == _WM_CLOSE:
            user32.ShowWindow(hwnd, _SW_HIDE)
            return 0
        if msg == _WM_HSCROLL:   # the slider reports to its parent
            self._edited()
        elif msg == _WM_COMMAND:
            control, note = wparam & 0xFFFF, wparam >> 16
            if control == _ID_HOTKEY and note in (_EN_SETFOCUS, _EN_KILLFOCUS):
                self._on_capture(note == _EN_SETFOCUS)
            elif control == _ID_CLEAR:
                user32.SendMessageW(self._item(_ID_HOTKEY), _HKM_SETHOTKEY, 0, 0)
                self._edited()
            elif control in (_ID_STARTUP, _ID_OVERLAY) or (control == _ID_HOTKEY and note == _EN_CHANGE):
                self._edited()
        return None
