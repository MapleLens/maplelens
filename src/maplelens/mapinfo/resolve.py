"""OCR'd tooltip title -> map (pure)."""

import re
from collections.abc import Iterable, Mapping

from rapidfuzz import fuzz

from maplelens.mapinfo.models import Location

_MIN_SCORE = 80      # noisy reads ('Kerrting City Middle Forestl') still land close to the real name
_MIN_MARGIN = 4      # ...but the best candidate must beat the runner-up ('Ground I' vs 'Ground II')
_EXACT_BONUS = 10
_MIN_BARE_CHARS = 12   # without a colon only a long phrase is trusted as a title ('Wi pin' is background noise)
_TITLE = re.compile(r"^\s*([A-Za-z][A-Za-z' .-]{2,}?)\s*[:;]\s*(\S.*?)\s*$")
_WORD = re.compile(r"^[A-Za-z0-9'.,-]+$")   # what a real word of a map name may be made of
# The game font makes OCR swap these ('Ground l' for 'Ground I'); folding both sides costs nothing.
_LOOKALIKES = str.maketrans("l|1j0", "iiiio")


def _strip_junk(text: str) -> str:
    """Drop edge tokens OCR made of stray symbols ('Ground I l/', '� Victoria'); hyphenated words stay."""
    words = text.split()
    while len(words) > 1 and (not _WORD.match(words[-1]) or not any(c.isalnum() for c in words[-1])):
        words.pop()
    while len(words) > 1 and (not _WORD.match(words[0]) or not any(c.isalpha() for c in words[0])):
        words.pop(0)
    return " ".join(words)


def parse_location(text: str) -> Location | None:
    """"Victoria Road : Henesys" -> Location; None for anything else (item tooltips, UI hints, noise).

    OCR sometimes drops the colon (or turns it into a symbol); the text then counts as a bare name (street
    "") if it looks like a title.
    """
    text = _strip_junk(text.strip())
    match = _TITLE.match(text)
    if match is not None and any(c.isalpha() for c in match.group(2)):
        street = re.sub(r"^(?:[A-Za-z]\s+)+", "", match.group(1).strip())   # 'i Warning Street'
        return Location(street, _strip_junk(match.group(2).strip()))
    # A symbol where the colon should be ('Victoria Road � 3-Way') is dropped, leaving a bare phrase.
    bare = " ".join(w for w in text.split() if _WORD.match(w))
    words = bare.split()
    letters = sum(c.isalpha() or c in " -'" for c in bare)
    if len(words) >= 2 and len(bare) >= _MIN_BARE_CHARS and bare[:1].isupper() and letters >= 0.8 * len(bare):
        return Location("", bare)
    return None


def _fold(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "", text.lower()).translate(_LOOKALIKES)


def key(location: Location) -> str:
    """Identity of a reading that ignores OCR look-alike slips ('Ground l' / 'Ground I')."""
    return _fold(location.street + location.name)


def score(location: Location, candidate: Mapping[str, object]) -> float:
    """How well a search result matches what was read (0-105)."""
    name, street = _fold(str(candidate["name"])), _fold(str(candidate["streetName"]))
    read = _fold(location.name)
    if location.street:
        points = fuzz.ratio(read, name) + (5 if _fold(location.street) == street else 0)
    else:
        points = max(fuzz.ratio(read, name), fuzz.ratio(read, street + name))   # no colon: the street is in the text
    # Neighbouring maps differ by a numeral ('Ground I' / 'Ground II'): an exact reading must beat them clearly.
    return points + (_EXACT_BONUS if read in (name, street + name) else 0)


def pick(location: Location, candidates: Iterable[Mapping[str, object]]) -> int | None:
    """Id of the search result named like `location`, or None if none is close enough or two are tied."""
    ranked = sorted(((score(location, c), int(c["id"])) for c in candidates), reverse=True)
    if not ranked or ranked[0][0] < _MIN_SCORE:
        return None
    if len(ranked) > 1 and ranked[0][0] - ranked[1][0] < _MIN_MARGIN:
        return None   # two maps fit equally well: better no card than a wrong one
    return ranked[0][1]


def search_terms(location: Location) -> list[str]:
    """Queries to try, most specific first (OCR may have mangled one word)."""
    words = sorted(dict.fromkeys(w for w in location.name.split() if len(w) >= 4), key=len, reverse=True)
    return [location.name, *(w for w in words[:5] if w != location.name)]
