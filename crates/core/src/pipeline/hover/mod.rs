//! Mouse position -> tooltip measurement -> item title -> market item.

mod elsewhere;

use std::collections::HashMap;
use std::sync::Arc;

use super::{Screen, grab_normalised};
use crate::config;
use crate::geometry::{Point, Rect, UiScale, scaled, ui_scale};
use crate::image::RgbImage;
use crate::market::catalog::{Catalog, clean_title};
use crate::market::models::Item;
use crate::vision::hud::find_bar;
use crate::vision::reader::{TextReader, find_bullet};
use crate::vision::tooltip::{measure_width, tooltip_present};

/// Titles whose measured tooltip width is remembered.
const WIDTH_MEMORY: usize = 256;

/// An item tooltip is showing under the mouse.
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    /// The matched item's name (or the cleaned read title when the item is unknown).
    pub title: String,
    pub item: Option<Arc<Item>>,
    /// Screen pixels.
    pub tooltip_size: (i32, i32),
    /// Equipment compare: a second tooltip (the worn item) sits to the right of the first.
    pub compare: bool,
    /// Game client area in screen coordinates.
    pub bounds: Rect,
    /// UI size relative to the measured resolution.
    pub scale: f32,
    /// Screen px; set when the game slid the tooltip up to fit on screen.
    pub pushed_up_height: Option<i32>,
    /// Where the tooltip was found, when it had to be searched for (no mouse position).
    pub found_at: Option<Point>,
    /// A player-crafted item: the tooltip has an extra crafter-name line on top.
    pub crafted: bool,
}

/// Where a tooltip is, beyond the usual mouse + offset.
#[derive(Clone, Copy, Debug, Default)]
struct Placement {
    pushed_up_height: Option<i32>,
    found_at: Option<Point>,
    crafted: bool,
}

/// Width and height of the tooltip for `item`. The width is measured on screen when that was possible (it is
/// what placement depends on and it does not need the item to be known); the height comes from the per-kind
/// table in config.
pub fn tooltip_size(item: Option<&Item>, measured_width: Option<i32>) -> (i32, i32) {
    let narrow = config::TOOLTIP_WIDTHS[0];
    let equipment = match item {
        Some(item) => item.is_equip(),
        None => measured_width == Some(narrow),
    };
    let kind = if equipment { Some("Equip") } else { item.and_then(|i| i.category.as_deref()) };
    let (width, height) = config::tooltip_size_for(kind);
    (measured_width.unwrap_or(width), height)
}

/// Item names are short phrases; UI hint tooltips are sentences ("Double-click to ...") or read noise. A colon
/// is no sign of a world-map tooltip: 200+ scrolls are named "...Scroll: Greater".
pub fn looks_like_title(text: &str) -> bool {
    let text = text.trim();
    let count = text.chars().count();
    // A '?' is a letter we could not read: such a title is not shown as an unknown item either.
    if count < 3
        || !text.chars().next().is_some_and(char::is_uppercase)
        || text.ends_with('.')
        || text.contains('?')
        || text.to_lowercase().contains("click")
    {
        return false;
    }
    let good = text.chars().filter(|c| c.is_alphanumeric() || " '-:".contains(*c)).count();
    good as f64 >= 0.9 * count as f64
}

pub struct HoverDetector {
    catalog: Arc<Catalog>,
    reader: TextReader,
    ui: Arc<UiScale>,
    bar_checked_at: f64,
    last_title: Option<String>,
    last_item: Option<Arc<Item>>,
    last_crafted: bool,
    /// The crafted-row and two-line reads behind `last_item`, when the one-line title read alone did not decide it:
    /// two items crafted by one player share their first line (the crafter's name).
    last_reads: Option<(String, String)>,
    compare_seen_at: f64,
    widths: HashMap<String, i32>,
    unknown_title: Option<String>,
    unknown_streak: u32,
    /// Where the tooltip was found last (no-mouse search) and how many looks since the whole frame was searched.
    last_spot: Option<Point>,
    looks_since_search: u32,
    /// When set, the capture of every newly recognised tooltip is kept here (debug aid: future test fixtures).
    pub captures: Option<Vec<(String, RgbImage)>>,
}

impl HoverDetector {
    pub fn new(catalog: Arc<Catalog>, reader: TextReader) -> Self {
        Self {
            catalog,
            reader,
            ui: Arc::default(),
            bar_checked_at: f64::NEG_INFINITY,
            last_title: None,
            last_item: None,
            last_crafted: false,
            last_reads: None,
            compare_seen_at: f64::NEG_INFINITY,
            widths: HashMap::new(),
            unknown_title: None,
            unknown_streak: 0,
            last_spot: None,
            looks_since_search: 0,
            captures: None,
        }
    }

