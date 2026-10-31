//! Tunable constants. Visual styling lives in `render::theme`, not here.

/// The game's executable, matched case-insensitively against the process image name.
pub const GAME_PROCESS: &str = "maplestory.exe";
/// Hourly updates keep prices current without polling the source aggressively.
pub const SNAPSHOT_REFRESH_S: u64 = 3600;
/// Bound network delays and back off after transient failures.
pub const SNAPSHOT_TIMEOUT_S: u64 = 30;
pub const SNAPSHOT_RETRY_S: u64 = 60;

/// The game draws an item tooltip's fill with its top-left corner at the mouse + this offset
/// (measured by hovering items of every inventory tab: always +1,+32 give or take 2 px).
pub const TOOLTIP_OFFSET: (i32, i32) = (1, 32);
/// Equipment tooltips can have a second one (the item you wear) to their right: its left edge is this far
/// from the first one's. Measured on a real compare screenshot (title reads fine anywhere in 368-380).
pub const TOOLTIP_COMPARE_OFFSET_X: i32 = 373;
/// A misread of the compare tooltip title is bridged for this long.
pub const COMPARE_GRACE_S: f64 = 0.4;
/// Equipment is the narrowest tooltip (370 px) and titles are read from this much of the left side, so
/// the compare tooltip starting 373 px in never leaks into the first title.
pub const TOOLTIP_TITLE_WIDTH: i32 = 370;
/// Only the top of the tooltips is captured: both titles, the presence check and the border rows that tell
/// the width.
pub const TOOLTIP_CAPTURE_SIZE: (i32, i32) = (TOOLTIP_COMPARE_OFFSET_X + TOOLTIP_TITLE_WIDTH, 120);

/// Tooltip size (width, height) per kind of item, measured by hovering items of every tab.
/// Heights vary a little inside a tab (equipment 323-398, use 178-277) with the description length;
/// tune these if the card overlaps or floats away from the tooltip. Unknown items use the default.
pub fn tooltip_size_for(kind: Option<&str>) -> (i32, i32) {
    match kind {
        Some("Equip") => (370, 324),
        Some("Stars") => (450, 209),
        Some("Cash Shop") => (450, 277),
        _ => TOOLTIP_DEFAULT_SIZE, // Scrolls, Use and Etc share the default shape
    }
}
pub const TOOLTIP_DEFAULT_SIZE: (i32, i32) = (450, 178);
/// The game slides a tooltip up when it would run off the bottom of the client area. This is taller than any
/// tooltip measured (an equipment-shaped cash ring with a long description: ~490), and bounds how far up to look.
pub const TOOLTIP_MAX_HEIGHT: i32 = 560;
/// The only widths the game draws item tooltips at: equipment is narrow, everything else wide.
pub const TOOLTIP_WIDTHS: [i32; 2] = [370, 450];

/// The status bar at the bottom of the game is the ruler for the UI size (`vision::hud`): this wide at the measured
/// resolution, outline to outline. Measured at 1920x1200: 1250 x 106 normally, 800 x 68 in "Ideal Ratio" (0.64).
pub const STATUS_BAR_WIDTH: i32 = 1250;
/// Only this much of the bottom of the client is grabbed to find the bar (at the measured resolution; the bar is
/// ~110).
pub const STATUS_BAR_STRIP_HEIGHT: i32 = 200;
/// The UI size only changes when the window is resized or the player changes "UI Size": once a second is plenty.
pub const STATUS_BAR_CHECK_S: f64 = 1.0;

/// The UI scale (off the status bar, or the shop's buttons) is decided on the readings of this many seconds, not on
/// one frame: one frame with a menu over the bar's outline or the pointer over a button measures it wrong, and every
/// offset and the shop's lists moved with it. A real change (a resize, "UI Size", full screen) still takes over in
/// about half of it, since the new readings are then most of the window.
pub const SCALE_VOTE_WINDOW_S: f64 = 5.0;
/// Readings this close (relative) are the same scale: the bar's width is good to a pixel in ~1000, the shop's
/// button to ~1%, and the UI sizes the game draws differ by 6% or more (1.2 and 1.28 at 1440p).
pub const SCALE_VOTE_TOLERANCE: f32 = 0.01;
/// Readings needed before a first decision (until then the height-derived scale stands in), and in any new majority.
pub const SCALE_VOTE_MIN_READINGS: usize = 3;

