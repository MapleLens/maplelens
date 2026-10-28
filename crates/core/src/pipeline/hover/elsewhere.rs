//! Tooltips that are not at mouse + offset: pushed up by the game's bottom clamp or left by its right-edge clamp, or
//! searched for when there is no mouse position (the browser).

use super::{Hover, HoverDetector, Placement, looks_like_title};
use crate::config;
use crate::geometry::{Point, Rect, scaled};
use crate::pipeline::{Screen, normalised};
use crate::vision::find;
use crate::vision::locate::candidate_tops;
use crate::vision::reader::find_bullet;
use crate::vision::tooltip::measure_width;

/// The title's bullet, at a plain or a crafted item's title row.
pub(super) fn has_bullet(image: &crate::image::RgbImage) -> bool {
    find_bullet(image, 0).is_some() || find_bullet(image, config::TOOLTIP_CRAFTED_SHIFT as usize).is_some()
}

/// Is a tooltip's title bullet left of `spot` (a point of the screen) on its title row? Then a tooltip read there
/// without a bullet of its own is that one's inside: the game slid it left of the usual spot.
pub(super) fn bullet_left_on_screen(screen: &mut dyn Screen, spot: Point, bounds: Rect, scale: f32) -> bool {
    let margin = scaled(BULLET_MARGIN, scale);
    let x0 = (spot.x - scaled(config::TOOLTIP_WIDTHS[1], scale) - margin).max(bounds.x);
    let y0 = (spot.y - margin).max(bounds.y);
    let y1 = (spot.y + scaled(BULLET_ROWS, scale)).min(bounds.bottom());
    if spot.x <= x0 || y1 <= y0 {
        return false;
    }
    let area = screen.grab(Rect::new(x0, y0, spot.x - x0, y1 - y0));
    bullet_left_of(&area, Point { x: spot.x - x0, y: spot.y - y0 }, scale)
}

/// Is there a title bullet left of `spot` on its title row (a plain or a crafted item's), as far as the widest
/// tooltip reaches?
fn bullet_left_of(frame: &crate::image::RgbImage, spot: Point, scale: f32) -> bool {
    // Room above and left for the corners `bullet_spots` gives (a bullet's corner is up and left of the dot).
    let margin = scaled(BULLET_MARGIN, scale);
    let x0 = (spot.x - scaled(config::TOOLTIP_WIDTHS[1], scale) - margin).max(0);
    let y0 = (spot.y - margin).max(0);
    let y1 = (spot.y + scaled(BULLET_ROWS, scale)).min(frame.height as i32);
    if spot.x <= x0 || y1 <= y0 {
        return false;
    }
    let area = frame.crop(x0 as usize, y0 as usize, spot.x as usize, y1 as usize);
    // The dot's row: from the spot's top to a crafted item's title row.
    let rows = (spot.y - y0)..(y1 - y0);
    find::bullet_spots(&area, scale).iter().any(|corner| rows.contains(&(corner.y + scaled(find::BULLET_AT.1, scale))))
}

/// Two corners further apart than this (UI scale 1) are two tooltips' (one found a pixel or two off is the same).
const INSIDE_MARGIN: i32 = 8;

/// Margin for `bullet_left_of`, and how far below a corner a crafted item's bullet still lies (UI scale 1).
const BULLET_MARGIN: i32 = 60;
const BULLET_ROWS: i32 = 75;

/// The pointer's glove from its hot spot (UI scale 1, measured on rendered scenes), and the dark fill it is painted over
/// with.
const POINTER: (i32, i32) = (30, 32);
/// The glove reaches this far left of the hot spot.
const POINTER_LEFT: i32 = 6;
const POINTER_FILL: [u8; 3] = [40, 40, 90];

/// Bullet corners of one look that are read loosely (by the atlas and OCR) when the font cannot read them.
const MAX_LOOSE_READS: usize = 6;

/// A pushed-up tooltip's bullet stands at most this far (UI scale 1) from where the tooltip's left edge was expected.
const BULLET_COLUMN: i32 = 4;

