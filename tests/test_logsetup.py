"""The log file players send us: written next to the exe, warnings and errors included."""

import logging
import sys
import threading

from maplelens import logsetup


def _restore(monkeypatch):
    root = logging.getLogger()
    monkeypatch.setattr(root, "handlers", list(root.handlers))
    monkeypatch.setattr(root, "level", root.level)
    monkeypatch.setattr(sys, "excepthook", sys.excepthook)
    monkeypatch.setattr(threading, "excepthook", threading.excepthook)


def test_log_goes_next_to_the_exe_when_frozen(monkeypatch, tmp_path):
    _restore(monkeypatch)
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "executable", str(tmp_path / "maplelens.exe"))
    path = logsetup.setup_logging()
    assert path == tmp_path / "maplelens.log"
    logging.getLogger("x").warning("careful")
    logging.getLogger("x").debug("hidden without --debug")
    for h in logging.getLogger().handlers:
        h.flush()
    text = path.read_text(encoding="utf-8")
    assert "careful" in text and "MapleLens" in text and "hidden" not in text


def test_unhandled_exception_is_logged(monkeypatch, tmp_path):
    _restore(monkeypatch)
    monkeypatch.chdir(tmp_path)
    path = logsetup.setup_logging()
    try:
        raise ValueError("boom")
    except ValueError:
        sys.excepthook(*sys.exc_info())
    for h in logging.getLogger().handlers:
        h.flush()
    assert "ValueError: boom" in path.read_text(encoding="utf-8")
