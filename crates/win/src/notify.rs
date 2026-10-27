//! Sends notifications (`maplelens_core::notify`) on a thread of its own: the shop watcher must never wait on the
//! network, and a service that is slow or down must never lose the others' messages.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use maplelens_core::notify::{Connector, Notification, NotifySettings, Request, connectors};

use crate::{net, paths};

/// Sales that come together are one message: the shop shows a burst when a player buys several things, and a
/// webhook takes about five messages in two seconds.
const BATCH_WINDOW: Duration = Duration::from_millis(1500);
/// Requests to one service are spaced this far apart.
const MIN_GAP: Duration = Duration::from_millis(600);
/// A request refused for going too fast is retried this many times, after the wait the service asks for.
const RETRIES: u32 = 3;
const MAX_WAIT_S: f64 = 30.0;
const TIMEOUT_S: u64 = 10;

enum Job {
    Settings(NotifySettings),
    Sales(Vec<Notification>),
    Test(NotifySettings, Sender<String>),
}

/// Handle to the sending thread; cheap to clone.
#[derive(Clone)]
pub struct Notifier {
    jobs: Sender<Job>,
}

impl Notifier {
    pub fn spawn(settings: NotifySettings) -> Self {
        let (jobs, inbox) = channel();
        let _ = jobs.send(Job::Settings(settings));
        let spawned = std::thread::Builder::new().name("notifier".into()).spawn(move || run(inbox));
        if let Err(error) = spawned {
            crate::warn!("notifications unavailable: {error}");
        }
        Self { jobs }
    }

    /// The player changed the settings.
    pub fn configure(&self, settings: NotifySettings) {
        let _ = self.jobs.send(Job::Settings(settings));
    }

    /// Things that happened (sold items); they go out after a moment, together with any that follow.
    pub fn send(&self, notifications: Vec<Notification>) {
        if !notifications.is_empty() {
            let _ = self.jobs.send(Job::Sales(notifications));
        }
    }

    /// Sends a test message with `settings` (not yet saved) and reports how it went, in words for the player.
    pub fn test(&self, settings: NotifySettings) -> Receiver<String> {
        let (answer, result) = channel();
        let _ = self.jobs.send(Job::Test(settings, answer));
        result
    }
}

fn run(inbox: Receiver<Job>) {
    let mut settings = NotifySettings::default();
    let mut last_sent: Option<Instant> = None;
    // Whose store the last notification was about: the test message names it too.
    let mut owner: Option<String> = None;
    while let Ok(job) = inbox.recv() {
        match job {
            Job::Settings(new) => settings = new,
            Job::Test(test, answer) => {
                let _ = answer.send(test_message(&test, owner.as_deref()));
            }
            Job::Sales(mut batch) => {
                owner = batch.iter().rev().find_map(whose).or(owner);
                // More sales in the next moments join this message.
                let deadline = Instant::now() + BATCH_WINDOW;
                while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                    match inbox.recv_timeout(left) {
                        Ok(Job::Sales(more)) => batch.extend(more),
                        Ok(Job::Settings(new)) => settings = new,
                        Ok(Job::Test(test, answer)) => {
                            let _ = answer.send(test_message(&test, owner.as_deref()));
                        }
                        Err(_) => break,
                    }
                }
                batch.retain(|n| settings.allows(n));
                if batch.is_empty() {
                    continue;
                }
                for connector in connectors(&settings) {
                    for request in connector.requests(&batch, paths::now_unix()) {
                        if let Some(since) = last_sent.and_then(|at| MIN_GAP.checked_sub(at.elapsed())) {
                            std::thread::sleep(since);
                        }
                        match deliver(connector.as_ref(), &request) {
                            Ok(()) => crate::debug!("{}: sent {} notification(s)", connector.name(), batch.len()),
                            Err(error) => crate::warn!("{}: {error}", connector.name()),
                        }
                        last_sent = Some(Instant::now());
                    }
                }
            }
        }
    }
}

/// Sends the test message and says how it went.
/// The store a notification is about, when it says.
fn whose(notification: &Notification) -> Option<String> {
    match notification {
        Notification::Sale(sale) => sale.store.clone(),
        Notification::Store(change) => change.owner.clone(),
    }
}

