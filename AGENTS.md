# AGENTS.md

Guidance for AI coding agents (and humans) working on **MapleLens**: screen-reading overlays for
MapleStory Classic. A Rust workspace: a platform-free core, a Windows app, and a WebAssembly engine for the browser.

## Commands

```bash
cargo test --workspace                          # all tests (Windows-layer tests need a desktop session)
cargo clippy --workspace --all-targets          # lint; must be clean
cargo clippy -p maplelens-web --target wasm32-unknown-unknown
cargo fmt --all                                 # rustfmt.toml: 120 columns
cargo doc --workspace --no-deps                 # docs must build without warnings (CI: RUSTDOCFLAGS=-D warnings)
cargo deny check                                # advisories, licences, sources (deny.toml)
cargo +1.88 check --workspace --all-targets     # the minimum Rust version still builds
cargo run -p maplelens -- --debug               # the Windows app; writes maplelens.log, saves captures to debug\
./scripts/build-web.ps1                         # rebuild docs/app/pkg (commit it) after changing core or web
cargo run -p maplelens-core --example build_atlas    # rebuild the glyph atlases from vision::samples
cargo run --release -p maplelens-core --example bench_search   # browser-path detection timings
cargo run -p maplelens-core --example render_cards -- OUT_DIR     # sample cards as PNGs, for a visual check
cargo test -p maplelens live_ -- --ignored --nocapture   # live checks: the running game, the store, remote services
cargo run --release -p maplelens-core --example bench_info   # Character Info window search timings per resolution
./scripts/stream-bench.ps1 -Channels a,b -Seconds 300    # live MapleStory streams through the real detectors (yt-dlp + ffmpeg)
MAPLELENS_FRAMES=<frames> [MAPLELENS_UI_SCALE=0.95] [MAPLELENS_CURSOR=x,y] cargo test --release -p maplelens stream_bench -- --ignored --nocapture
                                                # the same on a folder of stream frames; see "Stream footage" below
cargo test --release -p maplelens wrong_items_under -- --ignored --nocapture   # one-letter misreads of every name in the local snapshot: how many name another item
cargo test --release -p maplelens idle_hover_pass_cost -- --ignored --nocapture   # what one hover pass costs on this desktop (GDI grab included)
MAPLELENS_CORPUS=<debug captures> cargo test --release -p maplelens reading_bench -- --ignored --nocapture
                                                # title reading on real captures (glyphs, app path, OCR); see below
cargo run --release -p maplelens-core --example bench_find -- FRAMES_DIR 0.9   # tooltip and Character Info search timings on real frames
cargo run --release -p maplelens-core --example bench_shop -- FRAME.png      # the shop watcher on one whole frame: scale found, status, ms a look
cargo run --release -p maplelens-core --example build_shop_font -- --init C:/Windows/Fonts/arial.ttf --rounds 10
                                                # relearn the shop fonts from tests/fixtures/shop_text.tsv (--fold K/5, --check)
cargo run -p maplelens-core --example shop_lines -- OUT_DIR   # every distinct text line in the shop fixtures, to label
cargo test --release -p maplelens live_shop_text -- --ignored --nocapture   # the live store read by the fonts and by OCR
cargo run --release -p maplelens-core --example chat_frames -- FRAMES_DIR   # per frame: chat lines found, scroll thumb (locate_frames: scale, markers)
MAPLELENS_NAMES=debug/names MAPLELENS_NAME_LABELS=<labels.tsv> cargo test --release -p maplelens name_bench -- --ignored --nocapture
                                                # Character Info names on saved windows (--debug saves them)
```

CI (`.github/workflows/ci.yml`) runs all of these in parallel jobs, each restoring its dependencies' build from a
cache (rust-cache; keyed on the lock file, toolchain and env, so keep `env` identical in `release.yml`); workspace lints in
`Cargo.toml` include `undocumented_unsafe_blocks`, so an `unsafe` without a `// SAFETY:` comment fails the build.

Run tests, clippy and fmt before declaring work done. Never claim an overlay change works without either a test or
a screenshot of the running overlay over the game (`maplelens --screenshot OUT.png` draws the cards in).

## Hard rules

1. **Screen pixels only.** Never inject into, hook, open, or read memory of the game process. Capture is GDI
   `BitBlt` of the screen (Windows) or the browser's window sharing; the game window is found through the process
   list snapshot (names only, no process handle) and the window manager. No input is ever sent (no `SendInput`,
   no `keybd_event`, no hooks; the hotkey is `RegisterHotKey`). The one other capture is the Store Permit watcher's
   (`wgc.rs`): it must keep working while the game is covered, so it takes the game window's rendered frames through
   Windows Graphics Capture (what OBS's window capture uses), still touching nothing in the process.
2. **Never commit or bundle MesoWatch data.** Keep `data/*.json` git-ignored, and don't write docs or code that tell
   users how to download it. Fixtures and screenshots use hand-written `Item`s (`market::sample`). The maintainer
   has MesoWatch's permission for local use; that is personal and does not extend to other users or to bundling.
   The web app fetches the list itself under a separate written agreement with MesoWatch, on one condition: **at most
   one request every 30 minutes, with a localStorage cache between** (`docs/app/prices.js` stamps every attempt, success
   or failure, before asking, so a reload, a second tab or a failure never makes a second request inside 30 minutes;
   `MIN_GAP_MS` must not be lowered). It never rehosts the data: no copy in the repo, the site or a workflow. MesoWatch
   serves list.json without CORS headers, so every client (the web page and the Windows app alike) reads it through
   MapleLens's relay: a Cloudflare Worker kept outside this repo (fixed upstream, 30-minute cache, CORS for the site
   only, 10 requests a minute per client, MesoWatch's `notice` and `terms` fields dropped at their request).
   `PRICES_URL` in `prices.js` and `SNAPSHOT_URL` in `net.rs` point at it (`prices.maple.observer`; its old
   `maplelens-prices.prices-proxy.workers.dev` address stays up for copies released before); when it is unreachable the page says so and
   shows the demo items. An official API
   is promised: adopt it when it arrives and follow its conditions.
3. **The overlay must be click-through** and must never steal focus from the game.
4. **No per-frame guessing.** With a mouse (Windows), the tooltip is always at `mouse + config::TOOLTIP_OFFSET`
   (measured, not assumed); the card position comes from the live cursor on the UI tick; detection only supplies
   title and tooltip size. Do not add trackers/filters to paper over a wrong constant: re-measure it. Two
   exceptions: (a) the game's own clamp: a tooltip too tall for the room below the mouse is pushed up to the
   client's bottom edge, so `vision::locate` looks for it there (only when the usual spot found no item and a tall
   tooltip could not fit); the card then uses `min(mouse + offset, bottom - height)`. (b) Without a mouse position
   (the browser), `vision::find` searches the frame for tooltips (fill band + continuous borders + the usual checks).
   (c) The world map's tooltip is a different widget whose offset from the cursor was not measurable (two samples
   disagree by ~20 px), so `vision::map_tooltip` reads its text lines by OCR inside a bounded window next to the
   cursor and `MapHover.offset` is relative to the cursor; unknown text never shows a card (the lookup must find a
   map), and readings must agree several times (`mapinfo::vote`) before one counts.
5. **Overlays only over the game.** Nothing is drawn unless MapleStory is the foreground window, and the card is
   hidden if it would not fit inside the client area. (The shop watcher shows nothing itself, so it reads in the
   background; its status pill follows this rule.)