impl HoverDetector {
    /// Look for a known item's tooltip the game slid up to fit on screen.
    ///
    /// The game keeps a tooltip inside the client area, so one taller than the room under the mouse has its
    /// bottom on the client's bottom edge and its top somewhere above the usual spot. Only a recognised item
    /// counts here: stray UI text must never be mistaken for a tooltip we had to go looking for.
    ///
    /// Near the right edge the game also slides a tooltip left until its right border is the client's: then it is
    /// looked for at each of the game's widths from that edge, in place and pushed up.
    pub(super) fn clamped(
        &mut self,
        screen: &mut dyn Screen,
        cursor: Point,
        bounds: Rect,
        scale: f32,
        now: f64,
    ) -> Option<Hover> {
        let usual_x = cursor.x + scaled(config::TOOLTIP_OFFSET.0, scale);
        if let Some(hover) = self.pushed_up(screen, usual_x, cursor, bounds, scale, now) {
            return Some(hover);
        }
        // Widest first: read from a narrow tooltip's spot, a wide one's title loses its first word and can still spell
        // another item; read from a wide one's, a narrow tooltip's title starts in the scenery and spells nothing.
        for width in config::TOOLTIP_WIDTHS.into_iter().rev() {
            let x = bounds.right() - scaled(width, scale);
            if usual_x + scaled(width, scale) <= bounds.right() || x < bounds.x {
                continue; // a tooltip this wide fits where it opens: not slid
            }
            if let Some(hover) = self.pushed_left(screen, x, width, cursor, bounds, scale, now) {
                return Some(hover);
            }
            if let Some(hover) = self.pushed_up(screen, x, cursor, bounds, scale, now) {
                return Some(hover);
            }
        }
        None
    }

    /// A known item's tooltip at the usual height but slid left to `x` by the game's right-edge clamp.
    #[allow(clippy::too_many_arguments)]
    fn pushed_left(
        &mut self,
        screen: &mut dyn Screen,
        x: i32,
        width: i32,
        cursor: Point,
        bounds: Rect,
        scale: f32,
        now: f64,
    ) -> Option<Hover> {
        let y = cursor.y + scaled(config::TOOLTIP_OFFSET.1, scale);
        let (w, h) = (
            scaled(config::TOOLTIP_CAPTURE_SIZE.0, scale).min(bounds.right() - x),
            scaled(config::TOOLTIP_CAPTURE_SIZE.1, scale),
        );
        if y < bounds.y || y + h > bounds.bottom() {
            return None;
        }
        let native = screen.grab(Rect::new(x, y, w, h));
        let image = normalised(&native, scale);
        if !super::tooltip_evidence(&image) {
            return None; // nothing tooltip-like there: no read, so an idle pointer by the edge costs one grab
        }
        // Only a tooltip of the width assumed ends at the edge from `x`: a wider one starts further left, and read
        // from `x` its title loses its first word ("Crossbow Attack Scroll" read as the Bow one). (Its right border
        // on the client's last column often cannot be measured at all: that is no evidence against it.)
        if measure_width(&image, &config::TOOLTIP_WIDTHS).is_some_and(|w| w != width) {
            return None;
        }
        // The title starts with its bullet where the tooltip was assumed to start: from a wider tooltip's spot the
        // scenery before the title reads as a word of it ("Old Steel Nordic Helm" for "Steel Nordic Helm").
        if !has_bullet(&image) {
            return None;
        }
        match self.font_lookup(&native, &image, scale) {
            Some((read, Some(item), crafted)) => {
                let at = Placement { crafted, ..Placement::default() };
                return Some(self.hover(&image, &read, Some(item), bounds, scale, now, at));
            }
            Some((_, None, _)) => return None, // read in full, and no item of the catalog
            None => {}
        }
        self.reader.allow_ocr(true);
        let title = self.reader.read_title(&image, super::title_width(&image), 1);
        let (item, crafted) = self.lookup(&image, &title);
        let item = item.or_else(|| self.graded(screen, Point { x, y }, bounds, scale, &title))?;
        let at = Placement { crafted, ..Placement::default() };
        Some(self.hover(&image, &title, Some(item), bounds, scale, now, at))
    }

