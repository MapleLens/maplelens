"""The app's update state machine, driven through the same queues the update threads use."""

import queue
from pathlib import Path

import pytest

from maplelens import app as app_module
from maplelens import startup
from maplelens.market.catalog import Catalog
from maplelens.market.sample import SAMPLE_ITEMS
from maplelens.update import UpdateError, install
from maplelens.update.check import CheckResult
from maplelens.update.manifest import Manifest, asset_name
from maplelens.update.model import Phase, Update, UpdateState

UPDATE = Update("0.2.0", Manifest("0.2.0", asset_name("0.2.0"), 1, "0" * 64), "https://github.com/x", "https://page")


@pytest.fixture
def app(tmp_path, monkeypatch):
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path))   # settings and the startup entry stay out of the real profile
    monkeypatch.setattr(startup, "set_enabled", lambda enabled: None)
    instance = app_module.PriceOverlayApp(Catalog(SAMPLE_ITEMS))
    yield instance
    instance._tray.close()
    instance._settings.close()
    instance._window.close()
    instance._rank_window.close()


def test_found_update_reaches_tray_and_settings_and_clears(app):
    app._updates.events.put(CheckResult(update=UPDATE))
    app._follow_updates()
    assert app._update_state.phase is Phase.AVAILABLE
    assert "new update available (v0.2.0)" in app._tray.tooltip
    app._updates.events.put(CheckResult(update=None))
    app._follow_updates()
    assert app._update_state.phase is Phase.UP_TO_DATE and "update" not in app._tray.tooltip


def test_failed_check_keeps_the_announced_update(app):
    app._updates.events.put(CheckResult(update=UPDATE))
    app._follow_updates()
    app._updates.events.put(CheckResult(error="could not check"))
    app._follow_updates()
    assert app._update_state.phase is Phase.FAILED and "v0.2.0" in app._tray.tooltip


def test_source_install_opens_the_release_page_instead_of_updating(app, monkeypatch):
    opened = []
    monkeypatch.setattr(install, "can_self_update", lambda: False)
    monkeypatch.setattr(app_module.webbrowser, "open", opened.append)
    app._updates.events.put(CheckResult(update=UPDATE))
    app._follow_updates()
    app._start_update()
    assert opened == ["https://page"] and app._installer is None


class _FakeInstaller:
    def __init__(self, *events):
        self.events = queue.Queue()
        for event in events:
            self.events.put(event)

    def start(self):
        pass


def test_download_progress_then_swap_quits_and_asks_for_relaunch(app, monkeypatch, tmp_path):
    staged, swapped = tmp_path / "new.exe", []
    monkeypatch.setattr(install, "swap", lambda exe, new: swapped.append((exe, new)))
    app._update_state = UpdateState(Phase.AVAILABLE, UPDATE)
    app._installer = _FakeInstaller(install.InstallEvent(progress=0.5), install.InstallEvent(staged=staged))
    app._follow_updates()
    assert swapped == [(install.current_exe(), staged)]
    assert app._quit and app._relaunch == install.current_exe()


def test_installer_error_is_shown_and_the_update_can_be_retried(app):
    app._update_state = UpdateState(Phase.DOWNLOADING, UPDATE)
    app._installer = _FakeInstaller(install.InstallEvent(error="the download does not match its checksum"))
    app._follow_updates()
    assert app._update_state.phase is Phase.FAILED and app._update_state.update is UPDATE
    assert not app._quit and app._relaunch is None


def test_failed_swap_does_not_quit_and_removes_the_staged_file(app, monkeypatch, tmp_path):
    staged = tmp_path / "new.exe"
    staged.write_bytes(b"x")

    def refuse(exe: Path, new: Path):
        raise UpdateError("cannot replace maplelens.exe")

    monkeypatch.setattr(install, "swap", refuse)
    app._update_state = UpdateState(Phase.DOWNLOADING, UPDATE)
    app._installer = _FakeInstaller(install.InstallEvent(staged=staged))
    app._follow_updates()
    assert not app._quit and not staged.exists() and app._update_state.phase is Phase.FAILED


def test_check_results_during_a_download_are_ignored(app):
    app._update_state = UpdateState(Phase.DOWNLOADING, UPDATE)
    app._installer = _FakeInstaller()
    app._updates.events.put(CheckResult(update=None))
    app._follow_updates()
    assert app._update_state.phase is Phase.DOWNLOADING
