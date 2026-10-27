//! Pairing the phone app (pure): what the settings show about it, the pairing panel's steps, and what each answer
//! of the relay (`relay/push`) means. The platform makes the key, talks to the relay and feeds the answers back;
//! the web app's page (`docs/app/app.js`) follows the same rules.

use super::phone::{phone_url, qr_modules};
use super::refusal;

/// The QR code stays up this long at most: a streamer's screen is public, and the code is the key.
pub const QR_SHOWN_S: f64 = 120.0;
/// How often the relay is asked whether a phone has paired while the code is up.
pub const PAIR_POLL_S: f64 = 4.0;
/// MapleLens checks in this often otherwise, so a phone that left on its own is noticed.
pub const CHECK_IN_S: f64 = 60.0;

/// The relay's routes MapleLens uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// `/state/ping`: the phone count.
    Ping,
    /// `/notify`: a test message.
    Test,
    /// `/unpair` with `remove: true`.
    Remove,
    /// `/state/put`: the sealed store book; answers the phone count like a check-in.
    Upload,
}

impl Route {
    pub fn path(self) -> &'static str {
        match self {
            Self::Ping => "/state/ping",
            Self::Test => "/notify",
            Self::Remove => "/unpair",
            Self::Upload => "/state/put",
        }
    }
}

/// One answer of the relay: its HTTP status (None: it could not be reached) and the phone count it gave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    pub status: Option<u16>,
    pub phones: Option<u32>,
}

impl Reply {
    /// From the status and the body's JSON (`{phones}`).
    pub fn from_answer(status: u16, body: &str) -> Self {
        let phones = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|json| json["phones"].as_u64())
            .map(|n| n.min(u64::from(u32::MAX)) as u32);
        Self { status: Some(status), phones }
    }

    pub const UNREACHABLE: Self = Self { status: None, phones: None };
}

/// The pairing panel over the settings window.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Panel {
    #[default]
    Hidden,
    /// The QR code is up since `since`; `before` phones were paired then (a new one makes more).
    Code { since: f64, before: u32 },
    /// A phone just paired: the code is gone, a test is a click away.
    Connected,
    /// The code hid itself before a phone paired.
    Expired,
}

/// What the app must do after an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// The key is dead (removed here, or elsewhere): forget it, so Connect makes a new one.
    Forget,
}

/// Everything the settings window shows about the phone, besides the key itself (a setting).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pairing {
    /// Phones paired with the key, as last asked; None before the first answer.
    pub phones: Option<u32>,
    pub panel: Panel,
    /// The QR code's modules (`qr_modules`) while the panel may show it.
    pub qr: Option<(usize, Vec<bool>)>,
    /// What the last press or answer came to ("" before any).
    pub note: String,
    /// A test or a removal is on its way.
    pub busy: Option<Route>,
    /// The player said they scanned the code: the next answer says what it found.
    asked: bool,
}

impl Pairing {
    /// The row's state: "Not connected", "2 phones connected"...
    pub fn state_text(&self, has_key: bool) -> String {
        match (has_key, self.phones) {
            (false, _) => "Not connected".into(),
            (true, _) if matches!(self.panel, Panel::Code { .. }) && self.phones.unwrap_or(0) == 0 => {
                "Waiting for a phone".into()
            }
            (true, None) => "Checking…".into(),
            (true, Some(0)) => "No phone yet".into(),
            (true, Some(1)) => "1 phone connected".into(),
            (true, Some(n)) => format!("{n} phones connected"),
        }
    }

    /// The row's hint when nothing was pressed yet.
    pub fn hint(&self, has_key: bool) -> &str {
        match (has_key, self.phones) {
            _ if !self.note.is_empty() => &self.note,
            (false, _) => "Get your sales on your phone: press Connect and scan the code",
            (true, Some(n)) if n > 0 => "Your phone gets your sales. Remove it to connect another",
            _ => "No phone has scanned the code yet: press Connect to show it again",
        }
    }

