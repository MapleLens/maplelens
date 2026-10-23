"""Non-blocking map lookups: cached, one request chain at a time, sprites downloaded afterwards."""

import logging
import queue
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass
from enum import Enum

from maplelens import config
from maplelens.mapinfo.client import MapDataError, fetch_map, find_map_id
from maplelens.mapinfo.icons import IconCache
from maplelens.mapinfo.models import Location, MapInfo
from maplelens.mapinfo.store import MapStore

log = logging.getLogger(__name__)


class Status(Enum):
    LOADING = "loading"
    FOUND = "found"
    MISSING = "missing"   # no map is named like the tooltip
    FAILED = "failed"     # network trouble


@dataclass(frozen=True)
class MapResult:
    status: Status
    info: MapInfo | None = None
    icons: int = 0          # sprites on disk so far: a changed count means the card should be redrawn
    retry_at: float = 0.0   # monotonic time after which a failed lookup is tried again


LOADING = MapResult(Status.LOADING)


class MapService:
    """`get(location)` returns at once; the answer is fetched by a background thread and cached."""

    def __init__(self, icons: IconCache, store: MapStore | None = None,
                 find: Callable[[Location], int | None] = find_map_id,
                 fetch: Callable[[int], MapInfo] = fetch_map, clock: Callable[[], float] = time.monotonic) -> None:
        self.icons, self._store = icons, store
        self._find, self._fetch, self._clock = find, fetch, clock
        self._results: dict[Location, MapResult] = {}
        self._queued: set[Location] = set()
        self._queue: queue.LifoQueue[Location] = queue.LifoQueue()   # the newest hover is the one being looked at
        self._wanted: dict[Location, float] = {}
        self._lock = threading.Lock()

    def start(self) -> None:
        threading.Thread(target=self._work, daemon=True, name="map-fetch").start()

    def get(self, location: Location) -> MapResult:
        with self._lock:
            self._wanted[location] = self._clock()
            result = self._results.get(location, LOADING)
            due = result.status is Status.LOADING or (result.status is Status.FAILED
                                                      and self._clock() >= result.retry_at)
            if due and location not in self._queued:
                self._queued.add(location)
                self._queue.put(location)
            return result

    def _work(self) -> None:
        while True:
            location = self._queue.get()
            with self._lock:
                stale = self._clock() - self._wanted.get(location, 0.0) > config.MAP_STALE_S
                if stale:   # the pointer moved on while it waited (OCR variants pile up): not worth a request
                    self._queued.discard(location)
            if not stale:
                self.refresh(location)

    def refresh(self, location: Location) -> None:
        """Look `location` up now, then fetch its sprites (called by the worker thread)."""
        try:
            info = self._lookup(location)
            result = MapResult(Status.MISSING) if info is None else MapResult(Status.FOUND, info)
        except MapDataError as exc:
            log.warning("map lookup for %s failed: %s", location, exc)
            result = MapResult(Status.FAILED, retry_at=self._clock() + config.MAP_RETRY_S)
        log.debug("map lookup %s -> %s %s", location, result.status.value, result.info and result.info.name)
        self._put(location, result)
        if result.info is not None:
            self._download_icons(location, result.info)

    def _lookup(self, location: Location) -> MapInfo | None:
        """The map for `location`: from the disk cache when seen before, else from the network (and cached)."""
        map_id = self._store.find(location) if self._store else None
        if map_id is None:
            map_id = self._find(location)
            if map_id is None:
                return None
            if self._store:
                self._store.remember(location, map_id)
        info = self._store.load(map_id) if self._store else None
        if info is None:
            info = self._fetch(map_id)
            if self._store:
                self._store.save(info)
        return info

    def _put(self, location: Location, result: MapResult) -> None:
        with self._lock:
            self._results[location] = result
            self._queued.discard(location)

    def _download_icons(self, location: Location, info: MapInfo) -> None:
        wanted = [("mob", m.id) for m in info.mobs] \
            + [("npc", n.id) for n in info.npcs]
        ready = sum(self.icons.has(*key) for key in wanted)
        self._put(location, MapResult(Status.FOUND, info, ready))
        for key in wanted:
            if not self.icons.has(*key) and self.icons.download(*key):
                ready += 1
                self._put(location, MapResult(Status.FOUND, info, ready))
