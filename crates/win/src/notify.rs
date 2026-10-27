//! Sends notifications (`maplelens_core::notify`) on a thread of its own: the shop watcher must never wait on the
//! network, and a service that is slow or down must never lose the others' messages.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use maplelens_core::notify::picture::{self, Pictures};
use maplelens_core::notify::{
    Connector, Notification, NotifySettings, Request, Sale, connectors, item_icon_url, refusal, retry_after, telegram,
};

use crate::phone::SharedBook;
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
    Book(SharedBook),
    Sales(Vec<Notification>),
    Test(Service, NotifySettings, Sender<TestAnswer>),
}

/// Which connector a test is for: each has its own row and Test button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Discord,
    Telegram,
}

/// How a test went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestAnswer {
    pub service: Service,
    /// In words for the player.
    pub text: String,
    /// The chat the bot was found to be talking to, for the settings to keep.
    pub telegram_chat: Option<i64>,
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

    /// Sold items' pictures, once found, also go to the phones' store book.
    pub fn share_book(&self, book: SharedBook) {
        let _ = self.jobs.send(Job::Book(book));
    }

    /// Things that happened (sold items); they go out after a moment, together with any that follow.
    pub fn send(&self, notifications: Vec<Notification>) {
        if !notifications.is_empty() {
            let _ = self.jobs.send(Job::Sales(notifications));
        }
    }

    /// Sends a test message to `service` with `settings` (not yet saved) and reports how it went. A Telegram bot
    /// whose chat is not known yet is first asked who wrote to it.
    pub fn test(&self, service: Service, settings: NotifySettings) -> Receiver<TestAnswer> {
        let (answer, result) = channel();
        let _ = self.jobs.send(Job::Test(service, settings, answer));
        result
    }
}

