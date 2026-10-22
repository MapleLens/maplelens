"""Load a locally cached MesoWatch snapshot."""

import json
from dataclasses import dataclass
from pathlib import Path

from maplelens.market.models import Item


class SnapshotError(Exception):
    pass


@dataclass(frozen=True)
class Snapshot:
    items: list[Item]
    generated_at: str | None   # ISO-8601 UTC time MesoWatch exported the data


def load_snapshot(path: Path) -> Snapshot:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
        return Snapshot([Item.from_raw(raw) for raw in document["items"]], document.get("generated_at"))
    except FileNotFoundError as exc:
        raise SnapshotError(f"snapshot not found: {path}") from exc
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as exc:
        raise SnapshotError(f"{path} is not a MesoWatch list.json ({exc!r})") from exc
