"""Maps already looked up, kept on disk so a hover is instant (and works offline) next time."""

import json
import logging
from dataclasses import asdict
from pathlib import Path

from maplelens.mapinfo import resolve
from maplelens.mapinfo.icons import default_dir as _icons_dir
from maplelens.mapinfo.models import Location, MapInfo, Mob, Npc

log = logging.getLogger(__name__)

_VERSION = 2   # bump when MapInfo changes shape: older files are then ignored and fetched again


def default_dir() -> Path:
    return _icons_dir().parent / "maps"


class MapStore:
    """`maps/<id>.json` per map plus `index.json` from title readings to map ids."""

    def __init__(self, folder: Path) -> None:
        self._folder = folder
        self._index: dict[str, int] | None = None

    def find(self, location: Location) -> int | None:
        return self._load_index().get(resolve.key(location))

    def remember(self, location: Location, map_id: int) -> None:
        index = self._load_index()
        if index.get(resolve.key(location)) != map_id:
            index[resolve.key(location)] = map_id
            self._write(self._folder / "index.json", index)

    def load(self, map_id: int) -> MapInfo | None:
        try:
            raw = json.loads((self._folder / f"{map_id}.json").read_text(encoding="utf-8"))
            if raw["version"] != _VERSION:
                return None
            return MapInfo(raw["id"], raw["name"], raw["street"], raw["is_town"],
                           tuple(Mob(**m) for m in raw["mobs"]), tuple(Npc(**n) for n in raw["npcs"]))
        except FileNotFoundError:
            return None
        except (OSError, ValueError, KeyError, TypeError):
            log.warning("ignoring unreadable cache for map %s", map_id, exc_info=True)
            return None

    def save(self, info: MapInfo) -> None:
        self._write(self._folder / f"{info.id}.json", {"version": _VERSION, **asdict(info)})

    def _load_index(self) -> dict[str, int]:
        if self._index is None:
            try:
                self._index = {str(k): int(v) for k, v in
                               json.loads((self._folder / "index.json").read_text(encoding="utf-8")).items()}
            except (OSError, ValueError, TypeError):
                self._index = {}
        return self._index

    @staticmethod
    def _write(path: Path, data: object) -> None:
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            tmp = path.with_suffix(".tmp")   # write then rename: a crash never leaves half a file
            tmp.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
            tmp.replace(path)
        except OSError:
            log.warning("could not write %s", path, exc_info=True)
