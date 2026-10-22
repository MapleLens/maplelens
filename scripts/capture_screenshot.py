"""Captures the MapleStory window for the README.

Run from the repository root while MapleLens and the game are running:
    python scripts/capture_screenshot.py out.png [wait_seconds] [--blur=x,y,w,h ...]

The game draws its own mouse pointer into the frame, so the pointer is in the picture. Check the result for other
players' names before publishing it; each `--blur` pixelates one rectangle (client coordinates).
"""

import contextlib
import sys
import time
from pathlib import Path

import win32api
import win32con
import win32gui
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from maplelens.capture import game_window  # noqa: E402
from maplelens.capture.frame import ScreenGrabber  # noqa: E402


def capture(wait: float) -> Image.Image:
    hwnd = game_window.find_game_window()
    win32api.keybd_event(0x12, 0, 0, 0)   # an Alt tap lets us take the foreground
    win32gui.ShowWindow(hwnd, win32con.SW_RESTORE)
    with contextlib.suppress(win32gui.error):
        win32gui.SetForegroundWindow(hwnd)
    win32api.keybd_event(0x12, 0, 2, 0)
    time.sleep(wait)   # lets the glance card grow into the full one
    return Image.fromarray(ScreenGrabber().grab(game_window.client_box(hwnd))[:, :, ::-1]).convert("RGB")


def pixelate(image: Image.Image, box: tuple[int, int, int, int]) -> None:
    x, y, w, h = box
    small = image.crop((x, y, x + w, y + h)).resize((max(w // 8, 1), max(h // 8, 1)), Image.BILINEAR)
    image.paste(small.resize((w, h), Image.NEAREST), (x, y))


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--blur=")]
    out, wait = Path(args[0]), float(args[1]) if len(args) > 1 else 4.0
    shot = capture(wait)
    for flag in (a for a in sys.argv[1:] if a.startswith("--blur=")):
        pixelate(shot, tuple(int(v) for v in flag.split("=")[1].split(",")))
    out.parent.mkdir(parents=True, exist_ok=True)
    shot.save(out)
    print("saved", out, shot.size)
