//! The phone's pairing on the UI thread: presses in the settings window, the relay's answers (`crate::phone`), and
//! the code hiding itself after two minutes.

use maplelens_core::notify::pairing::Effect;
use maplelens_core::settings::Settings;

use super::App;
use crate::phone;
use crate::settings_window::PhoneAction;

impl App {
    pub(super) fn phone_action(&mut self, action: PhoneAction) {
        let now = self.now();
        let key = self.settings.phone_key.clone();
        match action {
            PhoneAction::ShowCode if !key.is_empty() && self.pairing.phones.unwrap_or(0) > 0 => {
                self.pairing.note = "A phone is already connected: Remove it first to connect another".into();
            }
            PhoneAction::ShowCode => {
                let key = if key.is_empty() {
                    let Some(key) = phone::new_key() else {
                        self.pairing.note = "Windows gave no random numbers for a key: try again".into();
                        return self.show_phone();
                    };
                    // Saved before the code shows: a phone may pair with it at once.
                    self.update_settings(Settings { phone_key: key.clone(), ..self.settings.clone() });
                    key
                } else {
                    key
                };
                self.pairing.show_code(&key, now);
                crate::info!("phone: pairing code shown");
            }
            PhoneAction::Scanned => {
                self.pairing.checking();
                self.phone.ping_now();
            }
            PhoneAction::Test if !key.is_empty() => {
                self.pairing.testing();
                self.phone.test();
            }
            PhoneAction::Remove if !key.is_empty() => {
                self.pairing.removing();
                self.phone.remove();
            }
            PhoneAction::Test | PhoneAction::Remove => {}
            PhoneAction::HidePanel => self.pairing.hide_panel(),
        }
        self.show_phone();
    }

    /// The relay's answers and the code's countdown; every UI tick.
    pub(super) fn follow_phone(&mut self) {
        while let Some(answer) = self.phone.try_answer() {
            // An answer about a key removed meanwhile (Remove, then Connect again) says nothing about the new one.
            if answer.key != self.settings.phone_key {
                continue;
            }
            if self.pairing.on_reply(answer.route, answer.reply) == Effect::Forget {
                crate::info!("phone: pairing forgotten ({})", self.pairing.note);
                self.update_settings(Settings { phone_key: String::new(), ..self.settings.clone() });
            }
        }
        self.pairing.tick(self.now());
        self.show_phone();
    }

    fn show_phone(&mut self) {
        self.phone.set_fast(self.pairing.fast());
        self.settings_window.set_phone(&self.pairing, self.pairing.seconds_left(self.now()));
    }
}
