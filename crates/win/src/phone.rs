//! The phone pairing's requests to MapleLens's relay (`relay/push`) on a thread of its own: the UI thread never waits
//! on the network. It checks in for the phone count every few seconds while the pairing code is up and once a minute
//! otherwise, uploads the store book (sealed: `seal`) when it changed and a phone is paired, and sends Test and
//! Remove when asked; what the answers mean is `notify::pairing`'s. The key is the only credential, so it is never
//! logged.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use maplelens_core::notify::Connector;
use maplelens_core::notify::pairing::{CHECK_IN_S, PAIR_POLL_S, Reply, Route};
use maplelens_core::notify::phone::{Phone, RELAY_URL};
use maplelens_core::notify::storebook::StoreBook;
use serde_json::json;

use crate::{net, paths};

const TIMEOUT_S: u64 = 10;
/// The relay's key: a 96-bit id to find the pairing and a 128-bit secret only the paired devices know
/// (`newKey` in docs/app/phone/relay.js).
const ID_BYTES: usize = 12;
const SECRET_BYTES: usize = 16;
/// The store book goes to the phones at most this often, and only when it changed (the page's `UPLOAD_GAP_MS`).
const UPLOAD_GAP: Duration = Duration::from_secs(5);

/// The store book, kept by the shop thread and read by this one.
pub type SharedBook = Arc<Mutex<StoreBook>>;

fn book_file() -> std::path::PathBuf {
    paths::data_dir().join("storebook.json")
}

/// The store book as MapleLens last left it (history survives a restart, as the page's does).
pub fn load_book() -> SharedBook {
    let text = std::fs::read_to_string(book_file()).unwrap_or_default();
    Arc::new(Mutex::new(if text.is_empty() { StoreBook::default() } else { StoreBook::from_json(&text) }))
}

pub fn save_book(book: &StoreBook) {
    if let Err(error) = paths::write_atomic(&book_file(), book.to_json().as_bytes()) {
        crate::warn!("could not save the store book: {error}");
    }
}

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
    pub fn spawn(key: String, book: SharedBook) -> Self {
        let (jobs, inbox) = channel();
        let (out, answers) = channel();
        let _ = jobs.send(Job::Key(key));
        let spawned = std::thread::Builder::new().name("phone".into()).spawn(move || run(&inbox, &out, &book));
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

fn run(inbox: &Receiver<Job>, out: &Sender<Answer>, book: &SharedBook) {
    let mut key = String::new();
    let mut fast = false;
    // When the next check-in is due; None without a key.
    let mut due: Option<Instant> = None;
    // Phones paired, as last answered: the book goes up only while there is one to read it.
    let mut phones = 0;
    let mut last_upload: Option<Instant> = None;
    loop {
        // While a phone is paired, look every few seconds for a changed book.
        let wake = match due {
            Some(at) if phones > 0 => Some(at.min(Instant::now() + UPLOAD_GAP)),
            other => other,
        };
        let job = match wake {
            Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let route = match job {
            Ok(Job::Key(new)) => {
                key = new;
                phones = 0;
                due = (!key.is_empty()).then(Instant::now);
                continue;
            }
            Ok(Job::Fast(now_fast)) => {
                fast = now_fast;
                due = (!key.is_empty()).then(Instant::now);
                continue;
            }
            Ok(Job::Ping) => Route::Ping,
            Err(RecvTimeoutError::Timeout) => {
                let changed = book.lock().unwrap_or_else(PoisonError::into_inner).changed;
                let upload_due = last_upload.is_none_or(|at| at.elapsed() >= UPLOAD_GAP);
                if phones > 0 && changed && upload_due {
                    Route::Upload
                } else if due.is_some_and(|at| at <= Instant::now()) {
                    Route::Ping
                } else {
                    continue;
                }
            }
            Ok(Job::Test) => Route::Test,
            Ok(Job::Remove) => Route::Remove,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if key.is_empty() {
            continue;
        }
        let reply = if route == Route::Upload {
            last_upload = Some(Instant::now());
            upload(&key, book)
        } else {
            send(&key, route, None)
        };
        if matches!(route, Route::Ping | Route::Upload) {
            let count = match reply.status {
                Some(200) => reply.phones.unwrap_or(0),
                Some(404 | 410) => 0,
                _ => phones,
            };
            // A phone that just joined gets the store straight away.
            if count > phones {
                book.lock().unwrap_or_else(PoisonError::into_inner).changed = true;
            }
            phones = count;
        }
        let gap = if fast { PAIR_POLL_S } else { CHECK_IN_S };
        due = Some(Instant::now() + Duration::from_secs_f64(gap));
        if out.send(Answer { key: key.clone(), route, reply }).is_err() {
            return;
        }
    }
}

/// Seals the book and puts it at the relay; it is marked changed again if that failed.
fn upload(key: &str, book: &SharedBook) -> Reply {
    let snapshot = {
        let mut book = book.lock().unwrap_or_else(PoisonError::into_inner);
        book.changed = false;
        // MapleLens for Windows always follows the store: the status says when it cannot see it.
        book.snapshot(true, paths::now_unix() * 1000.0)
    };
    let reply = match crate::seal::seal(key, &snapshot) {
        Some(blob) => send(key, Route::Upload, Some(&blob)),
        None => Reply::UNREACHABLE,
    };
    if reply.status != Some(200) {
        book.lock().unwrap_or_else(PoisonError::into_inner).changed = true;
    }
    reply
}

/// One request to the relay. The Windows app sends no Origin header, which the relay accepts from apps.
fn send(key: &str, route: Route, blob: Option<&str>) -> Reply {
    let body = match route {
        Route::Ping => json!({ "key": key }).to_string(),
        Route::Upload => json!({ "key": key, "blob": blob.unwrap_or_default() }).to_string(),
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
            let reply = send(&key, route, None);
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
