//! The map helper's detection: read the world-map tooltip next to the cursor ("Victoria Road : Henesys").

use std::collections::HashMap;

use super::{Screen, grab_normalised};
use crate::config;
use crate::geometry::Rect;
use crate::mapinfo::vote::Voter;
use crate::mapinfo::{Location, resolve};
use crate::vision::map_tooltip::{LineOcr, find_tooltip, may_hold_tooltip, text_mask};

/// A world-map tooltip is showing next to the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct MapHover {
    pub location: Location,
    /// Tooltip top-left relative to the cursor, screen px.
    pub offset: (i32, i32),
    /// Tooltip size, screen px.
    pub size: (i32, i32),
    /// Game client area in screen coordinates.
    pub bounds: Rect,
    pub scale: f32,
}

type Reading = (Location, (i32, i32), (i32, i32));

/// Turns frames into `MapHover`s; OCR readings vary frame to frame, so one only counts once several agree.
pub struct MapScanner {
    ocr: Box<dyn LineOcr + Send>,
    voter: Voter<Reading>,
    last_reading: f64,
    /// Readings that matched no map, until when they are ignored (OCR then tries other readings).
    rejected: HashMap<String, f64>,
    /// No OCR before this: the last one found no map tooltip, or one that names no map.
    ocr_rests_until: f64,
    /// Debug aid: the latest scan that held tooltip-like colours (source of test fixtures).
    pub capture: Option<crate::image::RgbImage>,
    pub keep_captures: bool,
    /// Tooltip colours were last seen near the cursor until this time: the caller scans fast while it is future.
    active_until: f64,
}

impl MapScanner {
    pub fn new(ocr: Box<dyn LineOcr + Send>) -> Self {
        Self {
            ocr,
            voter: Voter::default(),
            last_reading: f64::NEG_INFINITY,
            rejected: HashMap::new(),
            ocr_rests_until: f64::NEG_INFINITY,
            capture: None,
            keep_captures: false,
            active_until: f64::NEG_INFINITY,
        }
    }

    /// Is something tooltip-like near the cursor (or was there a moment ago)? Then the caller scans often.
    pub fn is_active(&self, now: f64) -> bool {
        now < self.active_until || now - self.last_reading <= config::MAP_HIDE_GRACE_S
    }

    /// An item tooltip is up (equips above all are full of text that is not a map title): nothing is looked for.
    pub fn stand_down(&mut self) {
        self.voter.clear();
    }

    /// The UI found no map for this reading (a misread): ignore it for a while so a better one can win.
    pub fn reject(&mut self, location: &Location, now: f64) {
        self.rejected.insert(resolve::key(location), now + config::MAP_REJECT_S);
        self.voter.clear();
        self.ocr_rests_until = now + config::MAP_IDLE_OCR_S;
    }

    pub fn step(&mut self, screen: &mut dyn Screen, bounds: Rect, scale: f32, now: f64) -> Option<MapHover> {
        let cursor = screen.cursor()?;
        let scaled = |v: i32| (v as f32 * scale).round() as i32;
        let (x, y) = (cursor.x + scaled(config::MAP_SCAN_OFFSET.0), cursor.y + scaled(config::MAP_SCAN_OFFSET.1));
        let w = scaled(config::MAP_SCAN_SIZE.0).min(bounds.right() - x);
        let h = scaled(config::MAP_SCAN_SIZE.1).min(bounds.bottom() - y);
        if x < bounds.x || y < bounds.y || w < scaled(config::MAP_TOOLTIP_MIN_SIZE.0) || h <= 0 {
            return self.forget(now); // the cursor is too close to the edge of the game
        }
        let image = grab_normalised(screen, Rect::new(x, y, w, h), scale);
        let mask = text_mask(&image);
        if !may_hold_tooltip(&image, &mask) {
            return self.forget(now);
        }
        self.active_until = now + config::MAP_ACTIVE_S;
        if now < self.ocr_rests_until {
            return self.forget(now);
        }
        if self.keep_captures {
            self.capture = Some(image);
        }
        let lines = self.ocr.read_lines(&mask);
        let (min_w, min_h) = config::MAP_TOOLTIP_MIN_SIZE;
        let Some(found) = find_tooltip(&lines).filter(|f| f.rect.w >= min_w && f.rect.h >= min_h) else {
            self.ocr_rests_until = now + config::MAP_IDLE_OCR_S;
            return self.forget(now);
        };
        let rect = found.rect;
        let offset = (x - cursor.x + scaled(rect.x), y - cursor.y + scaled(rect.y));
        let size = (scaled(rect.w), scaled(rect.h));
        self.last_reading = now;
        let reading = resolve::key(&found.location);
        if self.rejected.get(&reading).is_some_and(|&until| until > now) {
            self.ocr_rests_until = now + config::MAP_IDLE_OCR_S;
            return None; // this misreading already failed: wait for another
        }
        let (location, offset, size) = self.voter.add(&reading, (found.location, offset, size))?;
        Some(MapHover { location, offset, size, bounds, scale })
    }

    /// No tooltip this time; a single missed read is bridged, a longer gap starts the vote afresh.
    fn forget(&mut self, now: f64) -> Option<MapHover> {
        if now - self.last_reading > config::MAP_FORGET_S {
            self.voter.clear();
        }
        None
    }
}