fn run(inbox: Receiver<Job>) {
    let mut settings = NotifySettings::default();
    let mut last_sent: Option<Instant> = None;
    // Whose store the last notification was about: the test message names it too.
    let mut owner: Option<String> = None;
    let mut pictures = Pictures::default();
    let mut book: Option<SharedBook> = None;
    while let Ok(job) = inbox.recv() {
        match job {
            Job::Settings(new) => settings = new,
            Job::Book(shared) => book = Some(shared),
            Job::Test(service, test, answer) => {
                let _ = answer.send(test_message(service, test, owner.as_deref()));
            }
            Job::Sales(mut batch) => {
                owner = batch.iter().rev().find_map(whose).or(owner);
                // More sales in the next moments join this message.
                let deadline = Instant::now() + BATCH_WINDOW;
                while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                    match inbox.recv_timeout(left) {
                        Ok(Job::Sales(more)) => batch.extend(more),
                        Ok(Job::Settings(new)) => settings = new,
                        Ok(Job::Book(shared)) => book = Some(shared),
                        Ok(Job::Test(service, test, answer)) => {
                            let _ = answer.send(test_message(service, test, owner.as_deref()));
                        }
                        Err(_) => break,
                    }
                }
                batch.retain(|n| settings.allows(n));
                if batch.is_empty() {
                    continue;
                }
                find_pictures(&mut pictures, &batch);
                pictures.fill(&mut batch);
                if let Some(book) = &book {
                    picture_book(book, &batch);
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

/// Looks up by name the pictures of sold items without the game's id (those with one are its picture): each name once
/// a run, a failed lookup tried again next sale.
fn find_pictures(pictures: &mut Pictures, batch: &[Notification]) {
    for name in pictures.wanted(batch) {
        let response = net::agent(TIMEOUT_S).get(picture::search_url(&name)).call();
        match response {
            Ok(mut response) if response.status() == 200 => match response.body_mut().read_to_string() {
                Ok(answer) => pictures.learn(&name, &answer),
                Err(error) => crate::warn!("picture of {name:?}: {error}"),
            },
            Ok(response) => crate::warn!("picture of {name:?}: maplestory.io answered HTTP {}", response.status()),
            Err(error) => crate::warn!("picture of {name:?}: {error}"),
        }
    }
}

/// The sold items' pictures into the store book, so the phone shows them beside their sales.
fn picture_book(book: &SharedBook, batch: &[Notification]) {
    let mut book = book.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = book.changed;
    book.changed = false;
    for notification in batch {
        if let Notification::Sale(Sale { item, picture: Some(id), .. }) = notification {
            book.set_picture(item, &item_icon_url(*id));
        }
    }
    if book.changed {
        crate::phone::save_book(&book);
    }
    book.changed |= before;
}

/// The store a notification is about, when it says.
fn whose(notification: &Notification) -> Option<String> {
    match notification {
        Notification::Sale(sale) => sale.store.clone(),
        Notification::Store(change) => change.owner.clone(),
    }
}

/// Sends the test message to `service` alone and says how it went.
fn test_message(service: Service, mut settings: NotifySettings, owner: Option<&str>) -> TestAnswer {
    let mut answer = TestAnswer { service, text: String::new(), telegram_chat: None };
    let (missing, place) = match service {
        Service::Discord => {
            (settings.telegram_bot, settings.telegram_chat) = (String::new(), 0);
            ("Paste a Discord webhook URL first", "channel")
        }
        Service::Telegram => {
            settings.discord_webhook.clear();
            if settings.telegram_chat == 0 && telegram::valid_token(&settings.telegram_bot).is_some() {
                match find_chat(&settings.telegram_bot) {
                    Ok(Some(chat)) => (settings.telegram_chat, answer.telegram_chat) = (chat, Some(chat)),
                    Ok(None) => {
                        answer.text = "No message to the bot yet: send it /start in Telegram, then Test again".into();
                        return answer;
                    }
                    Err(error) => {
                        answer.text = format!("Not linked: {error}");
                        return answer;
                    }
                }
            }
            ("Paste your bot's token first", "chat")
        }
    };
    answer.text = match connectors(&settings).first() {
        Some(connector) => match deliver(connector.as_ref(), &connector.test_request(owner, paths::now_unix())) {
            Ok(()) => format!("Sent: check your {} {place}", connector.name()),
            Err(error) => format!("Not sent: {error}"),
        },
        None => missing.to_string(),
    };
    answer
}

/// The chat that last wrote to the bot, if any did lately.
fn find_chat(token: &str) -> Result<Option<i64>, String> {
    let Some(request) = telegram::link_request(token) else { return Ok(None) };
    match post("Telegram", &request)? {
        (200, answer) => Ok(telegram::chat_from_updates(&answer)),
        (status, _) => Err(refusal("Telegram", status)),
    }
}

/// POST `request`: the status and the answer's text. The error is in words for the player (never the URL: its token
/// is a secret).
fn post(service: &str, request: &Request) -> Result<(u16, String), String> {
    let response =
        net::agent(TIMEOUT_S).post(&request.url).header("Content-Type", "application/json").send(request.body.as_str());
    let mut response = response.map_err(|e| format!("{service} could not be reached ({})", short(&e.to_string())))?;
    let status = response.status().as_u16();
    Ok((status, response.body_mut().read_to_string().unwrap_or_default()))
}

/// POST `request`, or its fallback if the service refuses it.
fn deliver(connector: &dyn Connector, request: &Request) -> Result<(), String> {
    let refused = match deliver_one(connector, request) {
        Err(refused) if request.fallback.is_some() => refused,
        sent => return sent,
    };
    crate::warn!("{}: {refused}; sending it without the picture", connector.name());
    request.fallback.as_deref().map_or(Ok(()), |fallback| deliver_one(connector, fallback))
}

/// POST `request`, waiting out the service's rate limit.
fn deliver_one(connector: &dyn Connector, request: &Request) -> Result<(), String> {
    for attempt in 0..=RETRIES {
        match post(connector.name(), request)? {
            (200..=204, _) => return Ok(()),
            (429, answer) if attempt < RETRIES => {
                let wait = retry_after(&answer).unwrap_or(1.0).clamp(0.1, MAX_WAIT_S);
                std::thread::sleep(Duration::from_secs_f64(wait));
            }
            (status, _) => return Err(refusal(connector.name(), status)),
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
        let settings = NotifySettings { discord_webhook: url, sales: true, store: true, ..NotifySettings::default() };
        let notifier = Notifier::spawn(settings.clone());
        let answer =
            notifier.test(Service::Discord, settings).recv_timeout(Duration::from_secs(30)).expect("an answer");
        println!("test message: {}", answer.text);
        assert!(answer.text.starts_with("Sent"), "{}", answer.text);
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
                ..Sale::default()
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

    /// Links the bot in `MAPLELENS_TELEGRAM_BOT` to whoever wrote to it last (send it /start first) and sends a test
    /// message and two sample sales there: `cargo test -p maplelens live_telegram -- --ignored --nocapture`.
    #[test]
    #[ignore = "sends real messages through the bot in MAPLELENS_TELEGRAM_BOT"]
    fn live_telegram() {
        let Ok(token) = std::env::var("MAPLELENS_TELEGRAM_BOT") else { return };
        let notifier = Notifier::spawn(NotifySettings::default());
        let unlinked = NotifySettings { telegram_bot: token, sales: true, store: true, ..NotifySettings::default() };
        let answer = notifier.test(Service::Telegram, unlinked.clone()).recv_timeout(Duration::from_secs(30)).unwrap();
        println!("test message: {} (chat {:?})", answer.text, answer.telegram_chat);
        assert!(answer.text.starts_with("Sent"), "{}", answer.text);
        let linked = NotifySettings { telegram_chat: answer.telegram_chat.unwrap(), ..unlinked };
        notifier.configure(linked);
        let sale = |item: &str, buyer: &str, quantity, mesos, balance| {
            Notification::Sale(Sale {
                item: item.into(),
                buyer: buyer.into(),
                quantity,
                mesos,
                store: Some("ShopOwner".into()),
                balance,
                ..Sale::default()
            })
        };
        notifier.send(vec![
            sale("Pet Food", "Tikko", 20, Some(1_200), None),
            sale("Arrows for Bows", "TranzMog", 1000, Some(250_000), Some(1_839_311)),
        ]);
        std::thread::sleep(Duration::from_secs(6));
    }

    #[test]
    fn a_bad_webhook_or_token_is_not_sent_anywhere() {
        let notifier = Notifier::spawn(NotifySettings::default());
        let settings = NotifySettings {
            discord_webhook: "https://example.com/api/webhooks/1/x".into(),
            telegram_bot: "123:short".into(),
            telegram_chat: 111,
            phone_key: "a.b".into(),
            sales: true,
            store: true,
        };
        let answer = |service| notifier.test(service, settings.clone()).recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(answer(Service::Discord).text, "Paste a Discord webhook URL first");
        assert_eq!(answer(Service::Telegram).text, "Paste your bot's token first");
    }
}
