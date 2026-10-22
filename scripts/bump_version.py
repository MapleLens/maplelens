"""Set the release version: `python scripts/bump_version.py 0.2.0` (or patch/minor/major).

Edits src/maplelens/__init__.py, the single source of truth. Then commit and tag `v<version>`; pushing the tag
makes GitHub Actions build and publish the release.
"""

import re
import sys
from pathlib import Path

INIT = Path(__file__).resolve().parents[1] / "src" / "maplelens" / "__init__.py"
PATTERN = re.compile(r'__version__ = "(\d+)\.(\d+)\.(\d+)"')


def main(arg: str) -> int:
    match = PATTERN.search(INIT.read_text())
    if not match:
        print("could not find __version__")
        return 1
    major, minor, patch = map(int, match.groups())
    bumps = {"major": (major + 1, 0, 0), "minor": (major, minor + 1, 0), "patch": (major, minor, patch + 1)}
    if arg in bumps:
        new = ".".join(map(str, bumps[arg]))
    elif re.fullmatch(r"\d+\.\d+\.\d+", arg):
        new = arg
    else:
        print("usage: bump_version.py <major|minor|patch|X.Y.Z>")
        return 1
    INIT.write_text(PATTERN.sub(f'__version__ = "{new}"', INIT.read_text()))
    print(f"version -> {new}\nnext: git commit -am 'Release {new}' && git tag v{new} && git push --follow-tags")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1] if len(sys.argv) > 1 else ""))
