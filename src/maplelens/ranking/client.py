"""HTTP client for the public classicrank.com API (https://classicrank.com/developer, no key needed)."""

import json
import logging
import urllib.error
import urllib.request
from urllib.parse import quote

from maplelens.ranking.models import API_FIELDS, Rank

log = logging.getLogger(__name__)

API_URL = "https://classicrank.com/api/v1/characters/"
_TIMEOUT_S = 5
_DEFAULT_RETRY_S = 30.0


class RankError(Exception):
    """The lookup failed for a reason worth retrying later (network, rate limit, bad response)."""

    def __init__(self, message: str, retry_after: float = _DEFAULT_RETRY_S) -> None:
        super().__init__(message)
        self.retry_after = retry_after


def fetch_rank(name: str) -> Rank | None:
    """The character's ranks, or None if classicrank.com does not know the name."""
    request = urllib.request.Request(
        f"{API_URL}{quote(name, safe='')}?fields={API_FIELDS}",
        headers={"Accept": "application/json", "User-Agent": "MapleLens/0.1"},
    )
    try:
        with urllib.request.urlopen(request, timeout=_TIMEOUT_S) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return None
        if exc.code == 429:   # the API says how long to back off
            raise RankError("rate limited", float(exc.headers.get("Retry-After") or _DEFAULT_RETRY_S)) from exc
        raise RankError(f"HTTP {exc.code}") from exc
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
        raise RankError(str(exc)) from exc
    try:
        return Rank.from_api(payload["data"])
    except (KeyError, TypeError) as exc:
        raise RankError(f"unexpected response: {exc!r}") from exc
