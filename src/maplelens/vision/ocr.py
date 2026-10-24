"""Read the item name from a tooltip using the built-in Windows OCR engine (no extra install)."""

import asyncio
from typing import NamedTuple

import cv2
import numpy as np
from winrt.windows.graphics.imaging import BitmapPixelFormat, SoftwareBitmap
from winrt.windows.media.ocr import OcrEngine
from winrt.windows.storage.streams import DataWriter

_TITLE_ROW_H = 46        # title row incl. descenders (q, y, g); the description line starts below this
_TITLE_LINE_H = 24       # a long name wraps onto a second title line this far below the first
_TITLE_LEFT_SKIP = 18    # skips the blue bullet in front of the title
_UPSCALE = 3             # the OCR engine is far more accurate on larger glyphs
_STRAY_LEADING = "'`.,·• "
_TEXT_THRESHOLD = 185      # title glyphs are near-white; the translucent fill never gets this bright
_CACHE_SIZE = 64
_LINE_PAD = 20   # white border added around the upscaled image, removed again from line positions


class TextLine(NamedTuple):
    """One recognised line of text and where it is, in the pixels of the image that was read."""

    text: str
    x: int
    y: int
    w: int
    h: int


class TitleReader:
    def __init__(self) -> None:
        engine = OcrEngine.try_create_from_user_profile_languages()
        if engine is None:
            raise RuntimeError("Windows OCR unavailable: install an OCR language pack (Settings > Language).")
        self._engine = engine
        # One loop for the thread's lifetime: asyncio.run() per frame would rebuild a loop 20x/s.
        self._loop = asyncio.new_event_loop()
        self._cache: dict[tuple[tuple[int, ...], bytes], str] = {}
        self._line_cache: dict[tuple[tuple[int, ...], bytes], list[TextLine]] = {}

    def read(self, image: np.ndarray, tooltip_width: int, lines: int = 1, top: int = 0) -> str:
        """Item name printed in the title row(s) of the tooltip at the image's top-left ('' if unreadable).

        `top` skips rows above the title: a player-crafted item has the crafter's name on its own line first.
        """
        row = image[top:top + _TITLE_ROW_H + (lines - 1) * _TITLE_LINE_H, _TITLE_LEFT_SKIP:tooltip_width]
        if row.size == 0:   # capture clipped at the screen edge (or a mid-resize frame): nothing to read
            return ""
        gray = cv2.cvtColor(row, cv2.COLOR_BGR2GRAY)
        # Binarise: pure black text on white is what the OCR engine reads best, and it removes whatever
        # game background shows through the translucent tooltip.
        return self.read_mask(gray > _TEXT_THRESHOLD)

    def read_mask(self, text: np.ndarray) -> str:
        """Text of a boolean glyph mask (True = glyph pixel)."""
        # A moving tooltip keeps the same title over changing backgrounds, so the text mask alone is the
        # cache key. Skipping OCR for repeats is what keeps tracking fast.
        key = (text.shape, np.packbits(text).tobytes())
        if (cached := self._cache.get(key)) is not None:
            return cached
        title = self._ocr(np.where(text, 0, 255).astype(np.uint8))
        if len(self._cache) >= _CACHE_SIZE:
            self._cache.clear()
        self._cache[key] = title
        return title

    def read_lines(self, text: np.ndarray) -> list[TextLine]:
        """Every line of text in a boolean glyph mask, with its position (cached like `read_mask`)."""
        key = (text.shape, np.packbits(text).tobytes())
        if (cached := self._line_cache.get(key)) is not None:
            return cached
        scaled = cv2.resize(np.where(text, 0, 255).astype(np.uint8), None, fx=_UPSCALE, fy=_UPSCALE,
                            interpolation=cv2.INTER_CUBIC)
        padded = cv2.copyMakeBorder(scaled, _LINE_PAD, _LINE_PAD, _LINE_PAD, _LINE_PAD, cv2.BORDER_CONSTANT,
                                    value=255)
        result = self._loop.run_until_complete(self._recognize_result(padded))
        lines = []
        for line in result.lines:
            rects = [w.bounding_rect for w in line.words]
            if not rects:
                continue
            left, top = min(r.x for r in rects), min(r.y for r in rects)
            right, bottom = max(r.x + r.width for r in rects), max(r.y + r.height for r in rects)
            lines.append(TextLine(line.text.strip(), round((left - _LINE_PAD) / _UPSCALE),
                                  round((top - _LINE_PAD) / _UPSCALE), round((right - left) / _UPSCALE),
                                  round((bottom - top) / _UPSCALE)))
        if len(self._line_cache) >= _CACHE_SIZE:
            self._line_cache.clear()
        self._line_cache[key] = lines
        return lines

    def _ocr(self, dark_on_light: np.ndarray) -> str:
        scaled = cv2.resize(dark_on_light, None, fx=_UPSCALE, fy=_UPSCALE, interpolation=cv2.INTER_CUBIC)
        padded = cv2.copyMakeBorder(scaled, 20, 20, 20, 20, cv2.BORDER_CONSTANT, value=255)
        raw = self._loop.run_until_complete(self._recognize(padded))
        return raw.strip().lstrip(_STRAY_LEADING)

    async def _recognize(self, gray: np.ndarray) -> str:
        return (await self._recognize_result(gray)).text

    async def _recognize_result(self, gray: np.ndarray):
        height, width = gray.shape
        writer = DataWriter()
        writer.write_bytes(bytearray(cv2.cvtColor(gray, cv2.COLOR_GRAY2BGRA).tobytes()))
        bitmap = SoftwareBitmap.create_copy_from_buffer(
            writer.detach_buffer(), BitmapPixelFormat.BGRA8, width, height
        )
        return await self._engine.recognize_async(bitmap)
