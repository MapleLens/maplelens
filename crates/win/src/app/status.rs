//! The store status pill: only in our own store, only over the game (rule 5), and only if the player wants it.

use std::sync::Arc;

use maplelens_core::geometry::ui_scale;
use maplelens_core::render::status_pill::{place_status, render_status};
use windows::Win32::Foundation::HWND;

use super::App;
use crate::game;

impl App {
    pub(super) fn follow_status(&mut self) {
        while let Ok(update) = self.shop_events.try_recv() {
            self.shop = update;
        }
        let hwnd = HWND(self.shop.hwnd as *mut _);
        let client = (self.shop.hwnd != 0 && game::is_alive(hwnd) && game::is_foreground(hwnd))
            .then(|| game::client_rect(hwnd))
            .flatten();
        let Some(client) = client.filter(|_| self.settings.store_status_bar) else {
            return self.status_window.hide();
        };
        // Like the price card: the pill follows the game's UI scale (height-based, as the HUD it sits beside).
        let scale = (ui_scale(client) * 100.0).round() / 100.0;
        let key = (self.shop.status, scale.to_bits());
        if self.status_card.as_ref().is_none_or(|(k, _)| *k != key) {
            let card = render_status(&self.fonts, self.shop.status, scale).map(Arc::new);
            self.status_card = Some((key, card));
            self.status_window.hide(); // redrawn below with the new image
        }
        let Some((_, Some(card))) = &self.status_card else { return self.status_window.hide() };
        let card = card.clone();
        let (x, y) = place_status(client, (card.width as i32, card.height as i32), scale);
        if self.status_window.visible() {
            self.status_window.move_to(x, y);
        } else {
            self.status_window.set_image(&card, x, y);
        }
    }
}
