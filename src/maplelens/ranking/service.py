"""Non-blocking rank lookups: cached, one request at a time, paced under the API's rate limit."""

import logging
import queue
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass
from enum import Enum

from maplelens import config
from maplelens.ranking.client import RankError, fetch_rank
from maplelens.ranking.models import Rank

log = logging.getLogger(__name__)


class Status(Enum):
    LOADING = "loading"
    FOUND = "found"
    MISSING = "missing"   # classicrank.com does not know the name
    FAILED = "failed"     # network trouble or rate limit


@dataclass(frozen=True)
class RankResult:
    status: Status
    rank: Rank | None = None
    expires_at: float = 0.0   # monotonic time after which the entry is looked up again


LOADING = RankResult(Status.LOADING)


class RankService:
    """`get(name)` returns at once; the answer is fetched by a background thread and cached."""

    def __init__(self, fetch: Callable[[str], Rank | None] = fetch_rank,
                 clock: Callable[[], float] = time.monotonic) -> None:
        self._fetch, self._clock = fetch, clock
        self._results: dict[str, RankResult] = {}
        self._queued: set[str] = set()
        self._queue: queue.Queue[str] = queue.Queue()
        self._lock = threading.Lock()

    def start(self) -> None:
        threading.Thread(target=self._work, daemon=True, name="rank-fetch").start()

    def get(self, name: str) -> RankResult:
        """The cached answer for `name` (stale ones are shown while they refresh); queues a lookup if due."""
        with self._lock:
            result = self._results.get(name, LOADING)
            if self._clock() >= result.expires_at and name not in self._queued:
                self._queued.add(name)
                self._queue.put(name)
            return result

    def _work(self) -> None:
        while True:
            self.refresh(self._queue.get())
            time.sleep(config.RANK_REQUEST_GAP_S)   # 30 requests/minute per IP without an API key

    def refresh(self, name: str) -> None:
        """Look `name` up now and store the outcome (called by the worker thread)."""
        try:
            rank = self._fetch(name)
            ttl = config.RANK_TTL_S if rank else config.RANK_MISSING_TTL_S
            result = RankResult(Status.FOUND if rank else Status.MISSING, rank, self._clock() + ttl)
        except RankError as exc:
            log.warning("rank lookup for %r failed: %s", name, exc)
            old = self._results.get(name)
            result = RankResult(Status.FAILED if old is None or old.rank is None else Status.FOUND,
                                old.rank if old else None, self._clock() + exc.retry_after)
        with self._lock:
            self._results[name] = result
            self._queued.discard(name)
