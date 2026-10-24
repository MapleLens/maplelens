import json
import os
from types import SimpleNamespace

import pytest

from maplelens import config
from maplelens.market import refresh
from maplelens.market.snapshot import SnapshotError


def response(content, status=200):
    return SimpleNamespace(content=content, status_code=status, raise_for_status=lambda: None)


def test_missing_fresh_and_expired_cache(tmp_path, monkeypatch):
    path = tmp_path / "data" / "list.json"
    payload = json.dumps({"items": [{"name": "Handwritten demo", "price": 42}]}).encode()
    calls = []

    def get(*args, **kwargs):
        calls.append(kwargs)
        return response(payload)

    monkeypatch.setattr(refresh.requests, "get", get)
    assert refresh.refresh_snapshot(path, now=10000)
    assert path.read_bytes() == payload
    assert not refresh.refresh_snapshot(path, now=10001)
    assert refresh.refresh_snapshot(path, now=10000 + config.SNAPSHOT_REFRESH_S)
    assert len(calls) == 2
    assert calls[0]["impersonate"] in refresh.BROWSER_PROFILES
    assert calls[0]["headers"]["Referer"] == "https://meso.watch/"


def test_blocked_profile_is_replaced(tmp_path, monkeypatch):
    path = tmp_path / "list.json"
    payload = json.dumps({"items": [{"name": "Handwritten demo", "price": 42}]}).encode()
    profiles = []

    def get(*args, **kwargs):
        profiles.append(kwargs["impersonate"])
        return response(payload, 200 if len(profiles) == 3 else 403)

    monkeypatch.setattr(refresh.requests, "get", get)
    assert refresh.refresh_snapshot(path, now=10000)
    assert len(set(profiles)) == 3


@pytest.mark.parametrize("content", [b"<html>Cloudflare challenge</html>", b'{}'])
def test_invalid_response_preserves_cache(tmp_path, monkeypatch, content):
    path = tmp_path / "list.json"
    old = b'{"items": [{"name": "Demo", "price": 1}]}'
    path.write_bytes(old)
    os.utime(path, (0, 0))
    monkeypatch.setattr(refresh.requests, "get", lambda *a, **k: response(content))
    with pytest.raises(SnapshotError):
        refresh.refresh_snapshot(path, now=10000)
    assert path.read_bytes() == old
    assert list(tmp_path.iterdir()) == [path]


def test_network_failure_keeps_cache(tmp_path, monkeypatch):
    path = tmp_path / "list.json"
    path.write_text('{"items": []}')
    os.utime(path, (0, 0))

    def fail(*args, **kwargs):
        raise OSError("offline")

    monkeypatch.setattr(refresh.requests, "get", fail)
    with pytest.raises(OSError):
        refresh.refresh_snapshot(path, now=10000)
    assert path.read_text() == '{"items": []}'


def test_future_cache_is_refreshed(tmp_path, monkeypatch):
    path = tmp_path / "list.json"
    path.write_text('{"items": []}')
    os.utime(path, (20000, 20000))
    monkeypatch.setattr(refresh.requests, "get", lambda *a, **k: response(b'{"items": []}'))
    assert refresh.refresh_snapshot(path, now=10000)
    assert path.stat().st_mtime == 10000


def test_consumer_incompatible_response_preserves_cache(tmp_path, monkeypatch):
    path = tmp_path / "list.json"
    old = b'{"items": [{"name": "Demo", "price": 1}]}'
    path.write_bytes(old)
    os.utime(path, (0, 0))
    monkeypatch.setattr(refresh.requests, "get",
                        lambda *a, **k: response(b'{"items": [{"name": null}]}'))
    with pytest.raises(SnapshotError, match="cannot build a catalog"):
        refresh.refresh_snapshot(path, now=10000)
    assert path.read_bytes() == old
    assert list(tmp_path.iterdir()) == [path]


def test_worker_publishes_and_schedules_using_injected_clock(tmp_path, monkeypatch):
    path = tmp_path / "list.json"
    worker = refresh.SnapshotRefresh(path, clock=lambda: 10000)
    monkeypatch.setattr(refresh.requests, "get",
                        lambda *a, **k: response(b'{"items": [{"name": "Demo", "price": 42}]}'))
    waits = []

    def wait(delay):
        waits.append(delay)
        worker.stop_event.set()

    monkeypatch.setattr(worker.stop_event, "wait", wait)
    worker.run()
    assert worker.events.get_nowait().lookup("Demo").price == 42
    assert waits == [config.SNAPSHOT_REFRESH_S]


def test_worker_retries_with_backoff(tmp_path, monkeypatch):
    worker = refresh.SnapshotRefresh(tmp_path / "list.json", clock=lambda: 10000)
    waits = []

    def fail(*args, **kwargs):
        raise OSError("offline")

    def wait(delay):
        waits.append(delay)
        if len(waits) == 3:
            worker.stop_event.set()

    monkeypatch.setattr(refresh.requests, "get", fail)
    monkeypatch.setattr(worker.stop_event, "wait", wait)
    worker.run()
    assert waits == [config.SNAPSHOT_RETRY_S * n for n in (1, 2, 4)]
