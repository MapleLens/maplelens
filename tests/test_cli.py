"""Startup must leave network work to the refresh worker."""

import sys
from types import SimpleNamespace

from maplelens import cli, config


def test_default_startup_builds_from_cache_without_network(monkeypatch):
    calls = []

    class App:
        def __init__(self, catalog, *, auto_refresh):
            calls.append((catalog, auto_refresh))

        def run(self):
            calls.append("run")

    monkeypatch.setitem(sys.modules, "maplelens.app",
                        SimpleNamespace(PriceOverlayApp=App, build_catalog=lambda path: path))
    monkeypatch.setattr("maplelens.market.refresh.refresh_snapshot",
                        lambda *a, **k: (_ for _ in ()).throw(AssertionError("network on main thread")))
    assert cli.main([]) == 0
    assert calls == [(config.DEFAULT_SNAPSHOT, True), "run"]
