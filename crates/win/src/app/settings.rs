//! Applying a settings change: saved at once, then each changed part takes effect.

use std::sync::atomic::Ordering;

use maplelens_core::settings::Settings;

use super::App;
use crate::{paths, startup};

impl App {
    pub(super) fn update_settings(&mut self, new: Settings) {
        if new == self.settings {
            return;
        }
        let old = std::mem::replace(&mut self.settings, new.clone());
        if let Err(error) = paths::write_atomic(&paths::settings_file(), new.to_json().as_bytes()) {
            crate::error!("could not save settings: {error}");
        }
        if new.launch_at_startup != old.launch_at_startup {
            sync_startup(new.launch_at_startup);
        }
        if new.overlay_enabled != old.overlay_enabled {
            self.apply_overlay_enabled(new.overlay_enabled);
        }
        if new.toggle_hotkey != old.toggle_hotkey {
            self.register_hotkey(new.toggle_hotkey);
        }
        if new.check_updates != old.check_updates {
            self.updates.set_auto_check(new.check_updates);
            if new.check_updates {
                self.check_for_update();
            }
        }
        if new.phone_key != old.phone_key {
            self.phone.set_key(&new.phone_key);
        }
        if new.notify() != old.notify() {
            self.notifier.configure(new.notify());
        }
        self.settings_window.sync(new); // the window may be open while the hotkey is pressed
    }

    pub(super) fn apply_overlay_enabled(&mut self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.tray.set_overlay_enabled(enabled);
        if !enabled {
            self.hover = None;
            self.info = None;
            self.shown = None;
            self.window.hide();
            self.rank_window.hide();
            self.map.hide();
            self.training_window.hide();
            self.drops.hide();
            self.pointer.hide();
        }
    }

    pub(super) fn register_hotkey(&mut self, packed: u32) {
        if !self.tray.set_hotkey(packed) {
            crate::warn!("hotkey {packed:#06x} is taken by another program");
        }
    }
}

pub(super) fn sync_startup(enabled: bool) {
    if let Err(error) = startup::set_enabled(enabled, startup::VALUE_NAME) {
        crate::error!("could not update the Windows startup entry: {error}");
    }
}