    /// Whether the phone count is worth asking often: the code is up and a phone may be pairing.
    pub fn fast(&self) -> bool {
        matches!(self.panel, Panel::Code { .. })
    }

    /// Seconds before the code hides itself, while it is up.
    pub fn seconds_left(&self, now: f64) -> Option<u32> {
        match self.panel {
            Panel::Code { since, .. } => Some((QR_SHOWN_S - (now - since)).clamp(0.0, QR_SHOWN_S).ceil() as u32),
            _ => None,
        }
    }

    /// Shows the code of `key` (Connect, Add a phone, Show it again).
    pub fn show_code(&mut self, key: &str, now: f64) {
        self.qr = qr_modules(&phone_url(key));
        self.panel = Panel::Code { since: now, before: self.phones.unwrap_or(0) };
        self.note.clear();
        self.asked = false;
    }

    /// "I've scanned it": the app asks the relay at once.
    pub fn checking(&mut self) {
        self.asked = true;
        self.note = "Checking…".into();
    }

    /// Closes the panel; the code goes with it.
    pub fn hide_panel(&mut self) {
        self.panel = Panel::Hidden;
        self.qr = None;
    }

    /// Time passes: the code hides itself after `QR_SHOWN_S`. True when the picture changed.
    pub fn tick(&mut self, now: f64) -> bool {
        match self.panel {
            Panel::Code { since, .. } if now - since >= QR_SHOWN_S => {
                self.panel = Panel::Expired;
                self.qr = None;
                true
            }
            _ => false,
        }
    }

    /// The key was removed or forgotten: back to the start, saying why.
    fn forget(&mut self, note: &str) -> Effect {
        *self = Self { note: note.into(), ..Self::default() };
        Effect::Forget
    }

    /// The relay's answer to a check-in.
    pub fn on_ping(&mut self, reply: Reply) -> Effect {
        match reply.status {
            Some(410) => return self.forget("This pairing was removed elsewhere: press Connect to pair a phone again"),
            // A pairing no phone has joined yet does not exist at the relay: no phones.
            Some(200 | 404) => self.set_phones(reply.phones.unwrap_or(0)),
            // A failed check-in only matters while the player waits on a phone; the next one may get through.
            Some(429) if self.fast() => self.note = "The relay asks to slow down: checking again shortly".into(),
            Some(status) if self.fast() => self.note = format!("The relay answered HTTP {status}: still trying"),
            None if self.fast() => self.note = "Can't reach MapleLens's relay: check your connection".into(),
            _ => {}
        }
        Effect::None
    }

    fn set_phones(&mut self, count: u32) {
        let known = self.phones.unwrap_or(0);
        if let Panel::Code { before, .. } = self.panel
            && count > before
        {
            self.panel = Panel::Connected;
            self.qr = None;
            self.note = "Phone connected".into();
        } else if std::mem::take(&mut self.asked) && self.fast() {
            self.note = "No phone yet: on the phone, open the link and tap Turn on notifications".into();
        } else if known > 0 && count < known {
            self.note = match count {
                0 => "Your phone disconnected: press Connect to pair one again".into(),
                _ => "A phone disconnected".into(),
            };
        } else if ["Can't reach", "The relay", "Checking"].iter().any(|start| self.note.starts_with(start)) {
            self.note.clear();
        }
        self.phones = Some(count);
    }

    /// Test was pressed.
    pub fn testing(&mut self) {
        self.busy = Some(Route::Test);
        self.note = "Sending a test notification…".into();
    }

    /// The relay's answer to the test message.
    pub fn on_test(&mut self, reply: Reply) -> Effect {
        self.busy = None;
        self.note = match reply.status {
            Some(200..=204) => "Sent: check your phone".into(),
            Some(410) => return self.on_ping(reply),
            Some(404) => {
                self.phones = Some(0);
                "No phone is paired yet: press Connect and scan the code with your phone".into()
            }
            Some(429) => "Too many tries: wait a minute, then press Test again".into(),
            Some(status) => format!("Not sent: {}", refusal("Phone", status)),
            None => "Couldn't reach MapleLens's relay: check your connection and press Test again".into(),
        };
        Effect::None
    }