    pub fn set_catalog(&mut self, catalog: Arc<Catalog>) {
        self.catalog = catalog;
        self.last_title = None;
        self.last_item = None;
        self.last_crafted = false;
        self.last_reads = None;
    }

    /// Measure into a UI scale shared with others (the `InfoDetector` reads the same one).
    pub fn with_ui_scale(mut self, ui: Arc<UiScale>) -> Self {
        self.ui = ui;
        self
    }

    /// The UI scale this detector measures off the status bar; share it with the `InfoDetector`.
    pub fn ui_scale(&self) -> Arc<UiScale> {
        self.ui.clone()
    }

    pub fn catalog(&self) -> &Arc<Catalog> {
        &self.catalog
    }

    pub fn reader_mut(&mut self) -> &mut TextReader {
        &mut self.reader
    }

    /// One detection pass while the game is in front: the tooltip under the mouse, or None.
    pub fn step(&mut self, screen: &mut dyn Screen, bounds: Rect, now: f64) -> Option<Hover> {
        self.measure_ui(screen, bounds, now);
        let scale = self.ui.of(bounds);
        self.step_at_scale(screen, bounds, scale, now)
    }

    /// Re-measure the UI scale off the status bar now and then (a grab of the bottom of the client).
    fn measure_ui(&mut self, screen: &mut dyn Screen, bounds: Rect, now: f64) {
        if now - self.bar_checked_at < config::STATUS_BAR_CHECK_S {
            return;
        }
        self.bar_checked_at = now;
        let height = bounds.h.min(scaled(config::STATUS_BAR_STRIP_HEIGHT, ui_scale(bounds)));
        let strip = screen.grab(Rect::new(bounds.x, bounds.bottom() - height, bounds.w, height));
        // Hidden (Cash Shop, a cut scene) or covered by a menu: no reading (the settled scale stays).
        if let Some(bar) = find_bar(&strip) {
            self.ui.set(bounds, bar.width as f32 / config::STATUS_BAR_WIDTH as f32, now);
        }
    }

    /// `step` with the game's UI scale given rather than derived from the client height.
    pub fn step_at_scale(&mut self, screen: &mut dyn Screen, bounds: Rect, scale: f32, now: f64) -> Option<Hover> {
        let Some(cursor) = screen.cursor() else { return self.search(screen, bounds, scale, now) };
        let Some(area) = tooltip_area(cursor, bounds, scale) else {
            // No room below the mouse for even the top of a tooltip: it can only have been pushed up.
            return self.clamped(screen, cursor, bounds, scale, now);
        };
        let image = grab_normalised(screen, area, scale);
        self.reader.allow_ocr(tooltip_evidence(&image));
        let title = self.reader.read_title(&image, title_width(&image), 1);
        let same_title = self.last_title.as_deref() == Some(title.as_str());
        let unchanged = same_title
            && match self.last_reads.clone() {
                None => true,
                Some(reads) => reads == self.other_reads(&image),
            };
        if !unchanged {
            self.compare_seen_at = f64::NEG_INFINITY; // a new item: forget the old item's compare tooltip
            (self.last_item, self.last_crafted) = self.lookup(&image, &title);
            // The one-line read decided unless the match (or no match) came from the other reads.
            let decided_by_title = self.last_item.is_some() && !self.last_crafted && self.match_title_only(&title);
            self.last_reads = (!decided_by_title).then(|| self.other_reads(&image));
            self.last_title = Some(title.clone());
            if let Some(item) = self.last_item.clone() {
                self.keep_capture(&image, &item.name);
            }
        }
        // A recognised item name at the spot where tooltips appear is the best proof a tooltip is there: the
        // blue fill is unreliable because it changes with whatever is behind the translucent box. The colour
        // check only has to carry unknown items. Other blue-ish UI (friend list, menus) passes the colour check
        // and has text on it, so an unknown title must also have the tooltip border at one of the game's widths.
        if self.last_item.is_none()
            && let Some(hover) = self.clamped(screen, cursor, bounds, scale, now)
        {
            return Some(hover);
        }
        if self.last_item.is_none() {
            self.unknown_streak =
                if self.unknown_title.as_deref() == Some(title.as_str()) { self.unknown_streak + 1 } else { 1 };
            self.unknown_title = Some(title.clone());
        } else {
            self.unknown_title = None;
            self.unknown_streak = 0;
        }
        // The game's other tooltips ("Double-click to send whispers" in the friend list) look just like item
        // tooltips; their text is short, noisy and different every frame, an item title is stable.
        if self.last_item.is_none()
            && !(tooltip_present(&image)
                && measure_width(&image, &config::TOOLTIP_WIDTHS).is_some()
                && self.unknown_streak >= config::UNKNOWN_TITLE_FRAMES
                && looks_like_title(&title))
        {
            return None;
        }
        let (item, crafted) = (self.last_item.clone(), self.last_crafted);
        Some(self.hover(&image, &title, item, bounds, scale, now, Placement { crafted, ..Placement::default() }))
    }

