//! Tooltips that are not at mouse + offset: pushed up by the game's bottom clamp, or searched for when there is
//! no mouse position (the browser).

use super::{Hover, HoverDetector, Placement, looks_like_title};
use crate::config;
use crate::geometry::{Point, Rect, scaled, ui_scale};
use crate::image::RgbImage;
use crate::market::catalog::Catalog;
use crate::pipeline::{Screen, grab_normalised};
use crate::vision::find;
use crate::vision::locate::candidate_tops;
use crate::vision::reader::find_bullet;
use crate::vision::tooltip::measure_width;

/// How far (UI scale 1) a spot may be from a bullet's corner and still be that tooltip.
const INSIDE_MARGIN: i32 = 8;
/// "UI Size: Ideal Ratio" draws the UI at this scale whatever the window's size (from 1440 px high, at twice it).
const IDEAL_RATIO_SCALE: f32 = 0.641;
const IDEAL_RATIO_DOUBLED_FROM: i32 = 1440;
/// How often (seconds) the search looks at the window's other UI size.
const OTHER_SCALE_EVERY_S: f64 = 1.0;
/// Two scales this close are the same UI size.
const SAME_SCALE: f32 = 0.02;

/// The scale a search pass reads at: the voted one, or the window's other UI size, where a read needs proof.
#[derive(Clone, Copy)]
enum AtScale {
    Voted(f32),
    Other(f32),
}

/// The window's other UI size than `scale`: its default (the height's) or Ideal Ratio, unless both are one.
fn other_ui_scale(bounds: Rect, scale: f32) -> Option<f32> {
    let default = ui_scale(bounds);
    let ideal = if bounds.h >= IDEAL_RATIO_DOUBLED_FROM { 2.0 * IDEAL_RATIO_SCALE } else { IDEAL_RATIO_SCALE };
    let other = if (scale - default).abs() < SAME_SCALE { ideal } else { default };
    ((other - scale).abs() >= SAME_SCALE).then_some(other)
}

impl HoverDetector {
    /// Look for a known item's tooltip the game slid up to fit on screen.
    ///
    /// The game keeps a tooltip inside the client area, so one taller than the room under the mouse has its
    /// bottom on the client's bottom edge and its top somewhere above the usual spot. Only a recognised item
    /// counts here: stray UI text must never be mistaken for a tooltip we had to go looking for.
    pub(super) fn pushed_up(
        &mut self,
        screen: &mut dyn Screen,
        cursor: Point,
        bounds: Rect,
        scale: f32,
        now: f64,
    ) -> Option<Hover> {
        let x = cursor.x + scaled(config::TOOLTIP_OFFSET.0, scale);
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
        // A tooltip found a moment ago is first looked for where it was: a few hundred pixels instead of the whole
        // frame (the cost of a look drops from ~30 ms to ~2). The whole frame is still searched every few looks,
        // for a compare tooltip or a new one, and at once when the old spot holds no tooltip any more.
        let followed = match self.last_spot {
            Some(spot)
                if self.looks_since_search < config::FULL_SEARCH_EVERY && find::tooltip_at(&frame, scale, spot) =>
            {
                self.looks_since_search += 1;
                Some(spot)
            }
            _ => None,
        };
        self.last_spot = None;
        if let Some(spot) = followed {
            return self.read_spots(&frame, bounds, AtScale::Voted(scale), &[spot], &[], now);
        }
        self.looks_since_search = 0;
        let (spots, bullets) = (find::find_tooltips(&frame, scale), find::bullet_spots(&frame, scale));
        self.tooltip_in_view = !spots.is_empty() || !bullets.is_empty();
        if let Some(hover) = self.read_spots(&frame, bounds, AtScale::Voted(scale), &spots, &bullets, now) {
            return Some(hover);
        }
        // Just after the player switches UI Size the voted scale is still the old one for a few seconds (it needs
        // several readings), and nothing is found at it. The window draws its UI at one of two scales only, so the
        // other one is looked at by the bullet search now and then (it doubles the cost of a look), and a title read
        // there counts only with proof the tooltip is drawn at that scale: read at 0.64, a scroll drawn at 0.9 had come
        // out as "...Scroll: " and a fuzzy match named another grade.
        if now - self.other_scale_at < OTHER_SCALE_EVERY_S {
            return None;
        }
        self.other_scale_at = now;
        let other = other_ui_scale(bounds, scale)?;
        let bullets = find::bullet_spots(&frame, other);
        self.tooltip_in_view |= !bullets.is_empty();
        self.read_spots(&frame, bounds, AtScale::Other(other), &[], &bullets, now)
    }