6. **UI calls stay on the main thread.** Detection threads talk to the UI only through channels
   (`workers.rs` -> `app.rs`); `LayeredWindow`, the tray and the settings window are only touched from `App`.
   In the browser, the engine lives in a worker and talks to the page only through messages.
7. **Never weaken the update chain** (`crates/win/src/update`). Only a manifest signed by `update::PUBLIC_KEY` is
   trusted; download URLs are built from the verified version, every redirect is checked against the GitHub host
   allow-list, size and SHA-256 must match, and the new exe must pass `--version` before the swap. Nothing installs
   without the player's click; a development build (`target\`) only opens the release page.

## Architecture

```
crates/core/src/          platform-free; everything here is unit-testable
  config.rs               every tunable number, with a comment saying why
  geometry.rs             Rect, Point, ui_scale (client height / 1200), UiScale (the status bar's readings, voted)
  scale_vote.rs           ScaleVote: a scale settled on the last 5 s of readings, never on one frame
  image.rs                RgbImage / GrayImage, separable resize, PNG decode/encode
  vision/
    tooltip.rs            tooltip_present, measure_width
    locate.rs             candidate_tops: a tooltip pushed up by the game's bottom clamp
    find.rs               find_tooltips: tooltips anywhere in a frame (no mouse; the browser)
    info_window.rs        the Character Info window by its title (ncc.rs: pyramid template matching)
    map_tooltip.rs        world-map tooltips: top-hat text mask, tint pre-check, title + box from OCR lines
    segment.rs            lines (dense band -> baseline) and glyph runs of a text mask
    glyphs.rs             Atlas: glyph matching, touching-letter splits, learning from confirmed reads
    bitmap.rs             glyph bitmaps and their mismatch cost
    reader.rs             TextReader: atlas first, Ocr trait fallback, cache; title_mask; ocr_image
    samples.rs            the labelled fixtures the bundled atlases (assets/glyphs/*.json) are built from
    pixel_font.rs         exact reading of the small UI's bitmap font (names, and shop lines at Ideal Ratio:
                          assets/glyphs/name_small.json)
    vector_font/          the anti-aliased UI font read by drawing it: templates, DP reader, learner
                          (assets/glyphs/shop_vector.{png,json}); gaps.rs: OCR fills only a reading's '?'
    gauge.rs              the status bar's EXP ("12,128[59.99%]"): 5x7 gauge font scaled like the game, grammar-bound decode
    minimap.rs            the minimap's title (street, map) as two grey lines for OCR, found by its WORLD button
    quickslot.rs          the 4x2 quickslots over the bar's right end, and their item counts (the game's 8x11 count
                          digits, exact at Ideal Ratio, sampled at each pixel's centre when scaled); item_icon.rs: which
                          item an icon is (colour match over its solid pixels)
  market/                 models (Item::from_raw), snapshot, catalog (exact / fuzzy / '?' wildcard),
                          insights, format, time (ISO-8601 without a date crate), sample
  ranking/                classicrank.com model, URL, response parsing, RankCache (pacing is the caller's)
  render/                 canvas (tiny-skia + ab_glyph, Ink text style), theme, font, card, glance, details,
                          origin, range_bar, gamewin, rank_card, status_pill, map_card, maple_qr, training_card -> RgbaImage;
                          maple_ui (game-style buttons, check boxes, slider), settings_view (the settings window:
                          picture + hit-testing; panel.rs: the phone's pairing panel)
  pipeline/               Screen trait + FrameScreen; HoverDetector (hover/elsewhere.rs: pushed-up tooltips and the
                          no-mouse search), InfoDetector, MapScanner (map.rs)
  mapinfo/                map helper data: models, resolve (OCR title -> map), vote (settles noisy reads), data
                          (maplestory.io JSON and the disk cache format, shared with 0.1.x)
  shop/                   Store Permit watcher: markers (the button pair and the Sold Items close box, by colour, and
                          the UI scale), layout (anchors, offsets, View: reference pixels read off the frame as
                          captured), buttons (whose store, open?), lines (column-ink text compare), soldlist,
                          chatlog, scrollbar, align (the double-count rules), text (a line read by the game's own
                          fonts), parse (text -> events), watcher (ShopWatcher: frame in, events out)
  training/               session (Tracker: EXP gains, level ups, deaths, idle time, rates, potion tallies, history JSON),
                          potions (candidates, PotionWatcher: quickslots -> totals per item, Potions), kpq (stages,
                          Cloto's answers, combinations, QuestTracker), watcher (TrainingWatcher: frames in, Training out)
  placement.rs, settings.rs, hotkey.rs (naming and building the show/hide combo)
crates/win/src/           the Windows app (only this crate talks to Win32)
  main.rs                 CLI, single instance, --screenshot
  app/                    UI loop: tray/settings events; follow.rs: card cache, placement from the live cursor
  workers.rs              hover / info / rank threads; refresh.rs: hourly validated snapshot updates
  game.rs                 game window, client rect, foreground, cursor, DPI awareness
  capture.rs              GdiScreen (BitBlt), layered.rs (overlay window), tray.rs, winclass.rs, startup.rs (Run key), ocr.rs (Windows.Media.Ocr), net.rs (ureq),
                          log.rs, paths.rs, icon.rs, screenshot.rs
  maps/                   map helper: scanner thread, maplestory.io client, sprite and map caches, lookup service;
                          app/map.rs: the card. ocr.rs also reads whole lines with positions (CachedLines)
  phone.rs                the phone pairing's relay requests (a thread); app/phone.rs: its presses and answers
  shop.rs, wgc.rs         the shop watcher thread (and `--shop`), fed by Windows Graphics Capture; app/status.rs:
                          the status pill
  training.rs             the training reader thread (window capture, Windows OCR of the minimap, training.json);
                          app/training.rs: the training card in the corner picked in the settings (placement.rs
                          place_in_corner: beside the minimap, above the status bar); tray: its switches and session
  update/                 signed self-update: version, manifest (Ed25519), net (allow-listed hosts), check
                          (daily thread), install (stage, self-test, swap, relaunch), release (--sign-release,
                          --make-update-key)
  settings_window/        the drawn settings window on a layered window (mod.rs); input.rs: pointer, Tab focus,
                          slider drag and the hotkey box listening for a combo
crates/web/src/           wasm-bindgen Engine: process(frame) -> JSON state, card pixels, rank and OCR queues
docs/                     website; docs/app = web app (app.js, lens.js, worker.js, pkg/ = compiled engine)
```

Dependency direction: `win`/`web` -> `core`. Inside core: `pipeline` -> (`vision`, `market`) -> (`image`,
`geometry`, `config`); `render` -> `market`. Core must not depend on Win32, threads or the clock: time is passed in
(`now`), I/O is the platform's.

## Conventions

- Rust 2024, `rustfmt.toml` (120 columns), clippy clean (workspace lints in `Cargo.toml`).
- Magic numbers become named constants (module-level, or `config.rs` if user-tunable). Say *why* in a short
  comment, not *what*. Comments explain non-obvious reasons (e.g. why a pyramid, why no margin). No narration.
- Modules stay small and single-purpose; past ~300 lines, split along a natural seam (see `pipeline/hover/elsewhere.rs`).
- Every `unsafe` block has a `// SAFETY:` comment. Win32 is only in `crates/win`.
- Errors: specific error types at boundaries (`SnapshotError`); the UI tick catches panics and logs, so one bad
  frame never kills the overlay. Logging through the `info!/warn!/error!/debug!` macros of `win/src/log.rs`.
