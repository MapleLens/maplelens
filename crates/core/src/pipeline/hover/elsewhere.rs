//! Tooltips that are not at mouse + offset: pushed up by the game's bottom clamp or left by its right-edge clamp, or
//! searched for when there is no mouse position (the browser).

use super::{Hover, HoverDetector, Placement, looks_like_title};
use crate::config;
use crate::geometry::{Point, Rect, scaled};
use crate::pipeline::{Screen, grab_normalised};
use crate::vision::find;
use crate::vision::locate::candidate_tops;
use crate::vision::reader::find_bullet;

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
        for width in config::TOOLTIP_WIDTHS {
            let x = bounds.right() - scaled(width, scale);
            if usual_x + scaled(width, scale) <= bounds.right() || x < bounds.x {
                continue; // a tooltip this wide fits where it opens: not slid
            }
            if let Some(hover) = self.pushed_left(screen, x, cursor, bounds, scale, now) {
                return Some(hover);
            }
            if let Some(hover) = self.pushed_up(screen, x, cursor, bounds, scale, now) {
                return Some(hover);
            }
        }
        None
    }

    /// A known item's tooltip at the usual height but slid left to `x` by the game's right-edge clamp.
    fn pushed_left(
        &mut self,
        screen: &mut dyn Screen,
        x: i32,
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
        let image = grab_normalised(screen, Rect::new(x, y, w, h), scale);
        if !super::tooltip_evidence(&image) {
            return None; // nothing tooltip-like there: no read, so an idle pointer by the edge costs one grab
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
        let strip = grab_normalised(screen, Rect::new(x, top, w, bounds.bottom() - top), scale);
        for row in candidate_tops(&strip) {
            let screen_top = top + scaled(row as i32, scale);
            if screen_top > usual_top {
                continue; // only ever pushed up, never down
            }
            let image = strip.crop(0, row, strip.width, row + config::TOOLTIP_CAPTURE_SIZE.1 as usize);
            self.reader.allow_ocr(super::tooltip_evidence(&image));
            let title = self.reader.read_title(&image, super::title_width(&image), 1);
            let (Some(item), crafted) = self.lookup(&image, &title) else { continue };
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
            let image = if (scale - 1.0).abs() > 0.01 { native.resized(1.0 / scale) } else { native };
            self.reader.allow_ocr(true); // found by its fill and borders
            let title = self.reader.read_title(&image, super::title_width(&image), 1);
            let (item, crafted) = if self.last_title.as_deref() == Some(title.as_str()) {
                (self.last_item.clone(), self.last_crafted)
            } else {
                self.lookup(&image, &title)
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
        None
    }
}
