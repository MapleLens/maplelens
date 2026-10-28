//! The map card on the UI thread: placed beside the world-map tooltip from the live cursor, like the price card.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use maplelens_core::config;
use maplelens_core::geometry::Rect;
use maplelens_core::mapinfo::{Location, resolve};
use maplelens_core::pipeline::map::MapHover;
use maplelens_core::placement::place_card;
use maplelens_core::render::RgbaImage;
use maplelens_core::render::font::Fonts;
use maplelens_core::render::map_card::{SPINNER_FRAMES, render_loading_card, render_map_card};

use crate::game;
use crate::layered::LayeredWindow;
use crate::maps::service::{MapResult, MapService, Status};

/// The loading ring's frames take a dozen slots.
const CARD_CACHE_SIZE: usize = 48;
const SPINNER_FPS: f64 = 12.0;

/// (reading key, status, sprites ready, spinner frame, UI scale bits)
type CardKey = (String, Status, usize, usize, u32);

pub struct MapUi {
    events: Receiver<Option<MapHover>>,
    rejects: Sender<Location>,
    /// Tells the scanner to stand down while an item tooltip (and its price card) is up.
    pub item_showing: Arc<AtomicBool>,
    service: Arc<MapService>,
    window: LayeredWindow,
    hover: Option<MapHover>,
    seen_at: f64,
    shown: Option<CardKey>,
    logged: Option<(Location, Status)>,
    cards: Vec<(CardKey, Arc<RgbaImage>)>,
}

impl MapUi {
    pub fn new(
        events: Receiver<Option<MapHover>>,
        rejects: Sender<Location>,
        item_showing: Arc<AtomicBool>,
        service: Arc<MapService>,
    ) -> windows::core::Result<Self> {
        Ok(Self {
            events,
            rejects,
            item_showing,
            service,
            window: LayeredWindow::new()?,
            hover: None,
            seen_at: 0.0,
            shown: None,
            logged: None,
            cards: Vec::new(),
        })
    }

    /// The map lookups, shared with the map drops panel.
    pub fn service(&self) -> &Arc<MapService> {
        &self.service
    }

    pub fn hide(&mut self) {
        self.hover = None;
        self.shown = None;
        self.window.hide();
    }

    /// Throw away what the idle scanner keeps posting while the overlay is switched off.
    pub fn drain(&mut self) {
        while self.events.try_recv().is_ok() {}
    }

    /// An item tooltip is up: the map helper stands down (no scanning, no lookups, no card).
    pub fn set_item_showing(&mut self, showing: bool) {
        self.item_showing.store(showing, Ordering::Relaxed);
        if showing {
            self.hide();
        }
    }

    pub fn tick(&mut self, fonts: &Fonts, now: f64) {
        while let Ok(event) = self.events.try_recv() {
            if let Some(hover) = event {
                // Misses are judged by time below, not by a single scan.
                (self.hover, self.seen_at) = (Some(hover), now);
            }
        }
        let Some(hover) = self.hover.clone().filter(|_| now - self.seen_at <= config::MAP_HIDE_GRACE_S) else {
            return self.hide();
        };
        let result = self.service.get(&hover.location);
        if self.logged.as_ref() != Some(&(hover.location.clone(), result.status)) {
            crate::debug!(
                "map hover {:?} -> {:?} offset {:?} size {:?}",
                hover.location,
                result.status,
                hover.offset,
                hover.size
            );
            self.logged = Some((hover.location.clone(), result.status));
        }
        if result.status == Status::Missing {
            // Unknown text is noise, not a map: show nothing, and let a better reading of a real title win.
            self.hide();
            let _ = self.rejects.send(hover.location);
            return;
        }
        let (card, key) = self.card_for(fonts, &hover, &result, now);
        let Some(cursor) = game::cursor() else { return };
        let tooltip = Rect::new(cursor.x + hover.offset.0, cursor.y + hover.offset.1, hover.size.0, hover.size.1);
        let gap = (config::CARD_GAP_PX as f32 * hover.scale).round() as i32;
        let (x, y) = place_card(tooltip, (card.width as i32, card.height as i32), gap, hover.bounds, false);
        if self.shown.as_ref() != Some(&key) || !self.window.visible() {
            self.window.set_image(&card, x, y);
            self.shown = Some(key);
        } else {
            self.window.move_to(x, y);
        }
    }

    /// The card for what the lookup has so far, and the key that says when the window must be redrawn.
    fn card_for(&mut self, fonts: &Fonts, hover: &MapHover, result: &MapResult, now: f64) -> (Arc<RgbaImage>, CardKey) {
        // The ring turns while the lookup runs; the failure notice and the finished card stand still.
        let frame = if result.status == Status::Loading { (now * SPINNER_FPS) as usize % SPINNER_FRAMES } else { 0 };
        let key = (resolve::key(&hover.location), result.status, result.icons, frame, hover.scale.to_bits());
        if let Some(at) = self.cards.iter().position(|(k, _)| *k == key) {
            let entry = self.cards.remove(at);
            self.cards.push(entry); // most recently used last
            return (self.cards.last().expect("just pushed").1.clone(), key);
        }
        let card = match &result.info {
            Some(info) if result.status == Status::Found => {
                let service = &self.service;
                let room = (hover.bounds.w / 2, (hover.bounds.h as f32 * 0.96).round() as i32);
                render_map_card(fonts, info, &|kind, id| service.icon(kind, id), hover.scale, Some(room))
            }
            _ => render_loading_card(fonts, &hover.location.name, frame, result.status == Status::Failed, hover.scale),
        };
        let card = Arc::new(card);
        self.cards.push((key.clone(), card.clone()));
        if self.cards.len() > CARD_CACHE_SIZE {
            self.cards.remove(0);
        }
        (card, key)
    }
}
