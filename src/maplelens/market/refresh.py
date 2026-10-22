"""Validated, atomic snapshot refreshes; network work stays off the UI thread."""

import logging
import os
import queue
import tempfile
import threading
import time
from collections.abc import Callable
from pathlib import Path

from curl_cffi import requests

from maplelens import config
from maplelens.market.catalog import Catalog
from maplelens.market.snapshot import SnapshotError, load_snapshot

log = logging.getLogger(__name__)
SNAPSHOT_URL = "https://meso.watch/data/list.json"


def refresh_snapshot(path: Path, *, now: float) -> bool:
    """Fetch only when absent or stale, preserving the cache on any failure."""
    try:
        if 0 <= now - path.stat().st_mtime < config.SNAPSHOT_REFRESH_S:
            load_snapshot(path)
            return False
    except (FileNotFoundError, SnapshotError):
        pass
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        response = requests.get(SNAPSHOT_URL, impersonate="chrome", timeout=config.SNAPSHOT_TIMEOUT_S)
        response.raise_for_status()
        with tempfile.NamedTemporaryFile(dir=path.parent, suffix=".tmp", delete=False) as output:
            temporary = Path(output.name)
            output.write(response.content)
        snapshot = load_snapshot(temporary)
        try:
            Catalog(snapshot.items, snapshot.generated_at)
        except (ValueError, TypeError, AttributeError) as exc:
            raise SnapshotError("downloaded snapshot cannot build a catalog") from exc
        os.replace(temporary, path)
        os.utime(path, (now, now))
        return True
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


class SnapshotRefresh(threading.Thread):
    """Publish new catalogs for the UI thread; failed requests retry with backoff."""

    def __init__(self, path: Path, *, clock: Callable[[], float] = time.time) -> None:
        """Accept a clock so refresh scheduling can be tested without real delays."""
        super().__init__(daemon=True, name="snapshot-refresh")
        self.path = path
        self._clock = clock
        self.events: queue.Queue[Catalog] = queue.Queue()
        self.stop_event = threading.Event()

    def run(self) -> None:
        """Refresh immediately, then wait until due or retry after a failure."""
        retry_delay = config.SNAPSHOT_RETRY_S
        while not self.stop_event.is_set():
            try:
                if refresh_snapshot(self.path, now=self._clock()):
                    snapshot = load_snapshot(self.path)
                    self.events.put(Catalog(snapshot.items, snapshot.generated_at))
                delay = min(config.SNAPSHOT_REFRESH_S,
                            max(1, self.path.stat().st_mtime + config.SNAPSHOT_REFRESH_S - self._clock()))
                retry_delay = config.SNAPSHOT_RETRY_S
            except Exception:
                log.exception("snapshot refresh failed; retaining cached prices")
                delay = retry_delay
                retry_delay = min(retry_delay * 2, config.SNAPSHOT_REFRESH_S)
            self.stop_event.wait(delay)
