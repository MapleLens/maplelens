"""Settles noisy OCR readings of one tooltip into a single stable answer (pure)."""

from collections import deque
from typing import Generic, TypeVar

from rapidfuzz import fuzz

T = TypeVar("T")

_SIMILAR = 80   # readings of one title differ by a few glyphs; different titles score far lower


class Voter(Generic[T]):
    """Keeps the last few (reading, payload) pairs; answers with the payload of the most typical reading.

    The answer is held while enough recent readings still agree with it, so one odd frame cannot swap the
    location or resize the card; a new tooltip takes over once `need` readings of it have been seen.
    """

    def __init__(self, size: int = 7, need: int = 3, keep: int = 2) -> None:
        self._recent: deque[tuple[str, T]] = deque(maxlen=size)
        self._need, self._keep = need, keep
        self._held: tuple[str, T] | None = None

    def clear(self) -> None:
        self._recent.clear()
        self._held = None

    def add(self, reading: str, payload: T) -> T | None:
        self._recent.append((reading, payload))
        if self._held is not None and self._support(self._held[0]) >= self._keep:
            return self._held[1]
        best = max(self._recent, key=lambda r: (self._support(r[0]), self._score(r[0])), default=None)
        if best is None or self._support(best[0]) < self._need:
            self._held = None
            return None
        cluster = [r for r in self._recent if self._same(r[0], best[0])]
        self._held = max(cluster, key=lambda r: self._score(r[0]))   # the medoid: closest to all the others
        return self._held[1]

    @staticmethod
    def _same(a: str, b: str) -> bool:
        return fuzz.ratio(a, b) >= _SIMILAR

    def _support(self, reading: str) -> int:
        return sum(self._same(reading, r[0]) for r in self._recent)

    def _score(self, reading: str) -> float:
        return sum(fuzz.ratio(reading, r[0]) for r in self._recent)
