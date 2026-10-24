"""What the update machinery tells the UI."""

from dataclasses import dataclass
from enum import Enum

from maplelens.update.manifest import Manifest


@dataclass(frozen=True)
class Update:
    version: str
    manifest: Manifest
    url: str        # where the zip comes from
    page_url: str   # human release page, for installs that cannot update themselves


class Phase(Enum):
    IDLE = "idle"
    CHECKING = "checking"
    UP_TO_DATE = "up to date"
    AVAILABLE = "available"
    DOWNLOADING = "downloading"
    FAILED = "failed"


@dataclass(frozen=True)
class UpdateState:
    phase: Phase = Phase.IDLE
    update: Update | None = None   # kept through DOWNLOADING and FAILED so the player can retry
    detail: str = ""               # error text, or download progress such as "42%"