/// Items crafted by a player carry the crafter's name ("bTang's") on a line above the title, pushing the whole
/// tooltip down by this much (measured on a real capture; the title reads anywhere in 30-42).
pub const TOOLTIP_CRAFTED_SHIFT: i32 = 36;

/// A screen grab alone takes 6-9 ms (GDI floor, measured), and the loop used to sleep 1 ms after it: one core and
/// the compositor busy for as long as the game was in front. While a tooltip is up the loop re-reads it this often,
/// so a change of item or the tooltip going away shows within a few frames...
pub const POLL_S: f64 = 0.02;
/// ...and while none is, the loop looks this often: the game draws a tooltip only after the mouse has rested for
/// a moment, so one more frame of delay is not noticed, and the idle cost drops to a seventh.
pub const HOVER_IDLE_POLL_S: f64 = 0.05;
/// Game not in front: nothing to do, check again soon.
pub const POLL_BACKGROUND_S: f64 = 0.1;
pub const GAME_LOST_RETRY_S: f64 = 1.0;

/// Reads longer than this are a mis-read (e.g. title + description), not an item title.
pub const MAX_TITLE_CHARS: usize = 40;

/// An unknown title must read identically this many frames in a row before a "no price data" card shows: the
/// game's hint tooltips (friend list) read differently every frame, an item name does not.
pub const UNKNOWN_TITLE_FRAMES: u32 = 5;
/// Minimum fuzzy-match score (0-100) to accept a read name as a known item.
pub const MIN_MATCH_SCORE: f64 = 88.0;
/// The closest name must beat the next one by this much (ratio points): a tie names nothing.
pub const FUZZY_MARGIN: f64 = 0.5;
/// Minimum score of the part of a fuzzy match's name that tells it from its variants ("Intermediate" in "Gloves Magic
/// Attack Scroll: Intermediate"): a read cut short there ("...Scroll: Inter") scored best against the shorter
/// "Greater" and showed its price.
pub const VARIANT_MIN_SCORE: f64 = 80.0;

/// Gap between the tooltip and the card.
pub const CARD_GAP_PX: i32 = 6;
/// A single missed frame (mouse moving faster than the tooltip, a read hiccup) must not flash the card off.
pub const HIDE_GRACE_S: f64 = 0.15;
/// > 120 Hz polling so a 60 fps display never waits on us.
pub const UI_TICK_S: f64 = 0.006;

// --- Rank card beside the Character Info window (classicrank.com) ---
/// Offsets are from the window's top-left, measured on a native screenshot of the window.
/// Where the "CHARACTER INFO" title text starts.
pub const INFO_TITLE_OFFSET: (i32, i32) = (9, 9);
/// Inside the grey name plate under the portrait (x, y, w, h).
pub const INFO_NAME_BOX: (i32, i32, i32, i32) = (21, 200, 160, 20);
pub const INFO_WINDOW_SIZE: (i32, i32) = (493, 280);
/// Searching the whole game for the window is the expensive part (about 40 ms), so it only runs while the window
/// is not known (the grab of the whole client and the search cost 100 ms at 1080p), once a second: a window just
/// opened gets its panel within a second. Once found it is
/// re-checked in place, which is a tiny grab.
pub const INFO_SEARCH_S: f64 = 1.0;
pub const INFO_POLL_S: f64 = 0.05;
pub const INFO_HIDE_GRACE_S: f64 = 0.3;
pub const INFO_CARD_GAP_PX: i32 = 4;
/// Rank lookups: the API caches for 30 s and allows 30 requests/minute per IP without a key.
pub const RANK_TTL_S: f64 = 90.0;
/// Unknown names (often a misread) are not worth hammering.
pub const RANK_MISSING_TTL_S: f64 = 300.0;
pub const RANK_REQUEST_GAP_S: f64 = 2.0;