- Drawing: lay out in design units (the 1x card) with `Canvas`; never resample a finished card.

## Training: EXP tracker and party quest helper (`core/src/training`)

Frame in, `Training` out, about once a second (`TrainingWatcher` paces itself; the app runs it on its own thread from
window capture, the browser from every frame it is given). Sessions are timed by the wall clock (`now` is Unix
seconds) so the history has dates; the platform keeps `Tracker::to_json` (training.json / localStorage).
- **EXP** (`vision::gauge`): the gauge font is drawn 1:1 at Ideal Ratio and smoothed at 1.5625x in Normal; the read
  area is relative to the status bar and scaled by `bar.width / 800`. Glyphs are scaled the same way and the cheapest
  run is decoded under the printed shape (number with separators, `[`, 1-3 digits, `.`, two decimals, `%]`), which is
  what separates `[` from `1` in the blur. Reads with more than `MAX_COST_PER_PX` mismatch are no read (a window over
  the gauge). Fixtures: `status_bar_*` (both UI sizes, four values).
- **Sessions** (`training::session`): a value counts once two reads agree. A share drop of 20+ points with less EXP is a
  level up (the rest of the old level plus the new EXP), unless the new level is not bigger, or more than twice as
  big, than the old one: that is another character, which counts nothing. The level's size (EXP / share) also tells a
  death (same size, less EXP: counted as lost) from a character switch. Time counts while the bar is read and EXP
  keeps coming; a minute without EXP or a move to another map (not paused) ends the session into the history, and
  so does a gap over 30 minutes unseen. A session starts at its first EXP, and so does the card (`is_training`: a
  player training wants it at once); its rates fill in after 10 s of play (`MIN_RATE_S`).
- **Map** (`vision::minimap` + OCR): the title's two lines are cut as grey "whiteness" images, not masks (a mask broke
  "Street" into "Stl eet"), and OCR'd with a wide white border (tight, Windows OCR read nothing at Ideal Ratio). Readings
  settle in `training::map_name`: the look-alikes of I folded (l, 1, |), the last word kept apart when it is a numeral,
  and two readings are one map when their numerals agree (or one is missing) and their words score 90+. A different
  map counts on two readings in a row. Neither exact nor fuzzy alone works: the browser's frames are video, so
  Tesseract reads "III" as "Ill" and an exact match ended the session on every such pair; neighbouring maps differ by
  the numeral alone ("East Rocky Mountain V" and "VI"), and `mapinfo::vote` held the old one for as long as the
  player stayed. The pointer over the title is no read (it changes what OCR sees). OCR leaves out the Free Market's room ("<8>") about half the time; KPQ stage names
  keep their words. Only the default top-left minimap is found (its WORLD button anchors it).
- **Potions** (`vision::quickslot`, `vision::item_icon`, `training::potions`): the quickslots are a fixed offset from
  the status bar (measured at Ideal Ratio and Normal). Counts are read with the game's own count digits: crisp, every
  pixel matches; scaled, the game draws them blocky, so each 1x pixel is sampled at the centre of its block over a few
  sub-pixel phases, and of the digits that fit the one explaining most pixels wins (a 0 fits inside an 8). Items are
  told by their icon in colour (the Red, Orange and White Potions are one shape), from maplestory.io's Classic list of
  Use items 2000000-2022999; icons are the game's artwork, so they are downloaded at run time (app: a disk cache;
  web: the browser's cache), never committed (tests read them from `tests/fixtures/icons`). A slot is identified again
  only when its count changes (or every 10 s); totals per item count once two looks agree; a slot gone unreadable
  holds its item's total back for 5 s (a window or the pointer over it), and an item dragged out of the quickslots
  is no longer followed. The session counts a total that falls as used and one that rises as picked up (buying from
  an NPC reads as picked up).
- **Party quest** (`training::kpq`): stages by map name (fuzzy, letters and digits only), a run from the first stage
  until the party has been off the quest's maps for a minute. Facts (Cloto's answers, combinations) are written out in
  our own words. There is no real KPQ capture yet: the stage path was checked on a synthetic minimap (Tahoma Bold
  on the minimap panel) with both OCR engines; add a real one when there is one.
