# MapleLens 0.2.0: full feature test (with a player at the keyboard)

What the automated suite cannot do: move the mouse in the real game, open windows, make sales, or judge how a card
looks. Work through this together: one person plays, the other watches `maplelens.log` / `debug\` and ticks boxes.
Every failure gets a capture (`--debug` saves them) so it can become a fixture.

## 0. Setup

- [ ] Close any other MapleLens (the Python 0.1.x build `python -m maplelens` holds the same hotkey and draws its own
      cards). Tray icon gone, `tasklist | findstr /i maplelens` empty.
- [ ] Start `target\release\maplelens.exe --debug` from a terminal in the repo root (console output, `debug\` here).
- [ ] Note the game's settings: window size, **UI Size** (Normal / Ideal Ratio), windowed or full screen. Each section
      below is run at Normal UI first, then repeated where marked **[IR]** at Ideal Ratio.
- [ ] `maplelens.log` (next to the exe) shows `loaded N items` and `UI scale …` once the game is in front.

## 1. Price card

| # | Do | Expect |
|---|---|---|
| 1.1 | Hover an Etc item in the inventory (Emerald Ore) | Glance card right of the tooltip within a frame or two; moves with the mouse, no jitter |
| 1.2 | Keep the pointer still ~1.2 s | Card grows to the full view (chart, depth, origin); setting "Expand" changes the delay |
| 1.3 | Hover near the right screen edge | Card flips to the left of the tooltip |
| 1.4 | Hover an equip with an equipped one to compare | Card goes below the two tooltips |
| 1.5 | Hover a player-crafted equip (crafter line) | Correct item; card height accounts for the crafter line |
| 1.6 | Hover an item in the bottom rows (tall tooltip pushed up) | Card still beside the tooltip |
| 1.7 | Hover a **scroll** ("…Scroll: Greater") | Price card shows (a colon in the name must not block it) |
| 1.8 | Hover items in a player shop and in the Cash Shop / NX items | Cards with shop / NX info |
| 1.9 | Hover an item with no sales | "No price data" notice, not a wrong item |
| 1.10 | Alt-tab away mid-hover | Card disappears at once; nothing drawn over other apps |
| 1.11 | **[IR]** repeat 1.1–1.6 | Same results; log shows `UI scale 0.640` |
| 1.12 | Resize the game window (smaller / larger) | Cards scale with it |

## 2. Reading letters

- [ ] Hover ~10 items with unusual letters (Q, Z, digits, apostrophes). `--debug` saves each new tooltip to `debug\`.
- [ ] An item read with `?` resolves anyway (catalog wildcard) or after one OCR read; `%LOCALAPPDATA%\MapleLens\glyphs-title.json`
      grows (`learnt new title letters` in the log).

## 3. Rank panel

| # | Do | Expect |
|---|---|---|
| 3.1 | Open your own Character Info | Panel beside it: world / job / fame rank, "updated …" |
| 3.2 | Open another player's | Their ranks; "Looking up rank…" first, then the numbers |
| 3.3 | A brand-new / unranked character | "Not ranked yet" |
| 3.4 | Drag the window to each screen edge | Panel moves to the other side, or hides when there is no room |
| 3.5 | **[IR]** repeat 3.1–3.2 | Same |

## 4. Map helper (new in the Rust port)

| # | Do | Expect |
|---|---|---|
| 4.1 | Open the world map (W), hover a town (Henesys) | Loading card with a turning ring, then the town card: NPCs with sprites, "Town" chip |
| 4.2 | Hover a hunting ground | Monsters: level, EXP, HP, ×spawns, share bar; level range in the header |
| 4.3 | Hover the same place again | Instant (cache in `%LOCALAPPDATA%\MapleLens\cache\maps` and `icons`) |
| 4.4 | Sweep across many places quickly | No wrong card; at most a short loading card; no stuck card after the pointer leaves |
| 4.5 | Hover "Ground I" vs "Ground II" neighbours | Each gets its own map, never the other |
| 4.6 | Hover an item in the inventory while the map is open | Map card hides; price card shows |
| 4.7 | Unplug the network, hover a new place | "Can't reach maplestory.io, retrying…"; recovers when back online |
| 4.8 | A big town (many NPCs) | Card adds columns instead of running off the screen |
| 4.9 | **[IR]** repeat 4.1–4.2 | Same. `debug\map_scan.png` is the scan area if not |

## 5. Store Permit watcher (new in the Rust port)

Run the console view in a second terminal if wanted: `maplelens --shop` (prints events; also `debug\shop_events.log`).

| # | Do | Expect |
|---|---|---|
| 5.1 | Open your store (set up, not yet open) | Pill bottom-left: "Store not open" (grey) |
| 5.2 | Open it, Sold Items window open | "Store detected" (green); console `OPENED` once |
| 5.3 | Close the Sold Items window | "Open the Sold Items window" (amber) |
| 5.4 | Someone buys one item | `SALE <item> x<n> to <buyer>`, exactly once |
| 5.5 | Same buyer buys the same thing twice | Two `SALE` lines, never three |
| 5.6 | A sale while Sold Items is closed | `SALE? +N mesos …`; opening the window then reports the `SALE` |
| 5.7 | Visitors enter / leave, someone chats | `ENTERED` / `LEFT` / `CHAT` with names |
| 5.8 | Scroll the Sold Items list or chat up | Pill "Scroll the lists back…"; no events until scrolled back |
| 5.9 | Cover the shop with another app (game still visible behind) / alt-tab away | Events keep coming (window capture); pill hidden while the game is not in front |
| 5.10 | Minimise the game for a minute, then restore | No false CLOSED; missed sales show up when the list is seen again (or LOST SYNC) |
| 5.11 | Visit someone else's store | No pill, no events |
| 5.12 | Close your store | `CLOSED` after ~5 s, once |
| 5.13 | Settings: untick "Show the store status bar" | Pill gone; events still logged with --debug |
| 5.14 | **[IR]** repeat 5.1–5.4 | Open store, "open the Sold Items window" status, chat enter/leave and a live visitor were verified on real IR pixels (fixture `shop_ideal_ratio_open.png`); a sale and the Sold Items list at IR still need checking. Chat names and the meso counter now read correctly at IR |
| 5.15 | Windows 10 only: is a yellow capture border drawn round the game? | Windows 11: none |

## 6. Overlay behaviour, tray and settings

- [ ] Clicks pass through every card to the game; the game never loses focus.
- [ ] Ctrl+Alt+M toggles everything (tray icon state follows); tray double-click opens Settings.
- [ ] Settings: each checkbox applies at once and survives a restart (`%LOCALAPPDATA%\MapleLens\settings.json`);
      hotkey box takes a new combo (click it, press Ctrl/Alt + a key; Esc cancels, Backspace removes it), Clear
      removes it; "Launch when Windows starts" creates/removes the Run entry.
- [ ] Settings window looks like a game window: drag it by the title bar, hover/press states on every control, the
      slider drags and takes arrow keys, Tab shows a blue focus ring, Esc or the close box hides it; sharp at 150%
      Windows scaling and after moving it to a monitor with another scale.
- [ ] Second `maplelens.exe` says "already running" and quits.
- [ ] OBS / Discord screen share of the game does **not** show the cards (excluded from capture).

## 7. Updates

- [ ] Settings → "Check now": "You have the latest version." (v0.1.3 is the newest release and is older).
- [ ] Untick "Check for updates automatically": no check at start-up (log).
- [ ] A copy in `target\` shows "View release" instead of "Update now".
- [ ] After 0.2.0 is released (signed in CI): build a copy labelled 0.1.99, put it outside `target\` as `maplelens.exe`,
      run it → tray "Update to v0.2.0" → Update now → progress → restarts as 0.2.0, old exe cleaned up.

## 8. Web app

Done in Chrome (2026-10-24) through the real Share path, with the share dialog replaced by a stream of real game
frames: title-bar auto-detection, tooltip + Character Info reads, live ranks, Ideal Ratio frame (UI 0.64 measured,
no false detections), stop from outside, the sample, the pop-out window (cards now fit it), the site demo, all
links, 375/768 px layouts, no console errors. Still for a person:

- [ ] Share the real game window from Chrome's dialog and hover items (the dialog cannot be automated).
- [ ] Resize the pop-out window: the cards scale with it.

## 9. Resources

- [ ] Task Manager while idle in town: CPU low single digits; while hovering: brief spikes only.
- [ ] Idle with no shop open: the watcher costs ~30 ms every 1.5 s (no steady load).
- [ ] Memory stays flat over 30 min of play (no growth from card caches).

## Known gaps going in

- **Store Permit at Ideal Ratio**: verified live for the open store, its status, chat names and the meso counter
  (text is read from the unscaled pixels; fixture added). Not yet seen live: the Sold Items list and a sale at IR.
- **CPU**: at Ideal Ratio with a store open the app uses ~10% of one core (each shop frame is scaled up 1.56x).
- **Map helper offsets** were measured by the Python version on two samples only; the scan window is generous, but
  odd placements near screen edges are worth a look.
- The **updater** cannot be tested end to end until the first signed release exists.
- The **web app** has no map helper or store watcher (no mouse position, and maplestory.io/OCR line positions are
  not wired there yet).