fn test_message(settings: &NotifySettings, owner: Option<&str>) -> String {
    let answers: Vec<String> = connectors(settings)
        .iter()
        .map(|connector| match deliver(connector.as_ref(), &connector.test_request(owner, paths::now_unix())) {
            Ok(()) => connector.sent().to_string(),
            Err(error) => format!("Not sent: {error}"),
        })
        .collect();
    if answers.is_empty() { "Paste a Discord webhook URL first".to_string() } else { answers.join(" · ") }
}

/// POST `request`, waiting out the service's rate limit; the error is in words for the player (never the URL: its
/// token is a secret).
fn deliver(connector: &dyn Connector, request: &Request) -> Result<(), String> {
    for attempt in 0..=RETRIES {
        let response = net::agent(TIMEOUT_S)
            .post(&request.url)
            .header("Content-Type", "application/json")
            .send(request.body.as_str());
        let mut response =
            response.map_err(|e| format!("{} could not be reached ({})", connector.name(), short(&e.to_string())))?;
        match response.status().as_u16() {
            200..=204 => return Ok(()),
            429 if attempt < RETRIES => {
                let wait = response
                    .body_mut()
                    .read_to_string()
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                    .and_then(|v| v["retry_after"].as_f64())
                    .unwrap_or(1.0)
                    .clamp(0.1, MAX_WAIT_S);
                std::thread::sleep(Duration::from_secs_f64(wait));
            }
            401 | 404 | 410 => return Err(format!("{} {}", connector.name(), connector.not_found())),
            status => return Err(format!("{} answered HTTP {status}", connector.name())),
        }
    }
    Err(format!("{} keeps refusing for going too fast", connector.name()))
}

/// An error text without anything that could be the URL.
fn short(error: &str) -> String {
    error.split("https://").next().unwrap_or("").trim().trim_end_matches(':').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplelens_core::notify::Sale;

    /// Sends a test message and three sample sales (one message: they are batched) to the webhook in
    /// `MAPLELENS_DISCORD_WEBHOOK`: `cargo test -p maplelens live_discord -- --ignored --nocapture`.
    #[test]
    #[ignore = "sends real messages to the webhook in MAPLELENS_DISCORD_WEBHOOK (only the test one with MAPLELENS_DISCORD_TEST_ONLY)"]
    fn live_discord() {
        let Ok(url) = std::env::var("MAPLELENS_DISCORD_WEBHOOK") else { return };
        let settings = NotifySettings { discord_webhook: url, sales: true, store: true, ..Default::default() };
        let notifier = Notifier::spawn(settings.clone());
        let answer = notifier.test(settings).recv_timeout(Duration::from_secs(30)).expect("an answer");
        println!("test message: {answer}");
        assert!(answer.starts_with("Sent"), "{answer}");
        if std::env::var_os("MAPLELENS_DISCORD_TEST_ONLY").is_some() {
            return;
        }
        let sale = |item: &str, buyer: &str, quantity, mesos, item_id, balance| {
            Notification::Sale(Sale {
                item: item.into(),
                buyer: buyer.into(),
                quantity,
                mesos,
                item_id,
                store: Some("ShopOwner".into()),
                balance,
            })
        };
        notifier.send(vec![
            sale("Pet Food", "Tikko", 20, Some(1_200), Some(2_120_000), None),
            sale("Ribboned Pig Headband", "Frase", 1, Some(35_000), Some(1_002_140), None),
            sale("Arrows for Bows", "TranzMog", 1000, Some(250_000), Some(2_060_000), Some(1_839_311)),
        ]);
        // The sender batches for a moment, then sends: wait for it.
        std::thread::sleep(Duration::from_secs(6));
    }

    #[test]
    fn a_bad_webhook_is_not_sent_anywhere() {
        let notifier = Notifier::spawn(NotifySettings::default());
        let settings = NotifySettings {
            discord_webhook: "https://example.com/api/webhooks/1/x".into(),
            sales: true,
            store: true,
            ..Default::default()
        };
        let answer = notifier.test(settings).recv_timeout(Duration::from_secs(5)).expect("an answer");
        assert_eq!(answer, "Paste a Discord webhook URL first");
    }
}
