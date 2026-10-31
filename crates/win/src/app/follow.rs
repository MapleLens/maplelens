//! The two overlays following what they decorate: the price card the mouse, the rank panel the Character Info
//! window.

use std::sync::Arc;

use maplelens_core::config;
use maplelens_core::geometry::{Rect, card_scale, scaled};
use maplelens_core::pipeline::hover::Hover;
use maplelens_core::placement::{fits_inside, place_beside_window, place_card};
use maplelens_core::render::RgbaImage;
use maplelens_core::render::card::{CardSpec, render_card};
use maplelens_core::render::rank_card::render_rank_card;

use super::{App, CARD_CACHE_SIZE, Shown};
use crate::{game, paths};

impl App {
    /// Runs every tick: the card is placed from the live mouse position, never from stale detections.
    pub(super) fn follow_mouse(&mut self) {
        let now = self.now();
        while let Ok(event) = self.hover_events.try_recv() {
            if let Some(hover) = event {
                // Misses are judged by time below, not by a single frame.
                self.hover = Some(hover);
                self.seen_at = now;
            }
        }
        let (Some(hover), Some(cursor)) = (self.hover.clone(), game::cursor()) else { return self.hide_card() };
        if now - self.seen_at > config::HIDE_GRACE_S {
            return self.hide_card();
        }
        if self.shown.as_ref().is_none_or(|s| s.title != hover.title) {
            self.since = now; // a new item: start with the glance card
        }
        // Progressive disclosure: the glance card first, the full one once the pointer stays on the item.
        let expanded = now - self.since >= self.settings.expand_delay_s;
        let card = self.card_for(&hover, expanded);
        let scale = hover.scale;
        let mut tooltip_y = cursor.y + scaled(config::TOOLTIP_OFFSET.1, scale);
        if let Some(height) = hover.pushed_up_height {
            // The game keeps a tall tooltip's bottom inside the client area.
            tooltip_y = tooltip_y.min(hover.bounds.bottom() - height);
        }
        let tooltip = Rect::new(
            cursor.x + scaled(config::TOOLTIP_OFFSET.0, scale),
            tooltip_y,
            hover.tooltip_size.0,
            hover.tooltip_size.1,
        );
        let size = (card.width as i32, card.height as i32);
        let (x, y) = place_card(tooltip, size, scaled(config::CARD_GAP_PX, scale), hover.bounds, hover.compare);
        if !fits_inside(x, y, size, hover.bounds) {
            return self.hide_card(); // overlays only ever show over the game
        }
        let same = self.shown.as_ref().is_some_and(|s| s.title == hover.title && s.expanded == expanded);
        if same && self.window.visible() {
            self.window.move_to(x, y);
        } else {
            self.window.set_image(&card, x, y);
            self.shown = Some(Shown { title: hover.title.clone(), expanded });
        }
    }

    fn hide_card(&mut self) {
        self.hover = None;
        self.shown = None;
        self.window.hide();
    }

    /// Rendering takes a few ms, so each item's card is drawn once and kept.
    fn card_for(&mut self, hover: &Hover, expanded: bool) -> Arc<RgbaImage> {
        let scale = card_scale(hover.bounds, hover.scale);
        let key = (hover.title.clone(), expanded, scale.to_bits(), hover.compare);
        if let Some(index) = self.cards.iter().position(|(k, _)| *k == key) {
            let entry = self.cards.remove(index);
            self.cards.push(entry.clone());
            return entry.1;
        }
        // The UI's catalog may be newer than the one the detection thread matched against.
        let item = self.catalog.lookup(&hover.title).or_else(|| hover.item.clone());
        let source = if self.catalog.generated_at.is_some() { "meso.watch" } else { "demo data" };
        let spec = CardSpec {
            item: item.as_deref(),
            title: &hover.title,
            generated_at: self.catalog.generated_at.as_deref(),
            now: paths::now_unix(),
            expanded,
            ui_scale: scale,
            wide: hover.compare,
            source,
        };
        let card = Arc::new(render_card(&self.fonts, &spec));
        if self.cards.len() >= CARD_CACHE_SIZE {
            self.cards.remove(0);
        }
        self.cards.push((key, card.clone()));
        card
    }

    /// Keeps the rank panel glued to the Character Info window, which the player can drag around.
    pub(super) fn follow_info_window(&mut self) {
        let now = self.now();
        while let Ok(event) = self.info_events.try_recv() {
            if let Some(info) = event {
                self.info = Some(info);
                self.info_seen_at = now;
            }
        }
        let Some(info) = self.info.clone().filter(|_| now - self.info_seen_at <= config::INFO_HIDE_GRACE_S) else {
            self.info = None;
            self.rank_window.hide();
            return;
        };
        let Some(name) = info.name.clone() else { return self.rank_window.hide() };
        let result = self.ranks.lock().unwrap_or_else(|e| e.into_inner()).get(&name, now);
        if let Some(rank) = &result.rank
            && rank.name.eq_ignore_ascii_case(&name)
            && self.confirmed.as_deref() != Some(name.as_str())
        {
            // classicrank.com knows this exact name, so the read was right: let the reader learn from it.
            let _ = self.confirmations.send((info.read.clone(), rank.name.clone()));
            self.confirmed = Some(name.clone());
        }
        let scale = card_scale(info.bounds, info.scale);
        let key = (name, result.status, result.rank.as_ref().and_then(|r| r.last_checked.clone()), scale.to_bits());
        let unchanged = self.rank_card.as_ref().is_some_and(|(k, _)| *k == key);
        if !unchanged {
            let card = Arc::new(render_rank_card(&self.fonts, &result, paths::now_unix(), scale));
            self.rank_card = Some((key, card));
        }
        let Some((_, card)) = &self.rank_card else { return };
        let card = card.clone();
        let size = (card.width as i32, card.height as i32);
        match place_beside_window(info.rect, size, scaled(config::INFO_CARD_GAP_PX, info.scale), info.bounds) {
            // No room inside the game: show nothing rather than float outside it.
            None => self.rank_window.hide(),
            Some((x, y)) if unchanged && self.rank_window.visible() => self.rank_window.move_to(x, y),
            Some((x, y)) => self.rank_window.set_image(&card, x, y),
        }
    }
}
