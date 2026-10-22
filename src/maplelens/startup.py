"""Launch at Windows sign-in through the per-user Run key (no admin rights; removing the value undoes it)."""

import contextlib
import sys
import winreg
from pathlib import Path

RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
VALUE_NAME = "MapleLens"


def launch_command() -> str:
    """The packaged exe starts itself; from source we start the module with pythonw so no console window pops up."""
    if getattr(sys, "frozen", False):
        return f'"{sys.executable}"'
    pythonw = Path(sys.executable).with_name("pythonw.exe")
    return f'"{pythonw if pythonw.exists() else sys.executable}" -m maplelens'


def is_enabled(name: str = VALUE_NAME) -> bool:
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
            winreg.QueryValueEx(key, name)
        return True
    except FileNotFoundError:
        return False


def set_enabled(enabled: bool, name: str = VALUE_NAME) -> None:
    # CreateKeyEx, not OpenKey: a fresh profile (e.g. a CI runner) has no Run key yet.
    with winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, RUN_KEY, 0, winreg.KEY_SET_VALUE) as key:
        if enabled:
            winreg.SetValueEx(key, name, 0, winreg.REG_SZ, launch_command())
        else:
            with contextlib.suppress(FileNotFoundError):
                winreg.DeleteValue(key, name)
