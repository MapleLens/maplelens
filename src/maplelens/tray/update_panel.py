"""The update rows of the settings window: current version, update status, Check now / Update now."""

from collections.abc import Callable
from ctypes import wintypes

from maplelens.tray.winclass import user32
from maplelens.update.model import Phase, UpdateState

_ID_VERSION, _ID_STATUS, _ID_CHECK, _ID_UPDATE = 110, 111, 112, 113
_WS_TABSTOP, _SS_ETCHEDHORZ = 0x10000, 0x10   # a thin separator line

user32.EnableWindow.argtypes = [wintypes.HWND, wintypes.BOOL]
user32.SetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPCWSTR]


def describe(state: UpdateState) -> tuple[str, bool, bool]:
    """(status line, Check now enabled, Update now enabled)."""
    version = state.update.version if state.update else ""
    text = {
        Phase.IDLE: "",
        Phase.CHECKING: "Checking for updates...",
        Phase.UP_TO_DATE: "You have the latest version.",
        Phase.AVAILABLE: f"Version {version} is available.",
        Phase.DOWNLOADING: f"Downloading version {version}... {state.detail}".strip(),
        Phase.FAILED: f"Update failed: {state.detail}" if state.update else f"Could not check: {state.detail}",
    }[state.phase]
    busy = state.phase in (Phase.CHECKING, Phase.DOWNLOADING)
    return text, not busy, state.update is not None and not busy


class UpdatePanel:
    """`create` adds the controls to a window; `show_state` can be called before or after that."""

    def __init__(self, version: str, self_update: bool, on_check: Callable[[], None],
                 on_update: Callable[[], None]) -> None:
        self._version, self._self_update = version, self_update
        self._on_check, self._on_update = on_check, on_update
        self._state = UpdateState()
        self._item: Callable[[int], int] | None = None

    def create(self, child: Callable[..., int], item: Callable[[int], int], top: int) -> None:
        """`child(cls, text, style, rect, id)` makes a control, `item(id)` finds it; `top` is the first row's y."""
        self._item = item
        child("Static", "", _SS_ETCHEDHORZ, (20, top, 380, 2))
        child("Static", f"MapleLens version {self._version}", 0, (20, top + 14, 380, 20), _ID_VERSION)
        child("Static", "", 0, (20, top + 38, 380, 20), _ID_STATUS)
        child("Button", "Check now", _WS_TABSTOP, (20, top + 66, 110, 26), _ID_CHECK)
        child("Button", "Update now" if self._self_update else "View release", _WS_TABSTOP,
              (140, top + 66, 110, 26), _ID_UPDATE)
        self.show_state(self._state)

    def show_state(self, state: UpdateState) -> None:
        self._state = state
        if self._item is None:
            return
        text, can_check, can_update = describe(state)
        user32.SetWindowTextW(self._item(_ID_STATUS), text)
        user32.EnableWindow(self._item(_ID_CHECK), can_check)
        user32.EnableWindow(self._item(_ID_UPDATE), can_update)

    def command(self, control: int) -> None:
        if control == _ID_CHECK:
            self._on_check()
        elif control == _ID_UPDATE:
            self._on_update()
