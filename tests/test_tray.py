import ctypes
from ctypes import wintypes

from maplelens.settings_store import Settings
from maplelens.tray import tray as tray_module
from maplelens.tray.icon import ICON_PATH, load_icon
from maplelens.tray.settings import SettingsWindow
from maplelens.tray.tray import TrayIcon

user32 = ctypes.windll.user32
user32.SendMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
user32.IsWindowVisible.argtypes = [wintypes.HWND]
_WM_CLOSE, _WM_LBUTTONDBLCLK = 0x10, 0x203


def test_icon_file_loads_as_a_handle():
    assert ICON_PATH.exists()
    assert load_icon(32)


def test_double_click_on_the_tray_icon_opens_settings():
    calls = []
    tray = TrayIcon(lambda: calls.append("settings"), lambda: calls.append("exit"), lambda: calls.append("toggle"))
    try:
        user32.SendMessageW(tray._hwnd, tray_module._WM_APP_TRAY, 1, _WM_LBUTTONDBLCLK)
    finally:
        tray.close()
    assert calls == ["settings"]


def test_closing_the_settings_window_hides_it_and_it_can_reopen():
    window = SettingsWindow(load_icon(), lambda s: None, lambda c: None)
    try:
        window.show(Settings())
        assert user32.IsWindowVisible(window._hwnd)
        user32.SendMessageW(window._hwnd, _WM_CLOSE, 0, 0)
        assert not user32.IsWindowVisible(window._hwnd)
        window.show(Settings())
        assert user32.IsWindowVisible(window._hwnd)
    finally:
        window.close()


def test_hotkey_toggle_message_calls_back_and_set_hotkey_registers():
    calls = []
    tray = TrayIcon(lambda: None, lambda: None, lambda: calls.append("toggle"))
    try:
        tray._on_message(tray._hwnd, tray_module._WM_HOTKEY, 1, 0)   # Windows drops a WM_HOTKEY that was sent by hand
        assert calls == ["toggle"]
        assert tray.set_hotkey(0)
        assert tray.set_hotkey((0x02 | 0x04) << 8 | 0x7B)   # Ctrl+Alt+F12: rarely taken
        assert tray.set_hotkey(0)
    finally:
        tray.close()


def test_hotkey_control_flags_become_register_hotkey_flags():
    ctrl_alt = (0x02 | 0x04) << 8 | ord("M")
    assert tray_module.register_modifiers(ctrl_alt) == 0x2 | 0x1 | 0x4000   # MOD_CONTROL | MOD_ALT | NOREPEAT
    assert tray_module.register_modifiers(0x01 << 8 | 0x70) == 0x4 | 0x4000   # Shift+F1
