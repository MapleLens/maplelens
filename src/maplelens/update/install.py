"""Staging, verifying and swapping in a downloaded exe. A running exe can be renamed but not overwritten, so the
swap is rename-aside then move-in, with the old file put back if anything goes wrong."""

import logging
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import zipfile
from dataclasses import dataclass
from pathlib import Path

from maplelens import config
from maplelens.update import UpdateError, net
from maplelens.update.model import Update

log = logging.getLogger(__name__)
EXE_NAME, NEW_NAME, OLD_NAME = "maplelens.exe", "maplelens.new.exe", "maplelens.old.exe"
_CREATE_NO_WINDOW, _DETACHED_PROCESS = 0x08000000, 0x00000008
# PyInstaller one-file exes pass their unpack directory to children through the environment; a relaunch or
# self-test must start from scratch or it would reuse (and later lose) the parent's directory.
_FRESH_ENV = {"PYINSTALLER_RESET_ENVIRONMENT": "1"}


def can_self_update() -> bool:
    """Only the packaged exe can replace itself; a source checkout is updated with git/pip."""
    return bool(getattr(sys, "frozen", False))


def current_exe() -> Path:
    return Path(sys.executable)


def cleanup_leftovers(exe: Path | None = None, tries: int = 20, delay_s: float = 0.5) -> None:
    """The previous version's file stays locked until that process has fully exited, so keep trying for a while."""
    if not can_self_update():
        return
    exe = exe or current_exe()
    pending = [exe.with_name(name) for name in (OLD_NAME, NEW_NAME)]
    for attempt in range(tries):
        pending = [path for path in pending if path.exists()]
        for path in pending:
            try:
                path.unlink()
            except OSError:
                log.info("could not remove %s yet", path.name)
        if not [path for path in pending if path.exists()]:
            return
        if attempt + 1 < tries:
            time.sleep(delay_s)


def extract_exe(archive: Path, update: Update, target: Path) -> None:
    """Pull out the one expected member by exact name; nothing else in the zip is ever touched."""
    member = f"{Path(update.manifest.asset).stem}/{EXE_NAME}"
    try:
        with zipfile.ZipFile(archive) as bundle:
            # Some zip writers store backslashes; match the one expected name either way, never a pattern.
            info = next((i for i in bundle.infolist() if i.filename.replace("\\", "/") == member), None)
            if info is None:
                raise UpdateError("the update archive is not what was expected")
            if info.file_size > config.UPDATE_MAX_ZIP_BYTES:
                raise UpdateError("the update contains an oversized program")
            with bundle.open(info) as source, target.open("wb") as output:
                shutil.copyfileobj(source, output)
                if output.tell() != info.file_size:   # a zip lying about its size
                    raise UpdateError("the update archive is corrupt")
    except BaseException as exc:
        target.unlink(missing_ok=True)
        if isinstance(exc, zipfile.BadZipFile):
            raise UpdateError("the update archive is not what was expected") from exc
        raise


def self_test(exe: Path, version: str) -> None:
    """The new exe must start and report the version the signed manifest promised."""
    try:
        done = subprocess.run([str(exe), "--version"], capture_output=True, text=True, check=False,
                              timeout=config.UPDATE_SELF_TEST_S, creationflags=_CREATE_NO_WINDOW,
                              env={**os.environ, **_FRESH_ENV})
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise UpdateError("the downloaded program does not start") from exc
    if done.returncode or done.stdout.strip() != f"maplelens {version}":
        raise UpdateError("the downloaded program is not the expected version")


def stage(update: Update, exe: Path, progress: net.Progress | None = None) -> Path:
    """Download, verify, unpack next to `exe` and self-test. Returns the staged file or raises UpdateError."""
    staged = exe.with_name(NEW_NAME)
    with tempfile.TemporaryDirectory(prefix="maplelens-update-") as scratch:
        archive = Path(scratch) / update.manifest.asset
        net.download(update.url, archive, update.manifest.size, update.manifest.sha256, progress)
        try:
            extract_exe(archive, update, staged)
        except OSError as exc:
            raise UpdateError(f"cannot write to {exe.parent}: {exc}") from exc
    try:
        self_test(staged, update.version)
    except BaseException:
        staged.unlink(missing_ok=True)
        raise
    return staged


def swap(exe: Path, staged: Path) -> None:
    old = exe.with_name(OLD_NAME)
    try:
        old.unlink(missing_ok=True)
        exe.rename(old)
    except OSError as exc:
        raise UpdateError(f"cannot replace {exe.name}: {exc}") from exc
    try:
        os.replace(staged, exe)
    except OSError as exc:
        old.rename(exe)   # put the working version back
        raise UpdateError(f"cannot install the update: {exc}") from exc


def relaunch(exe: Path, old_version: str) -> None:
    subprocess.Popen([str(exe), "--updated-from", old_version], close_fds=True, creationflags=_DETACHED_PROCESS,
                     env={**os.environ, **_FRESH_ENV})


@dataclass(frozen=True)
class InstallEvent:
    progress: float = 0.0        # 0..1 while downloading
    staged: Path | None = None   # set once the update is verified and ready to swap in
    error: str | None = None


class UpdateInstaller(threading.Thread):
    """Does the slow, failure-prone part off the UI thread; the UI thread performs the swap on `staged`."""

    def __init__(self, update: Update, exe: Path) -> None:
        super().__init__(daemon=True, name="update-install")
        self._update, self._exe = update, exe
        self.events: queue.Queue[InstallEvent] = queue.Queue()

    def run(self) -> None:
        def progress(done: int, total: int) -> None:
            self.events.put(InstallEvent(progress=done / total))

        try:
            self.events.put(InstallEvent(staged=stage(self._update, self._exe, progress)))
        except Exception as exc:
            log.warning("update to %s failed: %s", self._update.version, exc)
            self.events.put(InstallEvent(error=str(exc) if isinstance(exc, UpdateError) else "the update failed"))
