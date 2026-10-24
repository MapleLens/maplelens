"""Monster and NPC sprites, downloaded when first needed and kept in a local folder."""

import logging
import os
from collections.abc import Callable
from pathlib import Path

from PIL import Image, UnidentifiedImageError

from maplelens.mapinfo.client import MapDataError, fetch_icon

log = logging.getLogger(__name__)


def default_dir() -> Path:
    base = os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local"
    return Path(base) / "MapleLens" / "cache" / "icons"


class IconCache:
    """`get` never touches the network: it answers from memory or disk, `download` fills the folder."""

    def __init__(self, folder: Path, fetch: Callable[[str, int], bytes] = fetch_icon) -> None:
        self._folder, self._fetch = folder, fetch
        self._memory: dict[tuple[str, int], Image.Image] = {}

    def _path(self, kind: str, entity_id: int) -> Path:
        return self._folder / kind / f"{entity_id}.png"

    def has(self, kind: str, entity_id: int) -> bool:
        return self._path(kind, entity_id).exists()

    def get(self, kind: str, entity_id: int) -> Image.Image | None:
        key = (kind, entity_id)
        if key not in self._memory:
            try:
                with Image.open(self._path(kind, entity_id)) as image:
                    self._memory[key] = image.convert("RGBA")
            except (OSError, UnidentifiedImageError):
                return None
        return self._memory[key]

    def download(self, kind: str, entity_id: int) -> bool:
        """Fetch a missing sprite; True when it is on disk afterwards."""
        path = self._path(kind, entity_id)
        if path.exists():
            return True
        try:
            data = self._fetch(kind, entity_id)
            path.parent.mkdir(parents=True, exist_ok=True)
            tmp = path.with_suffix(".tmp")   # write then rename: a crash never leaves half a sprite
            tmp.write_bytes(data)
            tmp.replace(path)
            return True
        except (MapDataError, OSError):
            log.warning("could not fetch %s %s", kind, entity_id, exc_info=True)
            return False