- **The party quest helper is hidden** until a real run has checked it, and is not mentioned on the website or in the
  README. Windows: the `quest_helper` setting is off, with no switch in the settings window or the tray; only
  settings.json turns it on. Web: the engine never draws it (`set_training_show(exp)`) and the page has no panel for
  it (the panel, its guide JSON and the switch are in PR #46's history). The tracker still follows the quest, and its
  tests and `render_cards`' `training_kpq_*.png` still cover it.

## Store Permit watcher (`core/src/shop`)

Frame in, events out. The app always runs it (the pill needs it); `--debug` also prints its events, `--shop` runs
it alone in the console.
- **Never dedupe by content.** Identical sold rows are real (the same buyer buying 1000 arrows twice); new entries
  are found by sliding the new page against the old one (`align`) and ties resolve to the smaller shift. A page of
  one repeated row cannot be counted that way: the meso balance moving then yields `AmbiguousSale`.
- Compare text by column ink (`lines::same`), never pixel-for-pixel, and never compare whole row crops: slots are a
  fractional number of pixels apart.
- A part of the shop is only believed once two consecutive frames agree on it (the game draws its mouse pointer
  into the frame), and only while its scroll thumb shows the live end.
- Only our own *open* store is watched: OPEN STORE greyed + SOLD ITEMS / CLOSE STORE labels (`buttons`). Someone
  else's store and ours while being set up are ignored. Closed = back to its set-up buttons for `STORE_GONE_AFTER` (5 s), or no shop window at all for
  `STORE_VANISHED_AFTER` (60 s): switching resolution, ratio or full screen hides the window 15-30 s, and a 5 s close
  announced a false closing and reopening each time.
- The shop finds itself and its own scale (`markers`): the lime SOLD ITEMS button with the orange CLOSE STORE button
  80 px (scaled) below it, the same width, found as colour runs on the frame as captured (a pointer-sized gap in a
  run is bridged: the pointer over a button splits every row of it). The button's width gives the scale to ~1%,
  refined to ~0.25% by the chat page's width between its blue-tinted borders; within 0.3% of 1 it is 1. The status
  bar's scale is not used: on 10 live captures it was wrong for 2 (Ideal Ratio at 1080p, a 1440p window), and every
  offset with it. The Sold Items dialog is its blue close box at the end of a long white bar, with list cells below.
- Offsets are measured at UI scale 1 from those markers; `layout::View` crops any part in reference pixels, resampling
  only that part (never the whole frame), and text masks are taken from the frame's own pixels. Scale and markers
  are held while new measures are within 1.2% / 2 px: a moved marker reads as a dragged window and forgets the lists.
  A measure further off rescales only once the last 5 s of measures settle on it (`watcher::settle_scale`, a
  `ScaleVote`): the pointer over a button once made one frame read 3% off and forgot every list. After 5 s without
  the shop (a resolution switch hides it 15-30 s) the first measure off the held scale is believed, as before; one
  within 1.2% of it stays the held scale (the pointer over the buttons for a while must not forget the lists).
- When the buttons are not found but the chat page is still where it was (a tooltip or the pointer over them), the
  look brings no news rather than a closing. A tooltip's dark blue reaching over the chat page makes it covered: its
  edge would read as a line.
- Before the markers (0.2.1), one small ENTER template anchored everything and the frame was resized to scale 1 every
  look: 5 of 9 live captures found the shop, 45-110 ms a cold look. Now 9 of 9, 4-14 ms (`bench_shop`).
- Shop OCR upscales bicubically (`shop.rs`): bilinear turns "D" into "O".
- Fixtures `tests/fixtures/shop_*.png` are crops of native 1920x1200 captures; the tests put them back in place.
  `shop_live_*.png` are crops of other client sizes (768p, Ideal Ratio 1080p, 1440p at 1.2 and 1.28), put back in a
  client of that size (`tests::live`).

### Parity with the Python watcher, names, and where the events go

The retired Python watcher (branch `claude/store-permit-detector-040680`, `scripts/replay_shop.py`) is the reference:
`MAPLELENS_SHOP_FRAMES=<its debug/shop> cargo test --release -p maplelens shop_replay -- --ignored --nocapture` replays the
same frames through the Rust watcher (373 real captures, 22 ms a frame against Python's ~78). Event for event they agree
(461 events, the 7 sales identical); live, over 25 minutes beside each other, 27 of 27 visitor events agreed (Rust lags
~1-3 s: it polls once a second). `--shop` runs beside the app for such comparisons.
- Names are settled (`shop::names::NameBook`): a read is matched against the players seen, those inside the store first
  for a leaver or buyer (a third of the letters may be off there), and reported as the most-read spelling. On the corpus,
  leavers whose name differed from their entrance fell from 58 of 221 to 9. A name unreadable on entering ("?") stays so.
- Sold items snap to the catalog (`ShopWatcher::set_catalog`); a tie between two items names neither.
- Notifications (`core::notify`): `Connector` turns a batch into HTTP requests, the platform sends them (Windows: the
  `notifier` thread, batching a burst, pacing, waiting out 429s; browser: the worker's `fetch`). `Discord` accepts only
  `discord.com/api/webhooks/<id>/<token>` URLs, sends embeds with `allowed_mentions` empty and names escaped, and its
  token is never logged or shown whole. `Telegram` is one URL like a Discord webhook,
  `https://api.telegram.org/bot<token>/sendMessage?chat_id=<chat>` (`telegram::parse_webhook` / `webhook_url`). A URL
  without the chat, or a token alone (found in a paste of @BotFather's message), is kept without it: the player sends
  the bot /start, and Test asks the bot who wrote last (`getUpdates`, POSTed like everything else), completes the URL
  and puts it on the clipboard. Its
  messages are HTML with names escaped and '@' kept from mentioning. Telegram's Bot API allows any origin, so the
  browser sends straight to it too. Settings: `discord_webhook`, `telegram_webhook`, `notify_sales`, `notify_store`. Refusals are worded by `notify::refusal`, waits read by `notify::retry_after`, for both platforms.
  A new connector is a type and a settings field. The item's picture (a Discord thumbnail, a Telegram `sendPhoto`
  whose caption is the message, sent again as text if refused) is maplestory.io's MapleStory Classic World data
  (`config::ITEM_API`, region MCW), which numbers and names items as the game does: the picture of the sold item's own
  id, with no request (1,866 of the price list's 1,867 items are there; 200 of 200 sampled icons loaded; meso.watch's
  icons are copies of the same data). Only a sale without an id is searched by name (`notify::picture`), each name
  once, and the browser holds a batch up to 8 s for those answers. The GMS v40 data used before (still the map
  helper's) numbers items differently and names 465 of the 1,867 otherwise (Classic's Iron Ingot is its Steel Plate),
  so a quarter of the sales had no picture. `cargo test -p maplelens live_discord -- --ignored` sends to
  `MAPLELENS_DISCORD_WEBHOOK`; `live_telegram` links and writes through the bot in `MAPLELENS_TELEGRAM_BOT`.
- Phone notifications (`notify::phone`, setting `phone_key`): Web Push through MapleLens's relay, `relay/push` (a
  Cloudflare Worker in this repo at `https://push.maple.observer`, behind that zone's firewall rate limit: `npx wrangler deploy` from that folder, `npm test` there for its crypto; VAPID and
  aes128gcm are hand-written on WebCrypto, no dependency). A pairing key is `<id>.<secret>` (made by the page,
  `phone/relay.js` `newKey`); the relay stores only the secret's SHA-256 and the phones' subscriptions (a Durable Object
  per pairing, the pairing's only record: KV cached a missing pairing for a minute per location), never a message, and accepts push endpoints on the browsers' own push hosts only. The web app shows the key as a QR code of
  `docs/app/phone/?pair=<key>` (hidden after two minutes or once `/status` sees a phone: a streamed screen is public);
  the phone page subscribes and pairs. A batch of sales is one notification, with the item's picture (`Sale.picture`) when it is one
  sale; store changes share a tag. iPhone pushes
  only to home-screen apps, so the phone manifest has no `start_url` (the installed app opens the address with its
  key; an installed app's storage is its own) and the app can also pair by scanning in-page (BarcodeDetector, else
  vendored jsQR). Android needs no install, but Chrome offers it only for a manifest that names its `start_url`
  ("start-url-not-valid") and a service worker with a fetch handler, so every other browser is pointed at
  `install.webmanifest` (`start_url` "./": an Android app shares Chrome's storage and finds the kept key), the worker
  is registered before pairing, and it answers navigations (offline: a short page). A phone follows one pairing: a code for another one (a link anyone can
  post) switches it only after a tap, joining the new pairing before leaving the old, and the key leaves the address
  bar once kept (not in an iPhone Safari tab, which installs from that address). A pairing takes 5 phones; a sixth is
  refused (409), never one dropped silently, and an unused pairing is deleted after 60 days. The website's QR (`docs/assets/phone-qr.svg`) is
  `cargo run -p maplelens-core --example phone_qr`. Both apps pair one phone at a time: once one is connected there is
  no Connect, and Remove is the way to another (the relay still takes 5, for pairings made before). The pairing
  steps live in a modal (`#phone-dialog` on the page; on Windows a panel over the settings window,
  `render/settings_view/panel.rs`): the code with numbered steps, then "connected" with Send a test, or "hidden" after
  two minutes with Show it again; closing it wipes the code. What every relay answer means is `notify::pairing`
  (shared wording, tested); the Windows app's requests are `crate::phone` (a thread: `/state/ping` every 4 s while
  the code is up, each minute otherwise; `/notify`; `/unpair`), and the QR is drawn by `render::maple_qr`, the same
  design as `mapleqr.js` and `examples/phone_qr`. The Windows app keeps the same store book (`notify::storebook`, the
  page's JSON shape; fed by the shop thread, pictures by the notifier, saved to `storebook.json`) and uploads it from
  that thread (`/state/put` every 5 s at most while a phone is paired and it changed), sealed by `seal.rs` exactly as
  `relay.js` `seal` (HKDF-SHA256 + AES-256-GCM on Windows' CNG; tested on RFC 5869 / NIST vectors, and the phone's
  `unseal` opens it). `cargo test -p maplelens live_phone_relay -- --ignored` walks a fresh key through the real relay.
- The phone app is also a dashboard of the store: the page keeps a store book (`docs/app/storebook.js`: sales with item
  pictures (a sale event carries its item's; one found by name the worker posts: `Engine::picture_url`), chat, visits, who is inside, status, owner, mesos; kept in localStorage), seals it with AES-GCM under
  HKDF(pairing secret) (`phone/relay.js` `seal`) and uploads it when it changed (every 5 s at most; a check-in each
  minute otherwise). Each waits for the one before, so every relay request times out (10 s, as on Windows): one left
  hanging had stopped the check-ins and the phone showed the computer offline while the page ran. A check-in may go a
  sync early: a tab the game covers wakes once a minute, and one due on the dot came every two minutes. The relay
  keeps the book in a Durable Object per pairing and never has the key. `?demo` on the phone
  page shows the dashboard with made-up data. Remove on the computer tombstones the key (410 everywhere) and pushes a
  "disconnected" notification; the phone app clears itself. A phone leaving is seen by the page's next check-in.
  `relay/push/README.md` has the routes, limits, deploy steps and the security model; `npm test` there.
- The browser runs the same watcher (`web/src/shopwatch.rs`). Its OCR is asynchronous: the reader answers None while
  text is being read. Only a sale or a visitor whose text is not in (`ShopWatcher::missing_text`) undoes the frame (the
  watcher is cloned first) to try again when the answers are in; text the watcher reads for itself (the owner's name,
  the counter's first value) is asked again next look and counts as a try only once answered. Undoing every frame with
  any pending text held the store "not in view" for as long as Tesseract took to load (seconds) and spent the owner's
  tries on unanswered reads. Lines are read by the game's own fonts
  first (`shop::text`), so the browser asks Tesseract only for letters the fonts have not learnt (a sale of known
  letters needs no OCR at all: `the_shop_is_followed_in_the_browser_and_its_sales_are_read_without_ocr`).
- The status the player sees is core's: `status_pill::text_for` / `tone_for` give the Windows pill's wording ("Press the
  SOLD ITEMS button", bold on the page and the phone) and its tone, and the browser shows exactly that on the page and in the floating window
  (the bottom line of `cards.js`'s view). Tested live against the real game window (Chrome started with
  `--auto-select-desktop-capture-source=MapleStory --remote-debugging-port`, the share started over DevTools with a user
  gesture): "Store detected", 24 ms mean a frame at 1080p-class, and the visitors the Windows watcher reported.
- A browser's frames are video: colour is kept at half resolution, so colour edges move by a pixel and the scale reads
  ~0.2% off (1.1975 for 1.2 at 1440p). A scroll track's ends had sat exactly on its arrow buttons; a pixel off, the
  arrow's blue read as the thumb at the start and the live chat as scrolled back, and the page reported no visitor
  for minutes. `thumb_position` looks past the track's ends and drops the blue runs touching them (the arrows).
  `replay` with `REPLAY_OCR_LAG=N` answers OCR N looks late, as Tesseract does.
- The shop watcher must get the whole frame, never the title-bar-cropped one: the page's inset guess cycles while no
  tooltip is found, which moved the shop window under the watcher and made it forget its lists (it saw no event for 15
  minutes of a live store until this was found). `state.shop.looks` / `undone` (and `maplelensStats().shop`) say how
  often it looked and how often a look waited for text.

### Shop text: read by the game's own fonts (`shop::text`)

The game draws the shop's text two ways, and each is read exactly, OCR only filling letters a font has not learnt:
- **Ideal Ratio (UI scale 0.641) and its 1440p double (1.282): a 1-bit bitmap font**, not one mid-grey pixel. It is the
  font the Character Info names use, so both read from one table (`assets/glyphs/name_small.json`,
  `vision::pixel_font`): `line_mask` binarises a line, collapses the doubled grid, anchors rows on the baseline (row 10
  of the table); `read_line` adds a space for a gap of 3+ font pixels (never inside a number: a narrow '1' leaves one).
  Names (the status bar, the name plate) draw 'd' with its bowl touching the stem, unlike the chat's 'd': the table
  spelt it as 'c' and 'l' touching, with nothing unknown, and a store kept "...cl..." as its owner (1366x768). Its shape
  is learnt from `shop_render_768`, a scene drawn by the game's own engine (mscw-render), not a capture.
- **Default ratio (0.9 at 1080p, 1.0 at 1200p, 1.2 at 1440p): a vector font at fractional positions** (Arial-like, 19.05
  px per em at scale 1, its size proportional to the scale). The same letter is never the same pixels twice and letters
  touch, which is why the binarised glyph atlas POC read 13% of lines. `vision::vector_font` holds each letter as a
  darkness template on a grid 4x finer than a reference pixel and draws it at any scale and quarter-pixel position (a
  frame pixel is the template's mean over its footprint). Reading is a dynamic program over pen positions (no
  segmentation): each letter's squared error over its advance cell, the previous letter's spill ('f', 't') taken off,
  a per-letter cost so 'm' beats 'rn' (more for punctuation, or a speck in a word gap reads ':'), letters up to 1 px
  off their advance at a cost (the game kerns pairs; 'I' and 'l' differ mostly by advance), a blank run crossed in
  one step, and an unknown span at `UNKNOWN_COST` a column that reads '?' when no letter explains the ink. A letter
  touching a '?' joins it: an unseen 'w' otherwise reads '?v'. Each position tries every letter at its advance and
  only the 20 best fits off it (`SCREEN`): 95 -> 23 ms a line natively, the same readings.
- A line is prepared before reading: paper and ink from the line itself, the box's edges cleared (a full-height
  column is the counter's frame; a full-width row's faint pixels a sold row's separator, while a descender crossing
  it stays), and the baseline taken as the row most inked columns end on (wide descenders, "Smoggy", fooled a
  dense-band estimate by 1.3 px), refined by fitting the font's row profile.
- One template fits every default scale: trained on two of 0.9 / 1.0 / 1.2, the held-out one read every line whose
  letters were known. Templates are learnt from the game's pixels by `examples/build_shop_font` from the labelled corpus
  `tests/fixtures/shop_text.tsv` (every distinct line the watcher reads in the shop fixtures, transcribed by eye;
  `examples/shop_lines` lists them): forced alignment by the same DP, then least-squares steps on full-line residuals.
  Training aligns with a ±1.5 px window (reading ±1 at a cost): with a narrow one an advance far from the game's never
  converged and its letter learnt blurred (a doubled 'I', 0.87 error a column; 0.05 with the wide one). `--init` takes
  a font file's outlines as a starting point (a dev machine's Arial); only templates learnt from game pixels are written.
- Capital I and lowercase l are one bar in both fonts: `settle_bars` makes a bar between capitals an I, between
  lowercase letters an l, after a capital and before lowercase an l ("Blvd": Windows OCR said "BIvd"), and starting a
  word an I before a consonant and an l before a vowel. Item names snap to the
  catalog, whose look-alikes include l/I; visitor names are keyed with them folded.
- In the watcher (`line_text`): the font first; a line with no '?' is final, never sent to OCR. Otherwise OCR's reading
  fills the gaps only where it agrees with every letter the font is sure of (`vision::gaps::fill_gaps`, shared with the
  Character Info names), else OCR's reading. Reads are cached by the line's pixels. The status bar's owner name stays
  OCR (another font); the name plate is read by the table at Ideal Ratio only.
- Measured on names the font never saw (`build_shop_font --fold K/5` holds out every line of a fifth of the names; the
  five folds run in parallel from the built example in ~9 minutes): of 284 held-out lines 206 (72.5%) exact, 73 with
  '?' gaps only, 5 wrong (two "looking4deal" read "Iooking4deal", one i/l in "Received"). With 1440p default-ratio
  lines added and letters let sit +-1.5 px off their advance (at 1.2 the letter after an 'a' sits further than a pixel
  off: "lazy", "Crazyyyy" read '?' there): of 325, 242 (74.5%) exact, 68 gaps only, 15 wrong, of which one is a name
  (an i read as l); the rest are pangram I/l bars and a speck read as '.' in "left.the room." (the phrase still matches). The bitmap table reads 48
  of 50 corpus lines (64 shapes; the two are an I/l bar `settle_bars` settles). Live, a store at Ideal Ratio: all 21
  lines exact at ~0.1 ms each, four of them visitors in no training data; Windows OCR read 3 of the 21 wrong
  ("Crazfy•yy", "Total Sales Total Received", "1 47,066"). In the browser: 29 ms median a frame, a one-time ~0.4 s
  frame when a store's lines are first read.
- Corpus labels are by eye and can be wrong: `letter_debug` (`MAPLELENS_LETTER=R`) shows each line's fit of a letter,
  which is how "Rawyn" (really "Ravyn") was found dragging 'R' off; `line_debug` shows one line's darkness, baseline
  and readings. New letters come from new captures: add the fixture, list its lines (`shop_lines`), transcribe them
  into the TSV, rerun `build_shop_font --init C:/Windows/Fonts/arial.ttf --rounds 10`.

Notifications (`core/src/notify`): `Announcer` turns watcher events into `Notification::{Sale, Store}` (Discord webhook and
Telegram bot connectors; the webhook and the token are settings, never committed). Every message names the store's owner: the character's name is
read from the status bar (under the job, right of the LV box: `vision::hud::find_bar` on the frame as captured, whose
1 px outline resampling had lost at 1366x768, + `HUD_NAME`, wide enough for the pixel font's long names; light ink at
low levels, voted over several reads), falling back to the shop window's name plate respelled from the chat log (at the
pixel-font scales the plate goes first: read exactly, it needs no vote). There is
no typed-name setting. A store announcement waits up to `OWNER_WAIT_S` for the name.

## The site's origin holds secrets

The project's domain is `maple.observer` (a Cloudflare zone on the same account): the bare and `www` names redirect
to the site with the same path (`relay/site`: a redirect, never a copy on a second origin), `push.` is the phone
relay and `prices.` the price relay, all under one firewall rate limit (`relay/push/README.md`, "Rate limits").

`maplelens.github.io` is one origin for the website, the web app and the phone app, and its localStorage holds the
player's Discord webhook, Telegram bot token and phone pairing key. So no page of the site runs a script from another server: Tesseract
and jsQR are vendored (`node scripts/vendor.mjs`: pinned versions checked against the npm registry's integrity,
files against each folder's SHA256SUMS; `--check` runs in CI), GoatCounter's script is `docs/assets/goatcounter-count.v4.js`,
and every page has a Content Security Policy (`script-src 'self'`, connections only to the hosts it uses). CI fails on
a `<script src="https://...">` or a dynamic import from another host. Adding a host to a page means adding it to that
page's policy; a new GitHub Pages repository on this account would share the origin, so there must not be one.

## Reading titles: how it is measured

`crates/win/src/reading_bench.rs` runs a folder of real tooltip captures (the `debug\` output of `--debug` play
sessions; not committed, they are players' screens) through the glyph atlas alone, the app's full path and Windows OCR
alone. On 792 hand-labelled captures (723 labelled with an item, 69 other screens): the app shows the right card for
665 and 1 wrong card in 666 (before the bullet and credibility rules: 41 wrong cards); the bundled atlas alone (the
browser without OCR) names 703 with no wrong item. About 40 of the item-labelled captures are not tooltips at all
(shop-list rows, quest-log name boxes) and rightly show nothing. The real tooltips still missed are frames where
the tooltip lagged a moving pointer (2-20 px off the measured spot, in any direction; the next frames catch it, and
rule 4 forbids searching for it) and Cash Shop pets (their translucent fill over the shop's dark blue matches no
fill test that keeps other blue UI out). A wrong price is worse than none, so precision comes first. Before
changing the reader, the segmenter, the atlas or the catalog lookup, run it and compare. The rules:
- The title is cut around its bullet (`reader::find_bullet`), so scenery beside a shifted tooltip is not read.
- A catalog match counts only with tooltip evidence (`HoverDetector::credible`): the bullet, the blue fill, or an
  atlas read of 6+ letters. OCR reads shop lists and quest logs too, and their text names items.
- A `?` wildcard that fits several items names none; a glyph that nearly matches two letters reads as `?`.
- When OCR fills in the atlas's unknown glyphs and the result names nothing, the atlas's own read (gaps as `?`) is
  tried too: OCR rereads the whole title and can lose letters the atlas had ('SiI?er Ore' -> 'Ore').
- Learning only takes bullet-anchored titles, and refuses a sample that is plainly another letter.
- New letters for the bundled atlas come from `title_*.png` crops in `vision::samples`.

Character Info names (`pipeline/info.rs`): `--debug` saves each new read's window to `debug\names\`; `name_bench`
reads a labelled folder of them (labels: file, tab, name) and `learn_pixel_names` rebuilds the small-UI name font
from it (`assets/glyphs/name_small.json`), reporting each name read with letters learnt from the *other* windows.
On 31 windows at Ideal Ratio, that held-out test names 30 right, 0 wrong, 1 not at all; OCR alone named 27 right
and 3 wrong, and with letters the 1x atlas had learnt from that font, 9 of 22.
- At Ideal Ratio the title is matched with its own capture (`character_info_title_ideal_ratio.png`): the small UI
  has its own lettering.
- Names there are a bitmap font: every letter the same pixels on the same rows, a blank column before the next. So
  `vision::pixel_font` reads them exactly (cut at blank columns, look each piece up; letters drawn touching are a
  piece of several letters). A piece not in the table is unknown, never a look-alike; only those gaps go to OCR, on
  the game's pixels enlarged 3x in whole blocks, and OCR's reading counts only where it agrees with the exact
  letters (`fill_gaps`). Names the rank lookup confirms teach their pieces (`glyphs-name-small.json`), which is safe
  because matching is exact. Never put this font through the 1x atlas: its tolerant matching turns the small
  letters into look-alikes, and what it learns from them misreads every later name.
- A name counts once two polls read it alike; one odd read keeps the settled name.
- Capital I and lowercase l are the same bar in that font: an unknown name is retried with its I/l look-alikes
  (`ranking::look_alikes`, at most 3).

## The UI scale is voted, never one frame's

`UiScale` (shared by the hover, info and map threads, and the browser engine) takes the status bar's width as a
reading (`config::STATUS_BAR_CHECK_S`, by the hover thread and, on Windows, the shop thread too) and answers with
`ScaleVote`'s decision: the readings of the last `SCALE_VOTE_WINDOW_S` (5 s), clustered within
`SCALE_VOTE_TOLERANCE` (1%), the largest cluster's median once it has `SCALE_VOTE_MIN_READINGS` (3) and is more than
half the window; otherwise the value already settled stays. A real change (a resize, "UI Size", full screen) takes
over in ~3 s; before the first decision the client height stands in. Readings are kept per client, and a client of
the same width and a height within 48 px is the same one: the browser's title bar guess (0/31/39/47 px, cycled while
nothing is detected) had changed the client every 1.5 s and thrown the measured scale away each time, so in Ideal
Ratio the page flipped between 0.64 and the height's 0.9. Every writer must time readings on one clock: on Windows
`paths::uptime`, in the browser `performance.now()`. A reading up to 1 s older than the newest goes in its place
(the hover thread reads the clock before its grab); a clock that goes back further starts the window over.

A map smaller than the client (Kerning City Pharmacy) is drawn with black around it, and that black joins the bar's
1 px outline: its top row is dark from edge to edge, and every reading there was the whole width (1.53 in Ideal
Ratio), so the vote settled on it and no tooltip was found. `hud::side` steps out of such a black area to the last
column before the bar's light inside; only then are the sides, not the run, checked for centring, and the row under
the top must be the bar's inside (black above it too). On 1,071 saved frames this changed only frames with black or
dark scenery beside the bar, each to the true width.

## Stream footage

Twitch footage (1080p, H.264) is how detections are reviewed over time without a play session: `stream-bench.ps1`
records a channel, extracts 1 frame per second and runs the browser's no-mouse path with Windows OCR and the local
price snapshot. It is compressed and sometimes cropped into a layout, so it measures recall and false positives,
never pixel-exact reading, and is never a fixture (fixtures are native captures). Things it taught that hold in
general:
- The status bar's 1 px outline does not survive compression, so the UI scale is not measured there: pass
  `MAPLELENS_UI_SCALE` (tooltip width / 450, or bar width / 1250). With the wrong scale nothing matches.
- A Twitch ad slate and a streamer's webcam look like windows to correlation alone: the Character Info title also
  needs its ink and paper (`info_window::has_title_contrast`).
- Tooltip fill over dark scenery is dark (18,19,60) or keeps the backdrop's hue (34,21,60), and its border over
  black is as dark as the fill: `is_fill` has a dark class, and `find` counts unreadable border pixels as unknown.
- Over a warm backdrop (Perion's orange rock, smoke over it) the fill is a dull purple, (78,65,93) to (83,77,116), and
  the browser found an equipment tooltip there in 3 of 59 frames of a live capture at Ideal Ratio (it showed nothing
  for as long as the player hovered). Three things missed it: `is_fill` has a warm class (blue leads red by 8-40, red
  at least green); the right border takes the rock's colour, light in red but as dark as the fill in blue, so
  `measure_width_lit` reads it by the brightest channel when the darkest shows nothing; and the 1.56x enlargement at
  Ideal Ratio smears the 1 px top line over two rows, so a line reading 0.7-0.8 counts while the other reads 0.95+.
  Now 58 of 58 frames with the tooltip find its top (`tooltip_ideal_ratio_*` fixtures). Lines inside the tooltip are
  found too; a spot without a title bullet no longer resets the unknown-title count, which had kept an item with no
  price from ever showing.
- Enhanced equipment has an orange title and a "(+3)" suffix (`reader::title_gray`, `catalog::without_enhancement`).
  The card shows the unenhanced item's price.

## The browser version

`docs/app` is the page (`app.js`: sources, pacing, stats; `cardfeed.js`: what the cards show and when; `cards.js`: its
view, in the page and in the floating window; `rankpanel.js`: the web's rank panel; `lens.js`/`worker.js`: the engine in
a worker). It measures itself: `window.maplelensStats()` gives frames,
dropped frames, engine time (avg / p50 / p95 / max), long tasks, heap, and `?report=label` posts the same to
`/report` of whatever served the page. Test sources, all through the code a shared window uses:
`?demo`, `?video=/clip.mp4` (a recording played in a loop), `?capture=/clip.mp4` (the clip as a media stream: the
track-processor path), `&scale=0.9` (the UI size by hand: the status bar's outline does not survive compression),
`&fps=`, `?preview=pip&size=340x560` (the floating window's view inside the page, resizable), `?demo&blink` (the sample's
tooltip and Character Info window come and go: 5 s shown in every 16). `window.maplelensCards` is the card feed. Chrome
started with `--js-flags=--expose-gc` makes the reported heap live data.

The page is deliberately bare: Share game window, the cards (Float over the game), the store log, the Discord webhook and
the Telegram bot (kept in localStorage, shown masked with a Change button). There is no settings panel; Tesseract (tesseract.js 7.0.0, served from `docs/app/vendor/`, loaded when
the engine meets text the atlas cannot read) is always on, for shop text and unknown letters. It reads a tooltip's
title or a player's name before anything else waiting (`lens.js`): the minimap's lines and the shop's can wait, and a
title queued behind them came late. A grey line alike one read lately (mean difference 6 of 255) takes that answer
(`web/src/ocr.rs`): video frames never repeat pixels, so the minimap's title went to Tesseract on every look.
The Training and Store Permit panels show only while they are about something (`showPanels`): a training session
(the engine's `training.active`, the card's own rule, `training_card::is_training`), the player's own store window (any
shop status but "hidden"), each kept 20 s after (a window over the game must not make it blink). Unfollowed, the store panel stays, or it could never be followed again. A line says what shows up
there while none does.

The cards (`cardfeed.js`, shown by `cards.js` in the page and the floating window alike):
- A card is live while its tooltip is read; once it is gone it stays `LINGER_MS` (6 s) under a bar that runs out, the
  time standing still while the pointer is over the view (the player moved there to read it), then goes. Before 0.2.6
  the canvas's `display:block` beat its `hidden` attribute, so a card never went away.
- Every item hovered is kept (six, newest first, with its card): the idle view lists them with price and trend, and a
  click shows that card again until the next tooltip or Back. The engine says what a card is about (`card_about`: the
  catalog's name, the price as the card writes it, the trend) in the same message as its pixels.
- The engine sends a card's pixels only when they change, and before the state that shows them; the page keeps the
  last ones and takes from each state whether they are live. Dropping them when a tooltip went showed nothing for an
  item hovered a second time.
- One thing shows at a time: the item card (or, without one, the rank panel) while it is live, leaving or recalled;
  else the training card while there is one; else the idle view. A player grinding keeps the EXP card in view, an item
  hovered takes its place, and the EXP card is back once that card has gone. While the EXP card shows, a card takes
  over only once two reports in a row have it (`CONFIRM_REPORTS`: the search finds look-alikes in a single frame), and
  it stays 2 s after its tooltip goes instead of 6. The player expects the price within a second of pointing: on the
  demo it shows 0.13-0.23 s after the tooltip is drawn (a 0.4 s wait plus idle-rate looks had made it over a second). (The training card had sat over the others, and the floating
  window grew into a tall stack the player could not leave in a corner.)
- The header says what is shown: the item, the training's map, or "Watching the
  game" / "Not watching the game" (with what to do). No engine numbers there: they are on the page.
- The rank panel is the web's own, in HTML (`rankpanel.js`): classicrank.com's photo of the character (`photo`, a 96 px
  PNG; the page's policy allows images from classicrank.com), the name, level, job and fame, the three ranks as tiles,
  and a visible link to classicrank.com (their terms ask for one). The Windows app's panel sits beside the Character
  Info window, which shows the name and the character; the floating window has neither, and the photo is how the player
  sees the name was read right. The engine says what the panel is about in `rank_about` (`{read, status, rank}`), sent
  with the canvas panel it still draws (the home page's demo shows that one). Pictures and links are used only when
  they are classicrank.com's; nothing in the panel wraps, so its height does not depend on its width. Neither it nor
  the store line is a `<footer>`: the site's sheet spaces those out in the page's document.
- The floating window is one box for the whole session, so it can sit in a corner of the screen: whatever it shows is
  drawn at its own size or smaller to fit both its width and height (never larger, which would blur it), centred, and
  recent items that do not fit are left out; the countdown bar lies over the body rather than resizing it. It opens
  square inside, its side 28% of the screen's height (280-480 px; the game's UI grows with the screen), or at the size
  the player last dragged it to (`maplelens.cards.box`). While it is open the page's panel says the cards are floating, with a button that
  closes it.

What measuring taught (1080p Twitch clips, Chrome, `scripts` not needed: a local server and `?report=`):
- OCR jobs piled up in the page while the OCR box was unticked (400 MB in a minute, 1.5 GB in six): the engine makes
  no job until `set_ocr(true)` and the page keeps the newest four.
- The tooltip search was 53 ms natively on busy frames, two thirds of it summing border lifts pixel by pixel for
  every band row: `find::Lifts` sums them ahead of time (`bench_find`, 26 ms). A tooltip followed from the last look is
  checked in place (`find::tooltip_at`, ~2 ms) and the whole frame searched every `FULL_SEARCH_EVERY` looks.
- The Character Info search (60-200 ms) is spread over four bands of the frame, one every 0.3 s; the page looks every
  quarter second while nothing is followed, every 100 ms while something is (a card, or a tooltip the search sees but
  has not shown yet: `tooltip_in_view`, else an unknown item's five reads took 1.25 s), and turns a frame into a bitmap
  only when it is due (a 60 fps clip made a bitmap of every frame).
- wasm SIMD is on (`.cargo/config.toml`); the engine then takes 20-35 ms a frame at 1080p, p95 under 75 ms, no stalls,
  heap flat over three minutes; the whole browser spends ~30% of a core on a 1080p60 clip (about half of it decoding the
  clip), 20% on a static frame at 10 fps, 2% idle.
- Compressed footage and the two demo items cannot show the browser reading real tooltips: that needs a window
  captured clean. The Windows app's search path on the same clips is the reference (`stream_bench`).

## Lookups and idle cost

- A misread must never name another item: `Catalog::fuzzy` returns nothing when two names fit about equally (HP / MP
  scrolls, Calaf / Calas), numbers in names are exact (the Leaf Points coupons differ by a zero), and a lost gender
  letter ("Red Cloth Pants ()") names neither variant. `wrong_items_under_ocr_noise` measures it on the real catalog:
  74,724 one-letter misreads, 9 name another item (each is another item's exact name or the f/t fold) and 437 name
  nothing.
- The fuzzy match (`catalog::similarity`, used by the app and the browser alike, for titles and shop items) is `ratio`'s
  measure (a lost or extra letter costs 1, a wrong one 2) where a look-alike letter (`CONFUSABLE`: o/0/q/a/c, l/i/1, e/c, f/t,
  s/5, u/v, n/h/r, g/q/9) costs 0.5, and a candidate is dropped as soon as no path can stay under the cost the cutoff allows.
  `wrong_items_under_heavier_noise` (2, 3, 4 random edits per name on the 1,864-item list, edits biased to look-alikes):
  found 74.1/47.3/28.1% before and 81.5/57.9/36.6% after, wrong 0.17/0.31/0.27% before and 0.16/0.42/0.38% after, and a
  lookup 2.7-3.4 ms before and 0.7-1.1 ms after; one edit: 431 -> 300 not found, 9 wrong either way. The margin and the
  `MIN_MATCH_SCORE` cutoff are unchanged: precision first.
- The screen is grabbed by need: a GDI grab of the tooltip area costs 6-9 ms, so the hover loop looks every 50 ms while
  no tooltip is up, the map scan every 200 ms until tooltip colours are near the cursor (`MapScanner::is_active`), and
  the Character Info search once a second (it costs ~80 ms at 1080p, whose scale 0.9 gets a coarser pyramid than 1.0:
  a stride-2 coarse scan was tried and loses windows at odd alignments).
- A tooltip the game clamps to the client's right edge has its right border on the last column or past it: `tooltip::right_edge_unreadable` ignores a width whose strip lies beyond the frame (and needs one that does not), 
  which is how the Cash Shop's translucent tooltip over wood is found (`cash_shop_tooltip_at_right_edge.png`). The browser's 
  frame there is the whole window, so the search had found nothing before.
- The browser's search (`pipeline/hover/elsewhere.rs`) shows a title that names no item only after five identical reads
  and only with the tooltip's bullet: a slide's heading on dark blue read stably as a title for a minute otherwise.

## Map helper data

`maps/client.rs` fetches the whole map list in one request (`map?count=2000`; kept a day in `cache/maps/catalog.json`)
so a tooltip reading resolves locally (`MapService::lookup`), and `maps/prefetch.rs` downloads
`config::MAP_PREFETCH_REGIONS` (Victoria Island) in the background, about 17 minutes the first time. Requests are
retried on 5xx; a map that still lacks a monster or NPC fails instead of being cached incomplete. Some listed maps
404 (about 10%); those are skipped. About 130 maps share a street and name with another and rightly resolve to
nothing.

## Testing

- Pure logic (format, catalog, insights, snapshot, ranking, settings, placement) gets plain unit tests.
- Fixtures are real native-resolution captures whose top-left pixel is the tooltip top-left (`tests/fixtures`).
  **They are not in git** (`.gitignore`): they show players' names. Each crate's `build.rs` sets `cfg(fixtures)` when
  the folder exists, and every test that reads a capture carries
  `#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]`, so CI runs the rest (192 tests,
  133 ignored) and a maintainer with the captures runs all of them. A name a test expects to read off a capture comes
  from `tests/fixtures/names.txt` (`owner=...`, `buyer=...`), never from the source. After restoring the folder, touch
  it (or `crates/*/build.rs`): a moved folder keeps its old timestamp and the build scripts do not rerun. Never write a
  real player's name in code, docs or commit messages; use a placeholder ("ShopOwner").
  `maplelens --debug` saves every new tooltip to `debug\` as a candidate fixture. Never use downscaled
  screenshots: the border detector and the glyph atlas need native pixels. Add fixtures for new tooltip kinds
  instead of loosening thresholds.
- `tests/golden/python.json` holds the retired Python version's answers on every fixture; the Rust tests check
  parity with it (presence, width, pushed-up tops, window search, fuzzy matches, number formats, card sizes).
- Anything time-dependent takes `now` as a parameter.
- Windows-layer tests (`crates/win/src/tests.rs`) create real windows, use the registry and Windows OCR.
- The web engine is tested natively (`crates/web/tests`) on `docs/app/demo-frame.png`.

## Adding a feature (recipe)

New overlay (e.g. stack total, equipment rolls):
1. Add the pure data/logic under `crates/core/src/market` (or a new module) with tests.
2. Add a section in `render/glance.rs` or `render/details.rs` (signature `(canvas, y, item) -> y`) using
   `render/theme.rs` colours, and list it in `render/card.rs`'s groups.
3. Extend `Hover`/the detectors only if new detection is needed; otherwise reuse `Hover.item`.
4. Wire it in `crates/win/src/app.rs`, and in `crates/web` + `docs/app` if it applies to the browser.
5. Rebuild `docs/app/pkg` (`scripts/build-web.ps1`), update the README feature table.

## Gotchas

- DPI awareness must be set before any coordinates are read (`game::make_dpi_aware`, first thing in `main`).
- The game must be the visible top window: capture is of the screen, not the window buffer.
- The overlay asks Windows to keep it out of captures (`WDA_EXCLUDEFROMCAPTURE`), so screenshots never show
  it; `--screenshot` draws the cards in itself.
- Titles are read on the reference-resolution mask (`TEXT_THRESHOLD` 185); at other resolutions the capture is
  resized first, so glyphs differ slightly and some letters may be relearnt.
- `rust-version` is 1.88 (let chains).
- The agent cannot move the user's mouse. Live hover tests need the user, or a fixture screenshot.
