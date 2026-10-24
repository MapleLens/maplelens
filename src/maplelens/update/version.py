"""Release versions are plain X.Y.Z (the tag is vX.Y.Z). Pre-release tags are never offered, so they do not parse."""

import re

from maplelens.update import UpdateError

Version = tuple[int, int, int]
_PATTERN = re.compile(r"v?(\d{1,6})\.(\d{1,6})\.(\d{1,6})")


def parse_version(text: object) -> Version:
    match = _PATTERN.fullmatch(text) if isinstance(text, str) else None
    if match is None:
        raise UpdateError(f"unsupported version {text!r}")
    major, minor, patch = map(int, match.groups())
    return major, minor, patch


def format_version(version: Version) -> str:
    return ".".join(map(str, version))
