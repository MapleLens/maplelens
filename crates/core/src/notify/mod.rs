//! Notifications: things that happened (for now, an item sold in the player's Store Permit) sent out through
//! connectors. A connector turns a batch of notifications into plain HTTP requests; sending them is the platform's
//! (the Windows app's thread, the browser's `fetch`), so everything here is pure and testable, and a new connector
//! (Telegram, a generic webhook) is one more type implementing `Connector` and one more field in the settings.

pub mod announce;
pub mod discord;

/// What happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notification {
    /// One of the player's items sold.
    Sale(Sale),
    /// The player's store was detected open, or closed.
    Store(StoreChange),
}

/// A store opening (found open) or closing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreChange {
    pub opened: bool,
    /// Whose store it is, when the name was read.
    pub owner: Option<String>,
    /// The meso counter, when it was read.
    pub balance: Option<i64>,
}

/// A sale and what is known around it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sale {
    pub item: String,
    pub buyer: String,
    pub quantity: u64,
    /// What the store received for it (the meso counter's gain), when known.
    pub mesos: Option<i64>,
    /// The game's item id, for the item's picture.
    pub item_id: Option<u64>,
    /// The player whose store it is.
    pub store: Option<String>,
    /// The store's meso counter after it, when known.
    pub balance: Option<i64>,
}

/// A picture of an item (maplestory.io, the same public service the map helper uses; no key).
pub fn item_icon_url(item_id: u64) -> String {
    format!("{}/item/{item_id}/icon", crate::config::MAP_API)
}

/// 1234567 -> "1,234,567".
pub fn grouped(number: i64) -> String {
    let digits: Vec<char> = number.unsigned_abs().to_string().chars().collect();
    // Groups of three from the right.
    let mut groups: Vec<String> = digits.rchunks(3).map(|g| g.iter().collect()).collect();
    groups.reverse();
    let out = groups.join(",");
    if number < 0 { format!("-{out}") } else { out }
}

/// One HTTP POST with a JSON body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub body: String,
}

/// Where notifications can go.
pub trait Connector: Send {
    /// For logs and the settings window ("Discord").
    fn name(&self) -> &'static str;

    /// The requests that deliver `batch` (one message may carry several notifications, so a burst of sales does not
    /// run into the service's rate limit).
    fn requests(&self, batch: &[Notification], unix_now: f64) -> Vec<Request>;

    /// A one-off request to check the connector works, sent when the player asks for a test; it names the player's
    /// store when `owner` is known, like every message.
    fn test_request(&self, owner: Option<&str>, unix_now: f64) -> Request;
}

/// What the player configured: one field per connector, empty or false when off.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotifySettings {
    /// A Discord channel's webhook URL; empty = off.
    pub discord_webhook: String,
    /// Tell about sold items.
    pub sales: bool,
    /// Tell when the store is detected open or closes.
    pub store: bool,
}

impl NotifySettings {
    /// Whether the player asked to hear about this.
    pub fn allows(&self, notification: &Notification) -> bool {
        match notification {
            Notification::Sale(_) => self.sales,
            Notification::Store(_) => self.store,
        }
    }
}

/// The connectors the settings switch on.
pub fn connectors(settings: &NotifySettings) -> Vec<Box<dyn Connector>> {
    let mut found: Vec<Box<dyn Connector>> = Vec::new();
    if let Some(discord) = discord::Discord::new(&settings.discord_webhook) {
        found.push(Box::new(discord));
    }
    found
}

/// `unix` seconds as an ISO-8601 UTC time ("2026-10-25T13:33:31Z"): what Discord's embeds take.
pub fn iso_utc(unix: f64) -> String {
    let seconds = unix.max(0.0) as i64;
    let (year, month, day) = crate::market::time::date_from_days(seconds.div_euclid(86_400));
    let of_day = seconds.rem_euclid(86_400);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", of_day / 3600, of_day / 60 % 60, of_day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_grouped() {
        assert_eq!(
            [grouped(0), grouped(999), grouped(1_000), grouped(39_311), grouped(-1_234_567)],
            ["0", "999", "1,000", "39,311", "-1,234,567"]
        );
    }

    #[test]
    fn times_are_iso() {
        assert_eq!(iso_utc(0.0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_761_399_211.0), "2025-10-25T13:33:31Z");
    }

    #[test]
    fn only_configured_connectors_exist() {
        assert!(connectors(&NotifySettings::default()).is_empty());
        let on = NotifySettings {
            discord_webhook: "https://discord.com/api/webhooks/1/abc".into(),
            sales: true,
            store: true,
        };
        assert_eq!(connectors(&on).iter().map(|c| c.name()).collect::<Vec<_>>(), ["Discord"]);
    }
}