    #[allow(clippy::too_many_arguments)]
    fn hover(
        &mut self,
        image: &RgbImage,
        title: &str,
        item: Option<Arc<Item>>,
        bounds: Rect,
        scale: f32,
        now: f64,
        at: Placement,
    ) -> Hover {
        let width = self.tooltip_width(image, title);
        let size = tooltip_size(item.as_deref(), width);
        let extra = if at.crafted { config::TOOLTIP_CRAFTED_SHIFT } else { 0 };
        let height = at.pushed_up_height.unwrap_or_else(|| scaled(size.1 + extra, scale));
        // Show the matched name, not the raw read: its per-frame variants must not repaint the card.
        let shown = item.as_ref().map_or_else(|| clean_title(title), |i| i.name.clone());
        let equipment = item.as_ref().map_or(width == Some(config::TOOLTIP_WIDTHS[0]), |i| i.is_equip());
        let compare = self.has_compare(image, equipment, now);
        Hover {
            title: shown,
            item,
            tooltip_size: (scaled(size.0, scale), height),
            compare,
            bounds,
            scale,
            pushed_up_height: at.pushed_up_height,
            found_at: at.found_at,
            crafted: at.crafted,
        }
    }

    /// The item named by `title` (the one-line read of the tooltip in `image`), and whether it is player-crafted.
    fn lookup(&mut self, image: &RgbImage, title: &str) -> (Option<Arc<Item>>, bool) {
        let width = title_width(image);
        let shift = config::TOOLTIP_CRAFTED_SHIFT as usize;
        // Crafted before wrapped: the wrapped read's two rows reach down into a crafted item's title and would
        // match it without knowing the tooltip is taller.
        if let Some(item) = self.match_read(image, title) {
            return (Some(item), false);
        }
        let crafted = self.reader.read_title_at(image, width, 1, shift);
        if let Some(item) = self.match_read(image, &crafted) {
            return (Some(item), true);
        }
        // Long names wrap onto a second title line the one-line read cuts off.
        let wrapped = self.reader.read_title(image, width, 2);
        if let Some(item) = self.match_read(image, &wrapped) {
            return (Some(item), false);
        }
        // OCR filled in the atlas's unknown glyphs and lost letters doing it: the atlas's read, gaps and all.
        if let Some(own) = self.reader.atlas_title(image, width)
            && own != title
            && let Some(item) = self.match_read(image, &own)
        {
            return (Some(item), false);
        }
        // A clean atlas read that names nothing: ask OCR for a second opinion ('Tree Brancla').
        if !title.is_empty()
            && !title.contains('?')
            && let Some(second) = self.reader.ocr_title(image, width)
        {
            return (self.match_read(image, &second), false);
        }
        (None, false)
    }

    /// The crafted-row and two-line reads of `image` (cached by mask in the reader, so cheap to repeat).
    fn other_reads(&mut self, image: &RgbImage) -> (String, String) {
        let width = title_width(image);
        let crafted = self.reader.read_title_at(image, width, 1, config::TOOLTIP_CRAFTED_SHIFT as usize);
        (crafted, self.reader.read_title(image, width, 2))
    }

    /// Whether the one-line read names the item by itself.
    fn match_title_only(&self, title: &str) -> bool {
        title.chars().count() <= config::MAX_TITLE_CHARS && self.catalog.lookup(title).is_some()
    }

    /// The catalog item a read names, when the capture is really a tooltip; a match teaches the atlas the letters
    /// it read as '?'.
    fn match_read(&mut self, image: &RgbImage, read: &str) -> Option<Arc<Item>> {
        // Longer than a title = misread (title + description), not worth matching.
        let item = (read.chars().count() <= config::MAX_TITLE_CHARS).then(|| self.catalog.lookup(read)).flatten()?;
        if !self.credible(image, read) {
            return None;
        }
        self.reader.confirm(read, &item.name);
        Some(item)
    }

