"""Name -> Item lookup that tolerates OCR slips ('Emerald 0re', stray punctuation, case)."""

import re
from collections.abc import Iterable

from rapidfuzz import fuzz, process

from maplelens.config import MIN_MATCH_SCORE
from maplelens.market.models import Item

_STRAY_TAIL = re.compile(r"(?:\s+[A-Za-z0-9|!]{1,2})+$")


def clean_title(text: str) -> str:
    """OCR text without trailing one- or two-character junk ('Kitty Camping Hat ii' -> 'Kitty Camping Hat')."""
    return _STRAY_TAIL.sub("", text.strip()) or text.strip()


def _key(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "", name.lower())


class Catalog:
    def __init__(self, items: Iterable[Item], generated_at: str | None = None) -> None:
        self.generated_at = generated_at   # when MesoWatch exported the data (shown as data age)
        self._by_key: dict[str, Item] = {}
        for item in items:
            known = self._by_key.get(_key(item.name))
            # Names can collide (variants of one item); keep the one with the most sales evidence.
            if known is None or item.sold > known.sold:
                self._by_key[_key(item.name)] = item
        self._keys = list(self._by_key)

    def __len__(self) -> int:
        return len(self._by_key)

    def lookup(self, ocr_text: str) -> Item | None:
        for text in (ocr_text, clean_title(ocr_text)):   # the second try drops trailing OCR junk
            key = _key(text)
            if not key:
                return None
            if key in self._by_key:
                return self._by_key[key]
            hit = process.extractOne(key, self._keys, scorer=fuzz.ratio, score_cutoff=MIN_MATCH_SCORE)
            if hit:
                return self._by_key[hit[0]]
        return None