// --- Map helper: hover a location on the world map (maplestory.io data and sprites) ---
/// v40 is the classic-era data set (the Hill East of Henesys: 49 spawns).
pub const MAP_API: &str = "https://maplestory.io/api/GMS/40";
/// Maps the classic-era data lacks are looked up in MapleStory Classic World's own (`mapinfo::CLASSIC_ID_BASE`).
pub const CLASSIC_MAP_API: &str = ITEM_API;
/// maplestory.io is sometimes slow (a request took >8 s).
pub const MAP_REQUEST_TIMEOUT_S: u64 = 10;
/// Sold items' pictures: maplestory.io's MapleStory Classic World data (region MCW), which numbers and names items as
/// the game does. Of the price list's 1,867 items 1,866 are there by their own id (GMS v40, the map helper's data,
/// names 465 of them otherwise: Classic's Iron Ingot is its Steel Plate).
pub const ITEM_API: &str = "https://maplestory.io/api/MCW/1";
/// After a failed lookup.
pub const MAP_RETRY_S: f64 = 30.0;
/// World-map tooltips are not at a fixed offset from the cursor (two samples differ by ~20 px vertically), so the
/// tooltip is looked for inside this window right of / below the cursor (reference px; origin is the cursor).
pub const MAP_SCAN_OFFSET: (i32, i32) = (8, 8);
pub const MAP_SCAN_SIZE: (i32, i32) = (560, 190);
/// Smaller blue boxes are not a tooltip.
pub const MAP_TOOLTIP_MIN_SIZE: (i32, i32) = (120, 20);
/// OCR of the scan window is the cost, not the grab; it is cached while the picture does not change.
pub const MAP_POLL_S: f64 = 0.06;
/// While nothing near the cursor looks like a tooltip (no tooltip colours for `MAP_ACTIVE_S`), the scan window is
/// grabbed this often: the grab is 5-8 ms each time, and the world map is open a small part of the session.
pub const MAP_IDLE_POLL_S: f64 = 0.2;
pub const MAP_ACTIVE_S: f64 = 1.5;
/// After an OCR scan found no map tooltip (or one naming no map), the next waits this long. Name tags, chat and
/// quest windows pass the colour pre-check, and the town never repeats a frame for the cache: OCR of the scan
/// window every poll took several CPU cores and made the game lag. Over a real tooltip scans stay every poll.
pub const MAP_IDLE_OCR_S: f64 = 0.5;
/// OCR scans are slower and noisier than item reads: bridge a few missed ones.
pub const MAP_HIDE_GRACE_S: f64 = 0.8;
/// The map list is re-fetched when older than this (new maps are rare).
pub const MAP_CATALOG_MAX_AGE_S: f64 = 24.0 * 3600.0;
/// Regions (map id / 10,000,000) downloaded in the background so a hover is answered from disk: 10 is Victoria
/// Island (about 350 maps). Each map costs a few requests, shared monsters are fetched once.
pub const MAP_PREFETCH_REGIONS: [u32; 1] = [10];
/// Pause between prefetched maps, so the background download never competes with what the player is looking at.
pub const MAP_PREFETCH_PAUSE_S: f64 = 0.25;
/// The browser's search looks over the whole frame at least this often while a tooltip is being followed.
pub const FULL_SEARCH_EVERY: u32 = 6;
/// A lookup nobody asked about for this long is dropped from the queue.
pub const MAP_STALE_S: f64 = 2.0;
/// No tooltip read for this long: the next one is a new hover, start voting afresh.
pub const MAP_FORGET_S: f64 = 0.6;
/// A reading that matched no map is ignored this long (OCR then tries other readings).
pub const MAP_REJECT_S: f64 = 10.0;
