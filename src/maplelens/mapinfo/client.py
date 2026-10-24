"""HTTP client for maplestory.io (map, monster and NPC data plus their sprites)."""

import json
import logging
import re
import urllib.error
import urllib.request
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from urllib.parse import quote

from maplelens import config
from maplelens.mapinfo import resolve
from maplelens.mapinfo.models import Location, MapInfo, Mob, Npc

log = logging.getLogger(__name__)


class MapDataError(Exception):
    """The lookup failed for a reason worth retrying later (network, bad response)."""


def _get(path: str) -> bytes:
    request = urllib.request.Request(f"{config.MAP_API}/{path}", headers={"User-Agent": "MapleLens/0.1"})
    try:
        with urllib.request.urlopen(request, timeout=config.MAP_REQUEST_TIMEOUT_S) as response:
            return response.read()
    except urllib.error.HTTPError as exc:
        raise MapDataError(f"HTTP {exc.code} for {path}") from exc
    except (urllib.error.URLError, TimeoutError) as exc:
        raise MapDataError(f"{exc} for {path}") from exc


def _get_json(path: str) -> dict | list:
    try:
        return json.loads(_get(path))
    except ValueError as exc:
        raise MapDataError(f"bad JSON for {path}") from exc


def find_map_id(location: Location) -> int | None:
    """Map id for a tooltip reading: search by the whole name, then by its words (one may be misread), and pick
    from everything found so far."""
    found: dict[int, dict] = {}
    for term in resolve.search_terms(location):
        for candidate in _get_json(f"map?searchFor={quote(term)}&count=100"):
            found[candidate["id"]] = candidate
        map_id = resolve.pick(location, found.values())
        if map_id is not None:
            return map_id
    return None


def fetch_icon(kind: str, entity_id: int) -> bytes:
    """PNG sprite of a monster ("mob") or NPC ("npc")."""
    return _get(f"{kind}/{entity_id}/icon")


def fetch_map(map_id: int) -> MapInfo:
    """Map with its monsters and NPCs; per-monster stats and NPC roles are separate requests."""
    raw = _get_json(f"map/{map_id}")
    try:
        counts = Counter(m["id"] for m in raw["mobs"])
        npc_ids = list(dict.fromkeys(n["id"] for n in raw["npcs"]))
        with ThreadPoolExecutor(max_workers=8) as pool:   # one request each: sequentially it takes seconds
            mob_jobs = [pool.submit(_mob, mob_id, n) for mob_id, n in counts.most_common()]
            npc_jobs = [pool.submit(_npc, npc_id) for npc_id in npc_ids]
            mobs = [m for m in (job.result() for job in mob_jobs) if m]
            npcs = [n for n in (job.result() for job in npc_jobs) if n]
        return MapInfo(map_id, raw["name"].strip(), raw["streetName"].strip(), bool(raw["isTown"]),
                       tuple(mobs), tuple(sorted(npcs, key=_npc_rank)))
    except (KeyError, TypeError) as exc:
        raise MapDataError(f"unexpected map response: {exc!r}") from exc


_SEASONAL = re.compile(r"event|thanksgiving|easter|christmas|halloween|valentine|holiday|santa|pie advocate",
                       re.IGNORECASE)


def _npc_rank(npc: Npc) -> tuple[bool, bool]:
    """Useful NPCs first: ones with a role, seasonal-event ones last (the data set carries old events)."""
    return bool(_SEASONAL.search(npc.role)), not npc.role


def _mob(mob_id: int, count: int) -> Mob | None:
    try:
        raw = _get_json(f"mob/{mob_id}")
        meta = raw["meta"]
        return Mob(mob_id, raw["name"], meta["level"], meta["exp"], meta["maxHP"], count)
    except (MapDataError, KeyError, TypeError):
        log.debug("monster %s skipped", mob_id, exc_info=True)   # event-only ids exist in some data sets
        return None


def _npc(npc_id: int) -> Npc | None:
    try:
        raw = _get_json(f"npc/{npc_id}")
        name = str(raw["name"]).strip()
        return Npc(npc_id, name, str(raw.get("function") or "").strip()) if name else None
    except (MapDataError, KeyError, TypeError):
        return None