    /// A known item's tooltip with its left edge at `x`, slid up from the usual spot by the bottom clamp.
    fn pushed_up(
        &mut self,
        screen: &mut dyn Screen,
        x: i32,
        cursor: Point,
        bounds: Rect,
        scale: f32,
        now: f64,
    ) -> Option<Hover> {
        let usual_top = cursor.y + scaled(config::TOOLTIP_OFFSET.1, scale);
        if usual_top + scaled(config::TOOLTIP_MAX_HEIGHT, scale) <= bounds.bottom() || x < bounds.x {
            return None; // even the tallest tooltip fits below the mouse: nothing was pushed
        }
        let w = scaled(config::TOOLTIP_CAPTURE_SIZE.0, scale).min(bounds.right() - x);
        let top = bounds.y.max(bounds.bottom() - scaled(config::TOOLTIP_MAX_HEIGHT, scale));
        if w < scaled(config::TOOLTIP_TITLE_WIDTH, scale) || top >= bounds.bottom() {
            return None;
        }
        let native_strip = screen.grab(Rect::new(x, top, w, bounds.bottom() - top));
        // The title's bullet pins the tooltip's top exactly where its border cannot be told from the scenery. It is
        // looked for in a strip reaching a little left of `x`: a corner a rounded pixel left of it is still this one.
        let column = scaled(BULLET_COLUMN, scale);
        let left = (x - column).max(bounds.x);
        let wide_strip = screen.grab(Rect::new(left, top, w + (x - left), bounds.bottom() - top));
        for corner in find::bullet_spots(&wide_strip, scale) {
            let corner = Point { x: corner.x - (x - left), ..corner };
            if corner.x.abs() > column || top + corner.y > usual_top {
                continue;
            }
            let (cx, cy) = ((corner.x + (x - left)) as usize, corner.y as usize);
            let native =
                wide_strip.crop(cx, cy, cx + w as usize, cy + scaled(config::TOOLTIP_CAPTURE_SIZE.1, scale) as usize);
            let image = normalised(&native, scale);
            let (read, item, crafted) = match self.font_lookup(&native, &image, scale) {
                Some((read, Some(item), crafted)) => (read, item, crafted),
                // As at the usual spot, the corner being pinned by the bullet in the cursor's column: an enhanced
                // item's orange title is seldom read in full by the font. A plain title's row only (see `search`).
                _ if find_bullet(&image, 0).is_some() => {
                    self.reader.allow_ocr(true);
                    let title = self.reader.read_title(&image, super::title_width(&image), 1);
                    let (item, crafted) = self.lookup(&image, &title);
                    let at = Point { x: x + corner.x, y: top + corner.y };
                    let Some(item) = item.or_else(|| self.graded(screen, at, bounds, scale, &title)) else { continue };
                    (title, item, crafted)
                }
                _ => continue,
            };
            let screen_top = top + corner.y;
            let at =
                Placement { pushed_up_height: Some(bounds.bottom() - screen_top), crafted, ..Placement::default() };
            return Some(self.hover(&image, &read, Some(item), bounds, scale, now, at));
        }
        // The game draws its pointer into the frame, here over the tooltip it opened: its white glove reads as title
        // ink on the rows above the title. It is painted over with fill before rows are looked at.
        let mut strip = normalised(&native_strip, scale);
        let (px, py) =
            (((cursor.x - x) as f32 / scale).round() as i32, ((cursor.y - top) as f32 / scale).round() as i32);
        for y in py.max(0)..(py + POINTER.1).min(strip.height as i32) {
            for x in (px - POINTER_LEFT).max(0)..(px + POINTER.0).min(strip.width as i32) {
                let i = (y as usize * strip.width + x as usize) * 3;
                strip.data[i..i + 3].copy_from_slice(&POINTER_FILL);
            }
        }
        for row in candidate_tops(&strip) {
            let screen_top = top + scaled(row as i32, scale);
            if screen_top > usual_top {
                continue; // only ever pushed up, never down
            }
            let image = strip.crop(0, row, strip.width, row + config::TOOLTIP_CAPTURE_SIZE.1 as usize);
            let native_top = scaled(row as i32, scale) as usize;
            let native = native_strip.crop(
                0,
                native_top,
                native_strip.width,
                native_top + scaled(config::TOOLTIP_CAPTURE_SIZE.1, scale) as usize,
            );
            let by_font = self.font_lookup(&native, &image, scale);
            self.reader.allow_ocr(super::tooltip_evidence(&image));
            let (title, item, crafted) = match by_font {
                Some((read, item, crafted)) => (read, item, crafted),
                None => {
                    let title = self.reader.read_title(&image, super::title_width(&image), 1);
                    let (item, crafted) = self.lookup(&image, &title);
                    (title, item, crafted)
                }
            };
            let Some(item) = item.or_else(|| self.graded(screen, Point { x, y: screen_top }, bounds, scale, &title))
            else {
                continue;
            };
            let at =
                Placement { pushed_up_height: Some(bounds.bottom() - screen_top), crafted, ..Placement::default() };
            return Some(self.hover(&image, &title, Some(item), bounds, scale, now, at));
        }
        None
    }