    /// The first of `spots` (found by fill and borders) and of `bullets`' corners that reads as a tooltip.
    fn read_spots(
        &mut self,
        frame: &RgbImage,
        bounds: Rect,
        at: AtScale,
        spots: &[Point],
        bullets: &[Point],
        now: f64,
    ) -> Option<Hover> {
        let (AtScale::Voted(scale) | AtScale::Other(scale)) = at;
        let (w, h) = (scaled(config::TOOLTIP_CAPTURE_SIZE.0, scale), scaled(config::TOOLTIP_CAPTURE_SIZE.1, scale));
        // A spot inside a tooltip whose bullet shows (its icon box's edge, a body line's row) is not a tooltip of its
        // own: its "title" is the end of the real one ("Snail Shell" off "Blue Snail Shell", a wrong price live).
        let margin = scaled(INSIDE_MARGIN, scale);
        let inside = |spot: Point| {
            bullets.iter().any(|c| {
                (spot.x - c.x).abs().max((spot.y - c.y).abs()) > margin
                    && (c.x - margin..c.x + scaled(config::TOOLTIP_WIDTHS[1], scale)).contains(&spot.x)
                    && (c.y - margin..c.y + scaled(config::TOOLTIP_MAX_HEIGHT, scale)).contains(&spot.y)
            })
        };
        // Then the bullets' corners the fill search did not find: over a busy backdrop it finds no band or border.
        let near = |a: Point, b: Point| (a.x - b.x).abs().max((a.y - b.y).abs()) <= margin;
        let by_bullet = bullets.iter().filter(|c| !spots.iter().any(|s| near(**c, *s))).map(|c| (*c, true));
        let candidates: Vec<(Point, bool)> = spots.iter().map(|s| (*s, false)).chain(by_bullet).collect();
        for (spot, bullet_only) in candidates {
            let native = frame.crop(spot.x as usize, spot.y as usize, (spot.x + w) as usize, (spot.y + h) as usize);
            let image = if (scale - 1.0).abs() > 0.01 { native.resized(1.0 / scale) } else { native };
            let has_bullet = find_bullet(&image, 0).is_some();
            if (bullet_only && !has_bullet) || (!has_bullet && inside(spot)) {
                continue;
            }
            self.reader.allow_ocr(true); // found by its fill and borders, or by its bullet
            let title = self.reader.read_title(&image, super::title_width(&image), 1);
            let (item, crafted) = if self.last_title.as_deref() == Some(title.as_str()) {
                (self.last_item.clone(), self.last_crafted)
            } else {
                self.lookup(&image, &title)
            };
            // A corner found by its bullet alone counts only when its title names an item: no unknown-title card.
            if bullet_only && item.is_none() {
                continue;
            }
            // At a scale the tooltip may not be drawn at, the read must be the item's very name, or the tooltip's right
            // border must stand where one of the game's widths puts it at that scale (it does not at a wrong one).
            let named_exactly = || item.as_ref().is_some_and(|it| Catalog::reads_as(&title, &it.name));
            if matches!(at, AtScale::Other(_))
                && !(named_exactly() || measure_width(&image, &config::TOOLTIP_WIDTHS).is_some())
            {
                continue;
            }
            if item.is_none() {
                // The bullet in front of the title is what tells a tooltip from any other stable text on a dark
                // blue background (a slide's heading read "EARRINGS" for a minute). A spot without one (a line inside
                // a tooltip, found too) is passed over before it touches the count below: its read in the same
                // look had reset the tooltip's own every time, and an item with no price never showed.
                if !has_bullet || !looks_like_title(&title) {
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
        None
    }
}
