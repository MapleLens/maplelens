//! The status pill: our own store's status, or a hint about training (`pill::PillPicker` chooses), only over the game
//! (rule 5), and only if the player wants it.

use std::sync::Arc;

use maplelens_core::geometry::ui_scale;
use maplelens_core::pill::Facts;
use maplelens_core::render::status_pill::{place_status, render_status};
use windows::Win32::Foundation::HWND;

use super::App;
use crate::game;
use crate::maps::service::Status;

impl App {
    pub(super) fn follow_status(&mut self) {
        while let Ok(update) = self.shop_events.try_recv() {
            self.shop = update;
        }
        let shown = self.settings.overlay_enabled;
        let no_drops = (shown && self.settings.map_drops).then(|| self.no_drops_map()).flatten();
        let facts = Facts {
            store: self.shop.status,
            exp_card: shown && self.settings.training_card,
            map_drops: shown && self.settings.map_drops,
            training: self.training.as_ref().map(|(_, t)| t),
            no_drops: no_drops.as_deref(),
        };
        let pill = self.pill.pick(&facts, crate::paths::now_unix());
        // The shop thread's game window while it sees one, else the training reader's: both find it the same way.
        let hwnd = match self.shop.hwnd {
            0 => self.training.as_ref().map_or(0, |(hwnd, _)| *hwnd),
            hwnd => hwnd,
        };
        let hwnd = HWND(hwnd as *mut _);
        let client = (!hwnd.0.is_null() && game::is_alive(hwnd) && game::is_foreground(hwnd))
            .then(|| game::client_rect(hwnd))
            .flatten();
        let Some((client, pill)) = client.filter(|_| self.settings.store_status_bar).zip(pill) else {
            return self.status_window.hide();
        };
        // Like the price card: the pill follows the game's UI scale (height-based, as the HUD it sits beside).
        let scale = (ui_scale(client) * 100.0).round() / 100.0;
        let key = (pill, scale.to_bits());
        if self.status_card.as_ref().is_none_or(|(k, _)| *k != key) {
            let card = render_status(&self.fonts, pill, scale).map(Arc::new);
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

    /// The map being played, when the drops panel has nothing to show for it: not found in the map data, or none of
    /// its drops known. None while either is still being looked up.
    fn no_drops_map(&self) -> Option<String> {
        let (_, training) = self.training.as_ref()?;
        let location = training.location.as_ref()?;
        let result = self.map.service().get(location);
        let missing = match result.status {
            Status::Missing | Status::Failed => true,
            Status::Found => result.info.is_some_and(|i| !i.is_town && self.drops.knows_nothing_of(i.id)),
            Status::Loading => false,
        };
        missing.then(|| location.name.clone())
    }
}
