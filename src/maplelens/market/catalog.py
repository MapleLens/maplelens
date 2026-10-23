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


# Glyphs the game font makes Windows OCR swap ('Stiff' -> 'Stitt', 'Ore' -> '0re'). Folding both sides before
# scoring lets one confusion cost nothing instead of an edit, without lowering the cutoff for real typos.
_OCR_LOOKALIKES = str.maketrans("f0i15", "tolls")


def _fold(key: str) -> str:
    return key.translate(_OCR_LOOKALIKES)


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
        self._folded = [_fold(key) for key in self._keys]

    def __len__(self) -> int:
        return len(self._by_key)

    def lookup(self, ocr_text: str) -> Item | None:
        for text in (ocr_text, clean_title(ocr_text)):   # the second try drops trailing OCR junk
            key = _key(text)
            if not key:
                return None
            if key in self._by_key:
                return self._by_key[key]
            hit = process.extractOne(_fold(key), self._folded, scorer=fuzz.ratio, score_cutoff=MIN_MATCH_SCORE)
            if hit:
                return self._by_key[self._keys[hit[2]]]
        return None
