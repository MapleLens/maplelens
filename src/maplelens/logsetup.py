"""Log file players can send when something breaks: maplelens.log next to the exe."""

import logging
import os
import platform
import sys
import threading
from logging.handlers import RotatingFileHandler
from pathlib import Path

from maplelens import __version__

LOG_NAME = "maplelens.log"
MAX_BYTES = 512 * 1024  # small enough to attach to a message; two rotations keep the previous run
BACKUPS = 2
FORMAT = "%(asctime)s %(levelname)s %(name)s %(message)s"
NOISY = ("asyncio", "PIL")  # chatty at DEBUG, never useful in a bug report

log = logging.getLogger(__name__)


def log_dir() -> Path:
    """The exe's folder when packaged, else the working directory (dev runs)."""
    if getattr(sys, "frozen", False):
        return Path(sys.executable).resolve().parent
    return Path.cwd()


def _open_handler() -> RotatingFileHandler:
    # The exe may sit somewhere read-only (Program Files); fall back to a per-user folder.
    fallback = Path(os.environ.get("LOCALAPPDATA", Path.home())) / "MapleLens"
    for folder in (log_dir(), fallback):
        try:
            folder.mkdir(parents=True, exist_ok=True)
            return RotatingFileHandler(folder / LOG_NAME, maxBytes=MAX_BYTES, backupCount=BACKUPS,
                                       encoding="utf-8")
        except OSError:
            continue
    raise OSError("no writable folder for the log file")


def setup_logging(debug: bool = False) -> Path | None:
    """Log INFO and above (DEBUG with --debug) to the log file. Returns its path, or None on failure."""
    try:
        handler = _open_handler()
    except OSError:
        return None
    handler.setFormatter(logging.Formatter(FORMAT))
    root = logging.getLogger()
    root.setLevel(logging.DEBUG if debug else logging.INFO)
    root.addHandler(handler)
    for noisy in NOISY:
        logging.getLogger(noisy).setLevel(logging.WARNING)

    def on_crash(exc_type, exc, tb) -> None:
        log.critical("Unhandled exception", exc_info=(exc_type, exc, tb))

    def on_thread_crash(args: threading.ExceptHookArgs) -> None:
        log.critical("Unhandled exception in thread %s", args.thread and args.thread.name,
                     exc_info=(args.exc_type, args.exc_value, args.exc_traceback))

    sys.excepthook = on_crash
    threading.excepthook = on_thread_crash
    log.info("MapleLens %s, Python %s, %s", __version__, platform.python_version(), platform.platform())
    return Path(handler.baseFilename)