    /// No mouse position (the browser): find the tooltip anywhere in the game, and read it the same way.
    pub(super) fn search(&mut self, screen: &mut dyn Screen, bounds: Rect, scale: f32, now: f64) -> Option<Hover> {
        let frame = screen.grab(bounds);
        let (w, h) = (scaled(config::TOOLTIP_CAPTURE_SIZE.0, scale), scaled(config::TOOLTIP_CAPTURE_SIZE.1, scale));
        // A tooltip found a moment ago is first looked for where it was: a few hundred pixels instead of the whole
        // frame (the cost of a look drops from ~30 ms to ~2). The whole frame is still searched every few looks,
        // for a compare tooltip or a new one, and at once when the old spot holds no tooltip any more.
        let spots = match self.last_spot {
            Some(spot)
                if self.looks_since_search < config::FULL_SEARCH_EVERY && find::tooltip_at(&frame, scale, spot) =>
            {
                self.looks_since_search += 1;
                vec![spot]
            }
            _ => {
                self.looks_since_search = 0;
                find::find_tooltips(&frame, scale)
            }
        };
        self.last_spot = None;
        // Tooltips whose title bullet shows: a spot inside one of them (an icon box's edge, a body line's row) is not a
        // tooltip of its own, however its text reads.
        let anchored = find::bullet_spots(&frame, scale);
        let inside = |spot: Point| {
            anchored.iter().any(|c| {
                (spot.x - c.x).abs().max((spot.y - c.y).abs()) > scaled(INSIDE_MARGIN, scale)
                    && (c.x - scaled(INSIDE_MARGIN, scale)..c.x + scaled(config::TOOLTIP_WIDTHS[1], scale))
                        .contains(&spot.x)
                    && (c.y - scaled(INSIDE_MARGIN, scale)..c.y + scaled(config::TOOLTIP_MAX_HEIGHT, scale))
                        .contains(&spot.y)
            })
        };
        self.tooltip_in_view = !spots.is_empty();
        for spot in spots {
            let native = frame.crop(spot.x as usize, spot.y as usize, (spot.x + w) as usize, (spot.y + h) as usize);
            let image = normalised(&native, scale);
            // A spot found inside a tooltip (its icon box's edge looks like a border) reads the end of the title, and
            // that can be an item's whole name ("...Leather") or close to one ("...ive Potion" for "Blue Potion"). Its
            // title row has that tooltip's bullet to the left, or (a body line's spot, "Type: Spear") above; a real
            // corner's title starts with its own bullet (unless the pointer is over it, as in a stream's frames).
            if !has_bullet(&image) && (inside(spot) || bullet_left_of(&frame, spot, scale)) {
                continue;
            }
            self.reader.allow_ocr(true); // found by its fill and borders
            let by_font = self.font_lookup(&native, &image, scale);
            let title = match &by_font {
                Some((read, _, _)) => read.clone(),
                None => self.reader.read_title(&image, super::title_width(&image), 1),
            };
            let (item, crafted) = match by_font {
                Some((_, item, crafted)) => (item, crafted),
                None if self.last_title.as_deref() == Some(title.as_str()) => {
                    (self.last_item.clone(), self.last_crafted)
                }
                None => self.lookup(&image, &title),
            };
            let corner = Point { x: bounds.x + spot.x, y: bounds.y + spot.y };
            let item = item.or_else(|| self.graded(screen, corner, bounds, scale, &title));
            if item.is_none() {
                // The bullet in front of the title is what tells a tooltip from any other stable text on a dark
                // blue background (a slide's heading read "EARRINGS" for a minute). A spot without one (a line inside
                // a tooltip, found too) is passed over before it touches the count below: its read in the same
                // look had reset the tooltip's own every time, and an item with no price never showed.
                if find_bullet(&image, 0).is_none() || !looks_like_title(&title) {
                    continue;
                }
                // As with a mouse: a title that names nothing shows a "no data" card only once it has read the same
                // several frames in a row. Body lines of a tooltip ("Attack Speed: Fast") and half-read titles
                // ("Steel" for "Steel Slain (+6)") change from frame to frame, an item's name does not.
                self.unknown_streak =
                    if self.unknown_title.as_deref() == Some(title.as_str()) { self.unknown_streak + 1 } else { 1 };
                self.unknown_title = Some(title.clone());
                if self.unknown_streak < config::UNKNOWN_TITLE_FRAMES {
                    continue;
                }
            }
            if self.last_title.as_deref() != Some(title.as_str()) {
                self.compare_seen_at = f64::NEG_INFINITY;
                if let Some(item) = &item {
                    self.keep_capture(&image, &item.name);
                }
            }
            self.last_title = Some(title.clone());
            self.last_item = item.clone();
            self.last_crafted = crafted;
            self.last_spot = Some(spot);
            let found_at = Some(Point { x: bounds.x + spot.x, y: bounds.y + spot.y });
            let at = Placement { found_at, crafted, ..Placement::default() };
            return Some(self.hover(&image, &title, item, bounds, scale, now, at));
        }
        // Over dark scenery the tooltip's border is as dark as its fill and the search above can settle on the icon's
        // box inside it. Every title starts with its light-blue bullet at a fixed spot: a tooltip is also looked for
        // there.
        if self.last_spot.is_none() {
            let mut loose_reads = 0;
            for spot in find::bullet_spots(&frame, scale) {
                let native = frame.crop(spot.x as usize, spot.y as usize, (spot.x + w) as usize, (spot.y + h) as usize);
                let image = normalised(&native, scale);
                let (title, item, crafted) = match self.font_lookup(&native, &image, scale) {
                    Some((title, Some(item), crafted)) => (title, item, crafted),
                    // The bullet pins the corner as a mouse does: the title is read as at the usual spot (an enhanced
                    // item's orange title is seldom read in full by the font). Only at a plain title's row: a crafted
                    // item's corner, above the maker line, reads the scenery there as its first line.
                    // A busy frame has dozens of light-blue dots: only the first few get the reading OCR may be
                    // asked for (the page keeps only its newest few OCR jobs, and a sky's specks would push the title's
                    // out).
                    _ if find_bullet(&image, 0).is_some() && loose_reads < MAX_LOOSE_READS => {
                        loose_reads += 1;
                        self.reader.allow_ocr(true);
                        let title = self.reader.read_title(&image, super::title_width(&image), 1);
                        let (item, crafted) = self.lookup(&image, &title);
                        let corner = Point { x: bounds.x + spot.x, y: bounds.y + spot.y };
                        let Some(item) = item.or_else(|| self.graded(screen, corner, bounds, scale, &title)) else {
                            continue;
                        };
                        (title, item, crafted)
                    }
                    _ => continue,
                };
                self.last_title = Some(title.clone());
                self.last_item = Some(item.clone());
                self.last_crafted = crafted;
                self.last_spot = Some(spot);
                let found_at = Some(Point { x: bounds.x + spot.x, y: bounds.y + spot.y });
                let at = Placement { found_at, crafted, ..Placement::default() };
                return Some(self.hover(&image, &title, Some(item), bounds, scale, now, at));
            }
        }
        None
    }
}