    /// Is `image` an item tooltip, as far as `read` naming an item can be trusted? With the pointer over a shop list,
    /// a quest log or a dialogue, OCR reads their text and that text often names an item too: measured on 792 real
    /// captures, 1 card in 18 was then for the wrong item. A tooltip shows its title bullet (at the usual row, or a
    /// crafted item's), or its blue fill; or the atlas read the title by itself, glyph by glyph in the title font.
    fn credible(&self, image: &RgbImage, read: &str) -> bool {
        tooltip_evidence(image) || self.reader.read_by_atlas(read)
    }

    /// Measured width, remembered per title so one unsure frame keeps the last good answer.
    fn tooltip_width(&mut self, image: &RgbImage, title: &str) -> Option<i32> {
        if let Some(width) = measure_width(image, &config::TOOLTIP_WIDTHS) {
            if self.widths.len() > WIDTH_MEMORY {
                self.widths.clear();
            }
            self.widths.insert(title.to_string(), width);
        }
        self.widths.get(title).copied()
    }

    /// Is a second tooltip (the worn item) showing next to an equipment tooltip?
    ///
    /// Only a title that matches a known item counts: in a shop the strip next to the tooltip holds arbitrary
    /// UI text, and treating that as a compare tooltip flipped the card between sides. A single misread is
    /// bridged for a moment so a real compare tooltip does not flicker either.
    fn has_compare(&mut self, image: &RgbImage, equipment: bool, now: f64) -> bool {
        if equipment {
            let second = image.crop(config::TOOLTIP_COMPARE_OFFSET_X as usize, 0, image.width, image.height);
            // The worn item may be crafted too.
            for top in [0, config::TOOLTIP_CRAFTED_SHIFT as usize] {
                let title = self.reader.read_title_at(&second, second.width, 1, top);
                if self.catalog.lookup(&title).is_some() {
                    self.compare_seen_at = now;
                    break;
                }
            }
        }
        now - self.compare_seen_at < config::COMPARE_GRACE_S
    }

    fn keep_capture(&mut self, image: &RgbImage, name: &str) {
        if let Some(captures) = &mut self.captures {
            captures.push((name.to_string(), image.clone()));
        }
    }
}

/// A wide tooltip has no compare tooltip beside it (only equipment, the narrow kind, does), so its title is read
/// across the whole tooltip: a long scroll name runs past the narrow width, and its cut read had named another grade
/// of the scroll.
const WIDE_TITLE_MARGIN: i32 = 8;

/// How much of `image` (a tooltip's top, at UI scale 1) its title may span.
pub(crate) fn title_width(image: &RgbImage) -> usize {
    let wide = config::TOOLTIP_WIDTHS[1];
    match measure_width(image, &config::TOOLTIP_WIDTHS) {
        Some(w) if w == wide && image.width as i32 >= wide => (wide - WIDE_TITLE_MARGIN) as usize,
        _ => config::TOOLTIP_TITLE_WIDTH as usize,
    }
}

/// Does `image` show a tooltip by its look: a title bullet (at the usual row or a crafted item's) or the blue fill?
/// Without it only an atlas read can name an item (`HoverDetector::credible`), so OCR is not asked: over a busy
/// scene, reading every frame's scenery with OCR cost a fifth of a CPU core for answers that could never count.
fn tooltip_evidence(image: &RgbImage) -> bool {
    find_bullet(image, 0).is_some()
        || find_bullet(image, config::TOOLTIP_CRAFTED_SHIFT as usize).is_some()
        || tooltip_present(image)
}

/// Where the top of the tooltip would be if one is showing, or None if that is outside the game.
pub fn tooltip_area(cursor: Point, bounds: Rect, scale: f32) -> Option<Rect> {
    let x = cursor.x + scaled(config::TOOLTIP_OFFSET.0, scale);
    let y = cursor.y + scaled(config::TOOLTIP_OFFSET.1, scale);
    let h = scaled(config::TOOLTIP_CAPTURE_SIZE.1, scale);
    // Near the right edge there is no room for a compare tooltip anyway.
    let w = scaled(config::TOOLTIP_CAPTURE_SIZE.0, scale).min(bounds.right() - x);
    let inside =
        bounds.x <= x && bounds.y <= y && w >= scaled(config::TOOLTIP_TITLE_WIDTH, scale) && y + h <= bounds.bottom();
    inside.then(|| Rect::new(x, y, w, h))
}