    /// Remove was pressed.
    pub fn removing(&mut self) {
        self.hide_panel();
        self.busy = Some(Route::Remove);
        self.note = "Removing…".into();
    }

    /// The relay's answer to Remove: the key is forgotten only once the relay has dropped it (or never had it), else
    /// it would keep working for whoever holds it.
    pub fn on_remove(&mut self, reply: Reply) -> Effect {
        self.busy = None;
        match reply.status {
            Some(200 | 404 | 410) => self.forget("Removed: your phones were told and disconnected"),
            Some(429) => {
                self.note = "Too many tries: wait a minute and press Remove again".into();
                Effect::None
            }
            Some(status) => {
                self.note = format!("The relay answered HTTP {status}: press Remove again");
                Effect::None
            }
            None => {
                self.note = "Couldn't reach the relay: press Remove again".into();
                Effect::None
            }
        }
    }

    /// The answer to `route`.
    pub fn on_reply(&mut self, route: Route, reply: Reply) -> Effect {
        match route {
            Route::Ping | Route::Upload => self.on_ping(reply),
            Route::Test => self.on_test(reply),
            Route::Remove => self.on_remove(reply),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";

    fn ok(phones: u32) -> Reply {
        Reply { status: Some(200), phones: Some(phones) }
    }

    fn status(code: u16) -> Reply {
        Reply { status: Some(code), phones: None }
    }

    #[test]
    fn replies_are_read() {
        assert_eq!(Reply::from_answer(200, r#"{"phones":2}"#), ok(2));
        assert_eq!(Reply::from_answer(429, r#"{"retry_after":3}"#), status(429));
        assert_eq!(Reply::from_answer(502, "<html>"), status(502));
    }

    #[test]
    fn the_code_closes_once_a_phone_pairs() {
        let mut pairing = Pairing::default();
        pairing.show_code(KEY, 10.0);
        assert!(pairing.fast() && pairing.qr.is_some());
        assert_eq!(pairing.state_text(true), "Waiting for a phone");
        assert_eq!(pairing.on_ping(status(404)), Effect::None);
        assert!(pairing.fast(), "no phone yet");
        pairing.on_ping(ok(1));
        assert_eq!(pairing.panel, Panel::Connected);
        assert_eq!(pairing.qr, None, "the code is gone once a phone has it");
        assert_eq!(pairing.state_text(true), "1 phone connected");
        assert_eq!(pairing.note, "Phone connected");
    }

    #[test]
    fn adding_a_phone_waits_for_one_more() {
        let mut pairing = Pairing { phones: Some(2), ..Pairing::default() };
        pairing.show_code(KEY, 0.0);
        assert_eq!(pairing.state_text(true), "2 phones connected");
        pairing.on_ping(ok(2));
        assert!(pairing.fast());
        pairing.on_ping(ok(3));
        assert_eq!(pairing.panel, Panel::Connected);
    }

    #[test]
    fn the_code_hides_itself_after_two_minutes() {
        let mut pairing = Pairing::default();
        pairing.show_code(KEY, 100.0);
        assert_eq!(pairing.seconds_left(100.2), Some(120));
        assert_eq!(pairing.seconds_left(219.5), Some(1));
        assert!(!pairing.tick(219.0));
        assert!(pairing.tick(220.0));
        assert_eq!((pairing.panel.clone(), pairing.qr.clone()), (Panel::Expired, None));
        assert!(!pairing.fast());
        pairing.show_code(KEY, 300.0);
        assert!(pairing.qr.is_some(), "Show it again brings the same key's code back");
    }

    #[test]
    fn a_removed_key_is_forgotten_from_any_route() {
        for route in [Route::Ping, Route::Test, Route::Remove, Route::Upload] {
            let mut pairing = Pairing { phones: Some(1), ..Pairing::default() };
            pairing.show_code(KEY, 0.0);
            assert_eq!(pairing.on_reply(route, status(410)), Effect::Forget, "{route:?}");
            assert_eq!(pairing.phones, None);
            assert_eq!(pairing.panel, Panel::Hidden);
            assert!(!pairing.note.is_empty());
        }
    }

    #[test]
    fn remove_forgets_the_key_only_once_the_relay_dropped_it() {
        for (reply, forgets) in [
            (ok(0), true),
            (status(404), true),
            (status(410), true),
            (status(429), false),
            (status(400), false),
            (status(500), false),
            (Reply::UNREACHABLE, false),
        ] {
            let mut pairing = Pairing { phones: Some(1), ..Pairing::default() };
            pairing.removing();
            assert_eq!(pairing.busy, Some(Route::Remove));
            let effect = pairing.on_remove(reply);
            assert_eq!(effect == Effect::Forget, forgets, "{reply:?}");
            assert_eq!(pairing.busy, None);
            assert!(forgets || pairing.note.contains("press Remove again"), "{}", pairing.note);
        }
    }

    #[test]
    fn every_test_answer_says_what_to_do() {
        let cases = [
            (ok(1), "Sent: check your phone"),
            (status(404), "No phone is paired yet: press Connect and scan the code with your phone"),
            (status(429), "Too many tries: wait a minute, then press Test again"),
            (status(502), "Not sent: The phones' push service did not answer: try again in a moment"),
            (status(400), "Not sent: Phone answered HTTP 400"),
            (Reply::UNREACHABLE, "Couldn't reach MapleLens's relay: check your connection and press Test again"),
        ];
        for (reply, text) in cases {
            let mut pairing = Pairing { phones: Some(1), ..Pairing::default() };
            pairing.testing();
            assert_eq!(pairing.on_test(reply), Effect::None);
            assert_eq!(pairing.note, text);
            assert_eq!(pairing.busy, None);
        }
    }

    #[test]
    fn a_phone_leaving_is_said() {
        let mut pairing = Pairing { phones: Some(2), ..Pairing::default() };
        pairing.on_ping(ok(1));
        assert_eq!(pairing.note, "A phone disconnected");
        pairing.on_ping(ok(0));
        assert_eq!(pairing.note, "Your phone disconnected: press Connect to pair one again");
        assert_eq!(pairing.state_text(true), "No phone yet");
    }

    #[test]
    fn failed_check_ins_are_said_only_while_waiting() {
        let mut idle = Pairing { phones: Some(1), ..Pairing::default() };
        idle.on_ping(Reply::UNREACHABLE);
        assert_eq!((idle.note.as_str(), idle.phones), ("", Some(1)));
        let mut waiting = Pairing::default();
        waiting.show_code(KEY, 0.0);
        waiting.on_ping(Reply::UNREACHABLE);
        assert!(waiting.note.starts_with("Can't reach"));
        waiting.on_ping(ok(0));
        assert_eq!(waiting.note, "", "the next answer clears it");
        waiting.on_ping(status(429));
        assert!(waiting.note.contains("slow down"));
        waiting.checking();
        assert_eq!(waiting.note, "Checking…");
        waiting.on_ping(ok(0));
        assert!(waiting.note.starts_with("No phone yet: on the phone"), "{}", waiting.note);
        waiting.on_ping(ok(0));
        assert!(waiting.note.starts_with("No phone yet"), "kept until something new is said");
    }

    #[test]
    fn states_read_well() {
        let pairing = Pairing::default();
        assert_eq!(pairing.state_text(false), "Not connected");
        assert_eq!(pairing.state_text(true), "Checking…");
        assert_eq!(Pairing { phones: Some(3), ..Pairing::default() }.state_text(true), "3 phones connected");
        assert!(pairing.hint(false).contains("Connect"));
    }
}
