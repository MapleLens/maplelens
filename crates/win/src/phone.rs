//! The phone pairing's requests to MapleLens's relay (`relay/push`) on a thread of its own: the UI thread never waits
//! on the network. It checks in for the phone count every few seconds while the pairing code is up and once a minute
//! otherwise, and sends Test and Remove when asked; what the answers mean is `notify::pairing`'s. The key is the only
//! credential, so it is never logged.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use maplelens_core::notify::Connector;
use maplelens_core::notify::pairing::{CHECK_IN_S, PAIR_POLL_S, Reply, Route};
use maplelens_core::notify::phone::{Phone, RELAY_URL};
use serde_json::json;

use crate::{net, paths};

const TIMEOUT_S: u64 = 10;
/// The relay's key: a 96-bit id to find the pairing and a 128-bit secret only the paired devices know
/// (`newKey` in docs/app/phone/relay.js).
const ID_BYTES: usize = 12;
const SECRET_BYTES: usize = 16;

/// A new pairing key, `<id>.<secret>` in base64url; None when Windows has no randomness to give.
pub fn new_key() -> Option<String> {
    let mut bytes = [0u8; ID_BYTES + SECRET_BYTES];
    getrandom::fill(&mut bytes).ok()?;
    let (id, secret) = bytes.split_at(ID_BYTES);
    Some(format!("{}.{}", URL_SAFE_NO_PAD.encode(id), URL_SAFE_NO_PAD.encode(secret)))
}

enum Job {
    Key(String),
    Fast(bool),
    Ping,
    Test,
    Remove,
}

/// The relay's answer to one request, for the key it was made with (the player may have removed it meanwhile).
#[derive(Clone, Debug)]
pub struct Answer {
    pub key: String,
    pub route: Route,
    pub reply: Reply,
}

/// Handle to the thread.
pub struct PhoneLink {
    jobs: Sender<Job>,
    answers: Receiver<Answer>,
    fast: bool,
}

impl PhoneLink {
    pub fn spawn(key: String) -> Self {
        let (jobs, inbox) = channel();
        let (out, answers) = channel();
        let _ = jobs.send(Job::Key(key));
        let spawned = std::thread::Builder::new().name("phone".into()).spawn(move || run(&inbox, &out));
        if let Err(error) = spawned {
            crate::warn!("phone pairing unavailable: {error}");
        }
        Self { jobs, answers, fast: false }
    }

    /// The key changed (made, or forgotten: empty); asks for its phone count at once.
    pub fn set_key(&self, key: &str) {
        let _ = self.jobs.send(Job::Key(key.to_string()));
    }

    /// Ask often (the code is up) or once a minute.
    pub fn set_fast(&mut self, fast: bool) {
        if fast != self.fast {
            self.fast = fast;
            let _ = self.jobs.send(Job::Fast(fast));
        }
    }

    pub fn ping_now(&self) {
        let _ = self.jobs.send(Job::Ping);
    }

    pub fn test(&self) {
        let _ = self.jobs.send(Job::Test);
    }

    pub fn remove(&self) {
        let _ = self.jobs.send(Job::Remove);
    }

    pub fn try_answer(&self) -> Option<Answer> {
        self.answers.try_recv().ok()
    }
}

fn run(inbox: &Receiver<Job>, out: &Sender<Answer>) {
    let mut key = String::new();
    let mut fast = false;
    // When the next check-in is due; None without a key.
    let mut due: Option<Instant> = None;
    loop {
        let job = match due {
            Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let route = match job {
            Ok(Job::Key(new)) => {
                key = new;
                due = (!key.is_empty()).then(Instant::now);
                continue;
            }
            Ok(Job::Fast(now_fast)) => {
                fast = now_fast;
                due = (!key.is_empty()).then(Instant::now);
                continue;
            }
            Ok(Job::Ping) | Err(RecvTimeoutError::Timeout) => Route::Ping,
            Ok(Job::Test) => Route::Test,
            Ok(Job::Remove) => Route::Remove,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if key.is_empty() {
            continue;
        }
        let reply = send(&key, route);
        let gap = if fast { PAIR_POLL_S } else { CHECK_IN_S };
        due = Some(Instant::now() + Duration::from_secs_f64(gap));
        if out.send(Answer { key: key.clone(), route, reply }).is_err() {
            return;
        }
    }
}

/// One request to the relay. The Windows app sends no Origin header, which the relay accepts from apps.
fn send(key: &str, route: Route) -> Reply {
    let body = match route {
        Route::Ping => json!({ "key": key }).to_string(),
        Route::Remove => json!({ "key": key, "remove": true }).to_string(),
        Route::Test => match Phone::new(key) {
            Some(phone) => phone.test_request(None, paths::now_unix()).body,
            None => return Reply { status: Some(400), phones: None },
        },
    };
    let url = format!("{RELAY_URL}{}", route.path());
    let response = net::agent(TIMEOUT_S).post(&url).header("Content-Type", "application/json").send(body.as_str());
    match response {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let text = response.body_mut().read_to_string().unwrap_or_default();
            if status != 200 {
                crate::info!("phone: the relay answered {} with HTTP {status}", route.path());
            }
            Reply::from_answer(status, &text)
        }
        Err(error) => {
            crate::warn!("phone: the relay could not be reached ({error})");
            Reply::UNREACHABLE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplelens_core::notify::phone::valid_key;

    #[test]
    fn new_keys_have_the_relays_shape_and_differ() {
        let (a, b) = (new_key().unwrap(), new_key().unwrap());
        assert_ne!(a, b);
        for key in [&a, &b] {
            assert_eq!(valid_key(key), Some(key.as_str()));
            let (id, secret) = key.split_once('.').unwrap();
            // 96 and 128 bits in base64url without padding, as relay.js makes them.
            assert_eq!((id.len(), secret.len()), (16, 22));
        }
    }

    /// A fresh key through the real relay: no phone yet, a test finds none, Remove drops it and leaves it dead (410):
    /// `cargo test -p maplelens live_phone_relay -- --ignored --nocapture`.
    #[test]
    #[ignore = "talks to the real relay at push.maple.observer"]
    fn live_phone_relay() {
        let key = new_key().unwrap();
        let ask = |route| {
            let reply = send(&key, route);
            println!("{route:?}: {reply:?}");
            reply
        };
        let ping = ask(Route::Ping);
        assert!(matches!(ping.status, Some(200 | 404)) && ping.phones.unwrap_or(0) == 0, "{ping:?}");
        assert_eq!(ask(Route::Test).status, Some(404), "no phone to send to");
        assert!(matches!(ask(Route::Remove).status, Some(200 | 404)));
        assert_eq!(ask(Route::Ping).status, Some(410), "a removed key is dead");
    }
}
