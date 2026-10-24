"""Looking for a newer release: GitHub names the candidate, the signed manifest is what makes it believable."""

import json
import logging
import queue
import threading
from collections.abc import Callable
from dataclasses import dataclass

from maplelens import config
from maplelens.update import UpdateError, net
from maplelens.update.manifest import asset_name, manifest_name, verify_manifest
from maplelens.update.model import Update
from maplelens.update.version import format_version, parse_version

log = logging.getLogger(__name__)
_API = f"https://api.github.com/repos/{config.UPDATE_REPO}/releases/latest"   # never a draft or pre-release
Fetch = Callable[[str, int], bytes]


def download_url(version: str, name: str) -> str:
    return f"https://github.com/{config.UPDATE_REPO}/releases/download/v{version}/{name}"


def release_page(version: str) -> str:
    return f"https://github.com/{config.UPDATE_REPO}/releases/tag/v{version}"


def find_update(current: str, fetch: Fetch = net.get_bytes,
                public_key: str = config.UPDATE_PUBLIC_KEY) -> Update | None:
    """None when up to date. Download URLs are built here from the version, never taken from server-sent text."""
    try:
        release = json.loads(fetch(_API, config.UPDATE_MAX_API_BYTES))
        tag = release["tag_name"]
    except (ValueError, KeyError, TypeError) as exc:
        raise UpdateError("GitHub sent an unexpected answer") from exc
    latest = parse_version(tag)
    if release.get("draft") or release.get("prerelease") or latest <= parse_version(current):
        return None
    version = format_version(latest)
    signed = fetch(download_url(version, manifest_name(version)), config.UPDATE_MAX_MANIFEST_BYTES)
    signature = fetch(download_url(version, manifest_name(version) + ".sig"), config.UPDATE_MAX_MANIFEST_BYTES)
    manifest = verify_manifest(signed, signature, public_key)
    if manifest.version != version:   # a validly signed manifest of another release must not pass as this one
        raise UpdateError("the update manifest is for a different version")
    return Update(version, manifest, download_url(version, asset_name(version)), release_page(version))


@dataclass(frozen=True)
class CheckResult:
    update: Update | None = None
    error: str | None = None


class UpdateChecker(threading.Thread):
    """Checks at start-up and then daily; `check_now` wakes it for a manual check. Results go to `events`."""

    def __init__(self, current: str, *, auto_check: bool = True, fetch: Fetch = net.get_bytes) -> None:
        super().__init__(daemon=True, name="update-check")
        self._current, self._fetch = current, fetch
        self.auto_check = auto_check
        self.events: queue.Queue[CheckResult] = queue.Queue()
        self.stop_event = threading.Event()
        self._wake = threading.Event()

    def check_now(self) -> None:
        self._wake.set()

    def stop(self) -> None:
        self.stop_event.set()
        self._wake.set()

    def run(self) -> None:
        wait: float | None = 0 if self.auto_check else None
        while not self.stop_event.is_set():
            manual = self._wake.wait(wait)
            self._wake.clear()
            if self.stop_event.is_set():
                break
            if not (manual or self.auto_check):
                wait = None
                continue
            try:
                self.events.put(CheckResult(update=find_update(self._current, self._fetch)))
                wait = config.UPDATE_CHECK_S
            except Exception as exc:   # offline, rate limited, bad signature: report it and try again later
                log.warning("update check failed: %s", exc)
                self.events.put(CheckResult(error=str(exc) if isinstance(exc, UpdateError) else "could not check"))
                wait = config.UPDATE_RETRY_S
            if not self.auto_check:
                wait = None
