"""Command line entry point: `maplelens` or `python -m maplelens`."""

import argparse
import logging
import threading
from collections.abc import Sequence
from pathlib import Path

from maplelens import __version__
from maplelens.logsetup import setup_logging


def _parse(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="maplelens", description="MapleStory Classic in-game overlays.")
    parser.add_argument("--snapshot", type=Path, default=None,
                        help="use a local snapshot instead of automatic hourly updates")
    parser.add_argument("--debug", action="store_true", help="also log detection details (DEBUG) to maplelens.log")
    parser.add_argument("--updated-from", metavar="VERSION", help=argparse.SUPPRESS)   # set by the self-updater
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _parse(argv)
    setup_logging(args.debug)

    # Imported late so `--help` / `--version` work without a display or OCR stack.
    from maplelens import config
    from maplelens.app import PriceOverlayApp, build_catalog
    from maplelens.update.install import cleanup_leftovers

    if args.updated_from:
        logging.getLogger(__name__).info("updated from %s to %s", args.updated_from, __version__)
    threading.Thread(target=cleanup_leftovers, daemon=True, name="update-cleanup").start()   # may need to wait

    path = args.snapshot or config.DEFAULT_SNAPSHOT
    PriceOverlayApp(build_catalog(path), auto_refresh=args.snapshot is None).run()
    return 0
