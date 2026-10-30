//! The training card: EXP rates as a strip as tall as the status bar, in the corner of the game the player picked (at
//! the bottom, level with the bar) or where they dragged it, only over the game (rule 5) and only while the player has
//! it switched on (settings or tray). Its bag button opens the map drops panel. The party quest helper joins it only
//! with settings.json's hidden `quest_helper`.

use std::sync::Arc;

use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect, scaled};
use maplelens_core::placement::{fraction_of, place_at, place_in_corner};
use maplelens_core::render::theme;
use maplelens_core::render::training_card::{Button, Show, Strip, has_content, on_drops_button, render_training_card};
use windows::Win32::Foundation::HWND;

use super::App;
use super::pointer::Card;
use crate::game;
use crate::training::TrainingCommand;

/// Away from the client's corner, at UI scale 1.
pub(super) const MARGIN: f32 = 10.0;
/// The status bar's height at UI scale 1 (68 at Ideal Ratio's 0.64): a bottom card that would cover it goes above.
const STATUS_BAR_HEIGHT: i32 = 106;

impl App {
    pub(super) fn follow_training(&mut self) {
        while let Ok(update) = self.training_events.try_recv() {
            self.training = Some(update);
        }
        let paused = self.training.as_ref().is_some_and(|(_, t)| t.exp.paused);
        self.tray.set_training(self.settings.training_card, paused);
        let show = Show { exp: self.settings.training_card, quest: self.settings.quest_helper, strip: None };
        let Some((hwnd, training)) = self.training.as_ref().filter(|(_, t)| has_content(t, show)) else {
            // The panel opens from the card: gone with it (not while the game is only covered).
            self.drops.close();
            return self.training_window.hide();
        };
        let hwnd = HWND(*hwnd as *mut _);
        let client = (self.settings.overlay_enabled && game::is_alive(hwnd) && game::is_foreground(hwnd))
            .then(|| game::client_rect(hwnd))
            .flatten();
        let Some(client) = client else { return self.training_window.hide() };
        let scale = (self.ui.of(client) * 100.0).round() / 100.0;
        let button = self.drops.available().then(|| match game::cursor() {
            _ if self.drops.is_open() => Button::On,
            Some(cursor) if self.over_drops_button(cursor, scale) => Button::Lit,
            _ => Button::Idle,
        });
        let show = Show { strip: Some(Strip { drops: button }), ..show };
        let now = crate::paths::now_unix();
        // The quest's clock ticks every second even when nothing else changes.
        let key = (training.clone(), show, scale.to_bits(), now as u64);
        let stale = self.training_card.as_ref().is_none_or(|(k, _)| *k != key);
        if stale {
            let card = render_training_card(&self.fonts, training, show, now, scale).map(Arc::new);
            self.training_card = card.map(|c| (key, c));
        }
        let Some((_, card)) = &self.training_card else { return self.training_window.hide() };
        let card = card.clone();
        let margin = (MARGIN * scale).round() as i32;
        let bar = status_bar(client, scale);
        let card_size = (card.width as i32, card.height as i32);
        let corner = self.settings.training_corner;
        let dragged = self.pointer.dragged(Card::Training).map(|at| fraction_of(client, at));
        let placed = match dragged.or(self.settings.training_at) {
            Some(fraction) => place_at(client, card_size, fraction),
            None => place_in_corner(client, card_size, corner, margin, Some(bar), training.minimap_right),
        };
        // Never outside the game.
        let Some((x, y)) = placed else { return self.training_window.hide() };
        if stale || !self.training_window.visible() {
            self.training_window.set_image(&card, x, y);
        } else {
            self.training_window.move_to(x, y);
        }
    }

    /// Is `cursor` on the card's map drops button (as last drawn at `scale`)?
    pub(super) fn over_drops_button(&self, cursor: Point, scale: f32) -> bool {
        let Some(card) = self.training_window.rect().filter(|r| self.training_window.visible() && r.contains(cursor))
        else {
            return false;
        };
        let factor = theme::CARD_SCALE * scale;
        self.drops.available()
            && on_drops_button((cursor.x - card.x) as f32 / factor, (cursor.y - card.y) as f32 / factor)
    }

    pub(super) fn send_training(&self, command: TrainingCommand) {
        if self.training_commands.send(command).is_err() {
            crate::warn!("the training reader has stopped");
        }
    }
}

/// The status bar on screen: a card at the bottom of the game sits above it.
pub(super) fn status_bar(client: Rect, scale: f32) -> Rect {
    let (bar_w, bar_h) = (scaled(config::STATUS_BAR_WIDTH, scale), scaled(STATUS_BAR_HEIGHT, scale));
    Rect::new(client.x + (client.w - bar_w) / 2, client.bottom() - bar_h, bar_w, bar_h)
}
