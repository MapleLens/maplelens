import io
import json
import urllib.error

import pytest

from maplelens.ranking import client
from maplelens.ranking.client import RankError, fetch_rank
from maplelens.ranking.models import Rank
from maplelens.ranking.service import RankService, Status

RAW = {"name": "masterball", "level": 27, "job": "Thief", "fame": 3, "overall_rank": 29516, "job_rank": 824,
       "fame_rank": 21312, "botting": False, "last_checked": "2026-10-22T10:16:24Z", "future_field": 1}


class _Response(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def _http_error(code: int, headers: dict | None = None) -> urllib.error.HTTPError:
    return urllib.error.HTTPError("u", code, "x", headers or {}, io.BytesIO(b"{}"))


def test_rank_parses_the_api_and_ignores_unknown_fields():
    rank = Rank.from_api(RAW)
    assert (rank.overall_rank, rank.job_rank, rank.fame_rank, rank.botting) == (29516, 824, 21312, False)


def test_rank_tolerates_unknown_job_and_fame():
    rank = Rank.from_api({"name": "x", "job": None, "fame": None, "job_rank": None})
    assert rank.job is None and rank.fame is None and rank.job_rank is None


def test_fetch_unwraps_data_and_quotes_the_name(monkeypatch):
    seen = []

    def fake(request, timeout):
        seen.append(request.full_url)
        return _Response(json.dumps({"data": RAW}).encode())

    monkeypatch.setattr(client.urllib.request, "urlopen", fake)
    assert fetch_rank("a b").name == "masterball"
    assert "/characters/a%20b?" in seen[0]


def test_fetch_maps_404_to_none_and_other_failures_to_errors(monkeypatch):
    def raising(error):
        def fake(request, timeout):
            raise error
        return fake

    monkeypatch.setattr(client.urllib.request, "urlopen", raising(_http_error(404)))
    assert fetch_rank("nobody") is None
    monkeypatch.setattr(client.urllib.request, "urlopen", raising(_http_error(429, {"Retry-After": "12"})))
    with pytest.raises(RankError) as limited:
        fetch_rank("x")
    assert limited.value.retry_after == 12
    monkeypatch.setattr(client.urllib.request, "urlopen", raising(urllib.error.URLError("offline")))
    with pytest.raises(RankError):
        fetch_rank("x")


def test_service_answers_loading_then_caches_the_result():
    now, calls = [0.0], []

    def fetch(name):
        calls.append(name)
        return Rank.from_api(RAW)

    service = RankService(fetch, lambda: now[0])
    assert service.get("masterball").status is Status.LOADING
    service.refresh(service._queue.get_nowait())
    assert service.get("masterball").status is Status.FOUND
    assert service._queue.empty() and calls == ["masterball"]   # fresh: no second lookup
    now[0] = 1000.0
    assert service.get("masterball").status is Status.FOUND     # stale entry is shown while it refreshes
    assert not service._queue.empty()


def test_service_remembers_unknown_names_and_failures():
    def fetch(name):
        if name == "boom":
            raise RankError("offline", 15)
        return None

    service = RankService(fetch, lambda: 0.0)
    for name, status in (("nobody", Status.MISSING), ("boom", Status.FAILED)):
        service.get(name)
        service.refresh(service._queue.get_nowait())
        assert service.get(name).status is status
