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
        let (Some(item), crafted) = self.lookup(&image, &title) else { return None };
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
        // The title's bullet pins the tooltip's top exactly where its border cannot be told from the scenery.
        let column = scaled(BULLET_COLUMN, scale);
        for corner in find::bullet_spots(&native_strip, scale) {
            if corner.x.abs() > column || top + corner.y > usual_top {
                continue;
            }
            let (cx, cy) = (corner.x.max(0) as usize, corner.y as usize);
            let native = native_strip.crop(
                cx,
                cy,
                native_strip.width,
                cy + scaled(config::TOOLTIP_CAPTURE_SIZE.1, scale) as usize,
            );
            let image = normalised(&native, scale);
            if let Some((read, Some(item), crafted)) = self.font_lookup(&native, &image, scale) {
                let screen_top = top + corner.y;
                let at =
                    Placement { pushed_up_height: Some(bounds.bottom() - screen_top), crafted, ..Placement::default() };
                return Some(self.hover(&image, &read, Some(item), bounds, scale, now, at));
            }
        }
        let strip = normalised(&native_strip, scale);
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
            let Some(item) = item else { continue };
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
        for spot in spots {
            let native = frame.crop(spot.x as usize, spot.y as usize, (spot.x + w) as usize, (spot.y + h) as usize);
            let image = normalised(&native, scale);
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
            if item.is_none() {
                // As with a mouse: a title that names nothing shows a "no data" card only once it has read the same
                // several frames in a row. Body lines of a tooltip ("Attack Speed: Fast") and half-read titles
                // ("Steel" for "Steel Slain (+6)") change from frame to frame, an item's name does not.
                self.unknown_streak =
                    if self.unknown_title.as_deref() == Some(title.as_str()) { self.unknown_streak + 1 } else { 1 };
                self.unknown_title = Some(title.clone());
                // The bullet in front of the title is what tells a tooltip from any other stable text on a dark
                // blue background (a slide's heading read "EARRINGS" for a minute).
                let bullet = find_bullet(&image, 0).is_some();
                if !bullet || !looks_like_title(&title) || self.unknown_streak < config::UNKNOWN_TITLE_FRAMES {
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
        // there, and counts only when the game's own font names a catalog item at its title.
        if self.last_spot.is_none() {
            for spot in find::bullet_spots(&frame, scale) {
                let native = frame.crop(spot.x as usize, spot.y as usize, (spot.x + w) as usize, (spot.y + h) as usize);
                let image = normalised(&native, scale);
                let Some((title, Some(item), crafted)) = self.font_lookup(&native, &image, scale) else { continue };
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
