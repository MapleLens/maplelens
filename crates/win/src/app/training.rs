//! The training card: EXP rates in the corner of the game the player picked, only over the game (rule 5) and only
//! while the player has it switched on (settings or tray). The party quest helper joins it only with settings.json's
//! hidden `quest_helper`.

use std::sync::Arc;

use maplelens_core::config;
use maplelens_core::geometry::{Rect, scaled};
use maplelens_core::placement::place_in_corner;
use maplelens_core::render::training_card::{Show, has_content, render_training_card};
use windows::Win32::Foundation::HWND;

use super::App;
use crate::game;
use crate::training::TrainingCommand;

/// Away from the client's corner, at UI scale 1.
const MARGIN: f32 = 10.0;
/// The status bar's height at UI scale 1 (68 at Ideal Ratio's 0.64): a bottom card that would cover it goes above.
const STATUS_BAR_HEIGHT: i32 = 106;

impl App {
    pub(super) fn follow_training(&mut self) {
        while let Ok(update) = self.training_events.try_recv() {
            self.training = Some(update);
        }
        let paused = self.training.as_ref().is_some_and(|(_, t)| t.exp.paused);
        self.tray.set_training(self.settings.training_card, paused);
        let show = Show { exp: self.settings.training_card, quest: self.settings.quest_helper };
        let Some((hwnd, training)) = self.training.as_ref().filter(|(_, t)| has_content(t, show)) else {
            return self.training_window.hide();
        };
        let hwnd = HWND(*hwnd as *mut _);
        let client = (self.settings.overlay_enabled && game::is_alive(hwnd) && game::is_foreground(hwnd))
            .then(|| game::client_rect(hwnd))
            .flatten();
        let Some(client) = client else { return self.training_window.hide() };
        let scale = (self.ui.of(client) * 100.0).round() / 100.0;
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
        let (bar_w, bar_h) = (scaled(config::STATUS_BAR_WIDTH, scale), scaled(STATUS_BAR_HEIGHT, scale));
        let bar = Rect::new(client.x + (client.w - bar_w) / 2, client.bottom() - bar_h, bar_w, bar_h);
        let card_size = (card.width as i32, card.height as i32);
        let corner = self.settings.training_corner;
        let placed = place_in_corner(client, card_size, corner, margin, Some(bar), training.minimap_right);
        // Never outside the game.
        let Some((x, y)) = placed else { return self.training_window.hide() };
        if stale || !self.training_window.visible() {
            self.training_window.set_image(&card, x, y);
        } else {
            self.training_window.move_to(x, y);
        }
    }

    pub(super) fn send_training(&self, command: TrainingCommand) {
        if self.training_commands.send(command).is_err() {
            crate::warn!("the training reader has stopped");
        }
    }
}
