# AGENTS.md

Guidance for AI coding agents (and humans) working on **MapleLens**: screen-reading overlays for
MapleStory Classic. Windows only. Python 3.11+.

## Commands

```bash
pip install -e ".[dev]"     # install with test tooling
pytest                      # full suite (includes OCR on a real screenshot; needs Windows OCR)
ruff check .                # lint (config in pyproject.toml)
maplelens --debug           # run; writes maplelens.log
```

Run tests and ruff before declaring work done. Never claim an overlay change works without either a test
or a screenshot of the running overlay over the game.

## Hard rules

1. **Screen pixels only.** Never inject into, hook, or read memory of the game process. Capture is `mss`
   on the client area; the game window is found by process name only.
2. **Never commit or bundle MesoWatch data.** Keep `data/*.json` git-ignored, and don't write docs or code that tell
   users how to download it. Fixtures and screenshots use hand-written `Item`s (`market/sample.py`). The maintainer
   has MesoWatch's permission for local use; that is personal and does not extend to other users or to bundling.
   An official API is promised: adopt it when it arrives and follow its conditions.
3. **The overlay must be click-through** and must never steal focus from the game.
4. **No searching, no per-frame guessing.** The tooltip is always at `mouse + config.TOOLTIP_OFFSET`
   (measured, not assumed). The card position comes from the live cursor on the UI tick; detection only
   supplies title and tooltip size. Do not add trackers/filters to paper over a wrong constant: re-measure it. The one exception is the game's own clamp: a tooltip too tall for the room below the mouse is pushed up to the client's bottom edge, so `vision/locate.py` looks for it there (only when the usual spot found no item and a tall tooltip could not fit); the card then uses `min(mouse + offset, bottom - height)`.
5. **Overlays only over the game.** The pipeline emits nothing unless MapleStory is the foreground window,
   and the card is hidden if it would not fit inside the client area.
6. **UI calls stay on the main thread.** The detection thread talks to the UI exclusively through
   `HoverPipeline.events`; `LayeredWindow` is only touched from `PriceOverlayApp`.

## Architecture

```
capture/   game_window.py  find MapleStory window, client rect (DPI aware), cursor
           frame.py        ScreenGrabber: BGR capture of a screen rectangle
vision/    tooltip.py      tooltip_present(image) -> bool   (pure)
           locate.py       candidate_tops(strip): find a tooltip pushed up by the game's bottom clamp (pure)
           ocr.py          TitleReader: Windows OCR of a tooltip title row (cached)
market/    models.py       Item and nested data (Day, NpcShop, Craft, CashInfo); Item.from_raw
           snapshot.py     load list.json -> Snapshot(items, generated_at)
           catalog.py      Catalog.lookup(ocr_text) fuzzy name match
           insights.py     derived numbers: ask premium, log range position, pace, craft margin (pure)
           format.py       format_mesos, format_ago (pure)
           sample.py       built-in demo items
overlay/   theme.py        RGBA palette, Tahoma fonts, measured translucency
           canvas.py       Canvas: drawing toolkit on an RGBA image (text, chips, layers)
           glance.py       glance card sections: what matters on hover (price, range, quick signals)
           details.py      details card sections, shown after a short dwell
           origin.py       details section: NPC prices, crafting cost and margin, where it drops
           card.py         render_card(item) -> RGBA image (pure, testable); stacks section groups with dividers
           placement.py    place_card(): right / left / below(compare), clamped to the game (pure)
           layered.py      LayeredWindow: per-pixel-alpha topmost click-through Win32 window
pipeline.py                HoverPipeline thread -> Hover events (title, item, size, compare)
app.py                     PriceOverlayApp: main loop wiring pipeline to window; card cache
cli.py / __main__.py       entry points
config.py                  every tunable number, with a comment saying why

Dependency direction: `cli → app → (pipeline, overlay) → (capture, vision, market) → geometry/config`.
`market/`, `vision/tooltip.py`, `overlay/{card,canvas,glance,details,origin,placement}.py` are pure (no win32, no windows) so
they stay unit-testable. Only `overlay/layered.py` and `capture/` talk to Windows. No tkinter anywhere.

## Conventions

- Type hints everywhere; `from __future__` not needed (3.11+). Prefer frozen dataclasses / `NamedTuple`.
- Magic numbers become named constants (module-level, or `config.py` if user-tunable). Say *why* in a
  short comment, not *what*.
- Comments explain non-obvious reasons (e.g. why a 3x upscale, why no margin). No narration of code.
- Modules stay small and single-purpose. If a file passes ~150 lines, split it.
- Errors: raise specific exceptions at boundaries (`SnapshotError`); the UI tick catches and logs so one
  bad frame never kills the overlay.
- Logging via `logging.getLogger(__name__)`, never `print` (except user-facing startup messages in `app.py`).
- Windows OCR and win32 are imported lazily or isolated in `capture/` and `vision/ocr.py`.

## Testing

- Pure logic (`format`, `catalog`, `snapshot`, `measure_tooltip`) gets plain unit tests.
- Fixtures are real native-resolution captures whose top-left pixel is the tooltip top-left (`tooltip_native_*.png`, all 450x178). Use `maplelens --debug`: every new tooltip is saved to `debug/` as a candidate fixture. Never use downscaled screenshots: the border detector needs native pixels.
  Add new fixtures for new tooltip kinds (equipment, scrolls) instead of loosening thresholds.
- Anything time-dependent takes `now` as a parameter.

## Adding a feature (recipe)

New overlay (e.g. stack total, equipment rolls):
1. Add the pure data/logic under `market/` (or a new package) with tests.
2. Add a section in `overlay/glance.py` or `overlay/details.py` (signature `(canvas, y, item) -> y`) using `overlay/theme.py` colours; translucent shapes go through `Canvas.overlay` (PIL overwrites RGBA pixels instead of blending).
3. Extend `Hover`/pipeline only if new detection is needed; otherwise reuse `Hover.item`.
4. Wire it in `app.py`. Keep `OverlayWindow` generic.
5. Update the README feature table.

## Gotchas

- `SetProcessDPIAware` must run before any coordinates are read (done in `capture/game_window.py`).
- The game must be the visible top window: capture is of the screen, not the window buffer.
- Windows OCR flips `'Emerald Ore'` / `"'Emerald Ore"` between frames; titles are stripped of stray
  leading punctuation and matching is fuzzy.
- pytest's default temp dir can be blocked by sandboxes; `addopts` pins `--basetemp=.pytest_tmp`.
- The agent cannot move the user's mouse (`SetCursorPos` is denied). Live hover tests need the user, or a
  fixture screenshot.
