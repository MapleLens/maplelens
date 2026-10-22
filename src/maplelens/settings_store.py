"""User settings, kept as JSON in %LOCALAPPDATA%\MapleLens (per user, no admin rights, machine specific)."""

import json
import logging
import os
from dataclasses import asdict, dataclass, fields
from pathlib import Path

log = logging.getLogger(__name__)

# Win32 hotkey-control encoding: virtual key in the low byte, HOTKEYF_* modifiers (shift 1, ctrl 2, alt 4) above it.
DEFAULT_HOTKEY = (0x02 | 0x04) << 8 | ord("M")   # Ctrl+Alt+M
EXPAND_DELAY_RANGE_S = (0.0, 3.0)


@dataclass(frozen=True)
class Settings:
    launch_at_startup: bool = False
    overlay_enabled: bool = True
    toggle_hotkey: int = DEFAULT_HOTKEY   # 0 = no hotkey
    expand_delay_s: float = 1.2           # how long the pointer rests on an item before the glance card expands


def default_path() -> Path:
    base = os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local"
    return Path(base) / "MapleLens" / "settings.json"


def _valid(name: str, value: object) -> bool:
    if name == "expand_delay_s":
        return isinstance(value, int | float) and not isinstance(value, bool) \
            and EXPAND_DELAY_RANGE_S[0] <= value <= EXPAND_DELAY_RANGE_S[1]
    if name == "toggle_hotkey":
        return isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= 0xFFFF
    return isinstance(value, bool)


def load(path: Path) -> Settings:
    """A missing, unreadable or partly invalid file never stops the app: bad fields fall back to defaults."""
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return Settings()
    except (OSError, ValueError):
        log.warning("could not read %s, using defaults", path, exc_info=True)
        return Settings()
    if not isinstance(raw, dict):
        return Settings()
    known = {f.name for f in fields(Settings)}
    return Settings(**{k: v for k, v in raw.items() if k in known and _valid(k, v)})


def save(settings: Settings, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")   # write then rename so a crash never leaves half a file
    tmp.write_text(json.dumps(asdict(settings), indent=2), encoding="utf-8")
    tmp.replace(path)
