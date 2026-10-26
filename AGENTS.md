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

CI (`.github/workflows/ci.yml`) runs all of these in parallel jobs with a shared build cache; workspace lints in
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
   `PRICES_URL` in `prices.js` and `SNAPSHOT_URL` in `net.rs` point at it; when it is unreachable the page says so and
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
  geometry.rs             Rect, Point, ui_scale (client height / 1200)
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
  market/                 models (Item::from_raw), snapshot, catalog (exact / fuzzy / '?' wildcard),
                          insights, format, time (ISO-8601 without a date crate), sample
  ranking/                classicrank.com model, URL, response parsing, RankCache (pacing is the caller's)
  render/                 canvas (tiny-skia + ab_glyph, Ink text style), theme, font, card, glance, details,
                          origin, range_bar, gamewin, rank_card, status_pill, map_card -> RgbaImage;
                          maple_ui (game-style buttons, check boxes, slider), settings_view (the settings window:
                          picture + hit-testing)
  pipeline/               Screen trait + FrameScreen; HoverDetector (hover/elsewhere.rs: pushed-up tooltips and the
                          no-mouse search), InfoDetector, MapScanner (map.rs)
  mapinfo/                map helper data: models, resolve (OCR title -> map), vote (settles noisy reads), data
                          (maplestory.io JSON and the disk cache format, shared with 0.1.x)
  shop/                   Store Permit watcher: markers (the button pair and the Sold Items close box, by colour, and
                          the UI scale), layout (anchors, offsets, View: reference pixels read off the frame as
                          captured), buttons (whose store, open?), lines (column-ink text compare), soldlist,
                          chatlog, scrollbar, align (the double-count rules), text (a line read by the game's own
                          fonts), parse (text -> events), watcher (ShopWatcher: frame in, events out)
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
  shop.rs, wgc.rs         the shop watcher thread (and `--shop`), fed by Windows Graphics Capture; app/status.rs:
                          the status pill
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
  token is never logged or shown whole. `Telegram` takes a bot token (`<id>:<secret>`, found anywhere in a paste of
  @BotFather's message) and writes to one chat: the player sends the bot /start, and Test asks the bot who wrote last
  (`getUpdates`, POSTed like everything else) and keeps that chat (`telegram_chat`; a new token resets it). Its
  messages are HTML with names escaped and '@' kept from mentioning. Telegram's Bot API allows any origin, so the
  browser sends straight to it too. Settings: `discord_webhook`, `telegram_bot`, `telegram_chat`, `notify_sales`,
  `notify_store`. Refusals are worded by `notify::refusal`, waits read by `notify::retry_after`, for both platforms.
  A new connector is a type and a settings field. `cargo test -p maplelens live_discord -- --ignored` sends to
  `MAPLELENS_DISCORD_WEBHOOK`; `live_telegram` links and writes through the bot in `MAPLELENS_TELEGRAM_BOT`.
- The browser runs the same watcher (`web/src/shopwatch.rs`). Its OCR is asynchronous: the reader answers None while
  text is being read. Only a sale or a visitor whose text is not in (`ShopWatcher::missing_text`) undoes the frame (the
  watcher is cloned first) to try again when the answers are in; text the watcher reads for itself (the owner's name,
  the counter's first value) is asked again next look and counts as a try only once answered. Undoing every frame with
  any pending text held the store "not in view" for as long as Tesseract took to load (seconds) and spent the owner's
  tries on unanswered reads. Lines are read by the game's own fonts
  first (`shop::text`), so the browser asks Tesseract only for letters the fonts have not learnt (a sale of known
  letters needs no OCR at all: `the_shop_is_followed_in_the_browser_and_its_sales_are_read_without_ocr`).
- The status the player sees is core's: `status_pill::text_for` / `tone_for` give the Windows pill's wording ("Open the
  Sold Items window") and its tone, and the browser shows exactly that on the page and in the floating window
  (`cards.js` `setStore`). Tested live against the real game window (Chrome started with
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
read from the status bar (under the job, right of the LV box: `vision::hud::find_bar` + `HUD_NAME`, light ink at low
levels, voted over several reads), falling back to the shop window's name plate respelled from the chat log. There is
no typed-name setting. A store announcement waits up to `OWNER_WAIT_S` for the name.

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
- Enhanced equipment has an orange title and a "(+3)" suffix (`reader::title_gray`, `catalog::without_enhancement`).
  The card shows the unenhanced item's price.

## The browser version

`docs/app` is the page (`app.js`: sources, pacing, stats; `cards.js`: the card view shared by the page and the floating
window; `lens.js`/`worker.js`: the engine in a worker). It measures itself: `window.maplelensStats()` gives frames,
dropped frames, engine time (avg / p50 / p95 / max), long tasks, heap, and `?report=label` posts the same to
`/report` of whatever served the page. Test sources, all through the code a shared window uses:
`?demo`, `?video=/clip.mp4` (a recording played in a loop), `?capture=/clip.mp4` (the clip as a media stream: the
track-processor path), `&scale=0.9` (the UI size by hand: the status bar's outline does not survive compression),
`&fps=`, `?preview=pip` (the floating window's view inside the page). Chrome started with `--js-flags=--expose-gc`
makes the reported heap live data.

The page is deliberately bare: Share game window, the cards (Float over the game), the store log, the Discord webhook and
the Telegram bot (kept in localStorage, shown masked with a Change button). There is no settings panel; Tesseract (tesseract.js 7.0.0 from jsDelivr, loaded when
the engine meets text the atlas cannot read) is always on, for shop text and unknown letters. The floating window opens at a fixed size from the shared game window (`cards.js` `floatingSize`: a fifth of its width, 
300-440 px, and tall enough for the biggest card plus a rank panel); the cards scale to its width and never shrink to its 
height (the body scrolls). The page cannot resize it afterwards (`resizeTo` needs a click in a document picture-in-picture 
window), so there is no auto-height.

What measuring taught (1080p Twitch clips, Chrome, `scripts` not needed: a local server and `?report=`):
- OCR jobs piled up in the page while the OCR box was unticked (400 MB in a minute, 1.5 GB in six): the engine makes
  no job until `set_ocr(true)` and the page keeps the newest four.
- The tooltip search was 53 ms natively on busy frames, two thirds of it summing border lifts pixel by pixel for
  every band row: `find::Lifts` sums them ahead of time (`bench_find`, 26 ms). A tooltip followed from the last look is
  checked in place (`find::tooltip_at`, ~2 ms) and the whole frame searched every `FULL_SEARCH_EVERY` looks.
- The Character Info search (60-200 ms) is spread over four bands of the frame, one every 0.3 s; the page looks every
  quarter second while nothing is followed, every 100 ms while something is, and turns a frame into a bitmap only when
  it is due (a 60 fps clip made a bitmap of every frame).
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
