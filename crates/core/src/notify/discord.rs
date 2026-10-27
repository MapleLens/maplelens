//! Discord webhooks: one POST of JSON to the channel's webhook URL, no account, no server of ours.

use serde_json::{Value, json};

use super::{Connector, EXAMPLE_ITEM, Notification, Request, Sale, StoreChange, grouped, iso_utc, item_icon_url};

/// Discord's colour for an embed's side bar: MapleLens orange.
const COLOUR: u32 = 0xFFAD1F;
/// Embeds in one message (Discord's limit).
const EMBEDS_PER_MESSAGE: usize = 10;
/// Discord shows at most this much of an embed title.
const TITLE_CHARS: usize = 256;
const AUTHOR_CHARS: usize = 256;
/// Green for a store found open, grey for one that closed.
const OPEN_COLOUR: u32 = 0x4EE08A;
const CLOSED_COLOUR: u32 = 0x8A94B8;
const USERNAME: &str = "MapleLens";
/// The hosts Discord serves webhooks from. Anything else is refused: the URL is typed in by the player, and a message
/// about their sales must not be sent to some other server because of a typo or a pasted lookalike.
const HOSTS: [&str; 4] = ["discord.com", "discordapp.com", "ptb.discord.com", "canary.discord.com"];

pub struct Discord {
    url: String,
}

/// `https://discord.com/api/webhooks/<id>/<token>` and nothing else (no query, no extra path): the URL as given,
/// trimmed, when it is one.
pub fn valid_webhook(url: &str) -> Option<&str> {
    let url = url.trim();
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !HOSTS.contains(&host) {
        return None;
    }
    let mut parts = path.strip_prefix("api/webhooks/")?.split('/');
    let (id, token) = (parts.next()?, parts.next()?);
    let id_ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_digit());
    let token_ok = !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    (id_ok && token_ok && parts.next().is_none()).then_some(url)
}

/// The URL with its secret token hidden, for showing in the settings and the log.
pub fn masked(url: &str) -> String {
    match valid_webhook(url) {
        Some(valid) => {
            let (head, token) = valid.rsplit_once('/').unwrap_or((valid, ""));
            let tail: String = token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("{head}/…{tail}")
        }
        None => String::new(),
    }
}

impl Discord {
    pub fn new(url: &str) -> Option<Self> {
        valid_webhook(url).map(|url| Self { url: url.to_string() })
    }

    /// A sale as a card: the store's owner on top, the item's picture and name, then buyer, what came in and what
    /// is in the till as three short columns.
    fn embed(notification: &Notification, unix_now: f64) -> Value {
        match notification {
            Notification::Sale(sale) => Self::sale_embed(sale, unix_now),
            Notification::Store(change) => Self::store_embed(change, unix_now),
        }
    }

    /// The store being found open (green) or closing (grey), with whose it is and what is in the till.
    fn store_embed(change: &StoreChange, unix_now: f64) -> Value {
        let whose = change
            .owner
            .as_ref()
            .map_or_else(|| "your store".to_string(), |owner| format!("{}'s store", escape(owner)));
        let (title, text, colour) = if change.opened {
            ("Store detected", format!("MapleLens is watching {whose}."), OPEN_COLOUR)
        } else {
            ("Store closed", format!("{} is no longer open.", capitalised(&whose)), CLOSED_COLOUR)
        };
        let mut embed = json!({
            "title": title,
            "description": text,
            "color": colour,
            "footer": { "text": "MapleLens · Store Permit" },
            "timestamp": iso_utc(unix_now),
        });
        if let Some(balance) = change.balance {
            embed["fields"] = json!([{"name": "Current mesos", "value": format!("🪙 **{}** mesos", grouped(balance)), "inline": true}]);
        }
        embed
    }

    fn sale_embed(sale: &Sale, unix_now: f64) -> Value {
        let Sale { item, buyer, quantity, mesos, picture, store, balance, .. } = sale;
        let what = if *quantity > 1 { format!("{item} ×{quantity}") } else { item.clone() };
        let mut fields = vec![json!({"name": "Buyer", "value": format!("**{}**", escape(buyer)), "inline": true})];
        if let Some(mesos) = mesos {
            fields.push(json!({"name": "Received", "value": format!("🪙 **{}**", grouped(*mesos)), "inline": true}));
        }
        if let Some(balance) = balance {
            fields.push(
                json!({"name": "Current mesos", "value": format!("{} mesos", grouped(*balance)), "inline": true}),
            );
        }
        let mut embed = json!({
            "title": truncate(&what, TITLE_CHARS),
            "color": COLOUR,
            "fields": fields,
            "footer": { "text": "MapleLens · Store Permit" },
            "timestamp": iso_utc(unix_now),
        });
        let author = match store {
            Some(owner) => format!("Sold in {owner}'s store"),
            None => "Sold in your store".to_string(),
        };
        embed["author"] = json!({ "name": truncate(&author, AUTHOR_CHARS) });
        if let Some(id) = picture {
            embed["thumbnail"] = json!({ "url": item_icon_url(*id) });
        }
        embed
    }

    fn message(&self, embeds: Vec<Value>) -> Request {
        // `allowed_mentions` empty: a name or item OCR'd as "@everyone" must not ping anyone.
        let body = json!({ "username": USERNAME, "allowed_mentions": { "parse": [] }, "embeds": embeds });
        Request { url: self.url.clone(), body: body.to_string(), fallback: None }
    }
}

/// Markdown in a name would restyle the message (and `[x](url)` makes a link).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '_' | '~' | '`' | '|' | '>' | '[' | ']' | '(' | ')' | '\\' | '@' | '#' | ':') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

fn truncate(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars { text.to_string() } else { text.chars().take(chars - 1).chain(['…']).collect() }
}

impl Connector for Discord {
    fn name(&self) -> &'static str {
        "Discord"
    }

    fn requests(&self, batch: &[Notification], unix_now: f64) -> Vec<Request> {
        batch
            .chunks(EMBEDS_PER_MESSAGE)
            .map(|chunk| self.message(chunk.iter().map(|n| Self::embed(n, unix_now)).collect()))
            .collect()
    }

    fn test_request(&self, owner: Option<&str>, unix_now: f64) -> Request {
        let whose = owner.map_or_else(|| "your store".to_string(), |owner| format!("{}'s store", escape(owner)));
        // A sale card as one will look, picture included, so the player sees what they will get.
        let example = Sale {
            item: EXAMPLE_ITEM.0.to_string(),
            buyer: "Buyer".to_string(),
            quantity: 8,
            mesos: Some(388),
            item_id: Some(EXAMPLE_ITEM.1),
            picture: Some(EXAMPLE_ITEM.1),
            store: owner.map(str::to_string),
            balance: None,
        };
        let mut sale = Self::sale_embed(&example, unix_now);
        sale["footer"] = json!({ "text": "MapleLens · example sale" });
        self.message(vec![
            json!({
                "title": "MapleLens is connected",
                "description": format!("Sold items and openings of {whose} will show up here, like this:"),
                "color": COLOUR,
                "footer": { "text": "MapleLens · test message" },
                "timestamp": iso_utc(unix_now),
            }),
            sale,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://discord.com/api/webhooks/123456789012345678/FakeTokenForTests_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUV";

    fn sale(item: &str, buyer: &str, quantity: u64) -> Notification {
        Notification::Sale(Sale { item: item.into(), buyer: buyer.into(), quantity, ..Sale::default() })
    }

    #[test]
    fn only_discords_own_webhook_urls_are_accepted() {
        assert_eq!(valid_webhook(&format!("  {URL}\n")), Some(URL));
        assert!(valid_webhook("https://ptb.discord.com/api/webhooks/12/a-b_c").is_some());
        for bad in [
            "",
            "http://discord.com/api/webhooks/12/abc",
            "https://discord.com.evil.example/api/webhooks/12/abc",
            "https://evil.example/api/webhooks/12/abc",
            "https://discord.com/api/webhooks/12",
            "https://discord.com/api/webhooks/ab/abc",
            "https://discord.com/api/webhooks/12/abc/extra",
            "https://discord.com/api/webhooks/12/abc?wait=true",
            "https://discord.com@evil.example/api/webhooks/12/abc",
            "https://discord.com/api/channels/12/abc",
        ] {
            assert_eq!(valid_webhook(bad), None, "{bad}");
            assert!(Discord::new(bad).is_none());
        }
    }

    #[test]
    fn the_token_is_hidden_when_shown() {
        let shown = masked(URL);
        assert!(shown.starts_with("https://discord.com/api/webhooks/123456789012345678/…"));
        assert!(shown.ends_with("STUV") && !shown.contains("FakeToken"));
        assert_eq!(masked("nonsense"), "");
    }

    #[test]
    fn a_sale_becomes_an_embed() {
        let discord = Discord::new(URL).unwrap();
        let requests = discord.requests(&[sale("Pet Food", "Tikko", 20)], 1_761_399_211.0);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, URL);
        let body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(body["embeds"][0]["title"], "Pet Food ×20");
        assert_eq!(body["embeds"][0]["fields"][0], json!({"name": "Buyer", "value": "**Tikko**", "inline": true}));
        assert_eq!(body["embeds"][0]["author"]["name"], "Sold in your store");
        assert_eq!(body["embeds"][0]["timestamp"], "2025-10-25T13:33:31Z");
        assert_eq!(body["allowed_mentions"]["parse"], json!([]));
    }

    #[test]
    fn a_single_item_has_no_quantity() {
        let discord = Discord::new(URL).unwrap();
        let body: Value = serde_json::from_str(&discord.requests(&[sale("Iron Ore", "Wina", 1)], 0.0)[0].body).unwrap();
        assert_eq!(body["embeds"][0]["title"], "Iron Ore");
    }

    #[test]
    fn a_burst_of_sales_is_batched_ten_to_a_message() {
        let discord = Discord::new(URL).unwrap();
        let burst: Vec<Notification> = (0..23).map(|i| sale("Pet Food", &format!("Buyer{i}"), 1)).collect();
        let sizes: Vec<usize> = discord
            .requests(&burst, 0.0)
            .iter()
            .map(|r| serde_json::from_str::<Value>(&r.body).unwrap()["embeds"].as_array().unwrap().len())
            .collect();
        assert_eq!(sizes, [10, 10, 3]);
        assert!(discord.requests(&[], 0.0).is_empty());
    }

    #[test]
    fn names_cannot_ping_or_restyle() {
        let discord = Discord::new(URL).unwrap();
        let hostile = sale("@everyone", "**x** [click](https://evil.example) @here", 1);
        let request = &discord.requests(&[hostile], 0.0)[0];
        let body: Value = serde_json::from_str(&request.body).unwrap();
        let description = body["embeds"][0]["fields"][0]["value"].as_str().unwrap();
        assert!(!description.contains("[click](") && description.contains("\\[click\\]\\(https\\:"), "{description}");
        assert_eq!(body["allowed_mentions"]["parse"], json!([]));
    }

    #[test]
    fn a_full_sale_shows_the_store_the_picture_the_price_and_the_balance() {
        let discord = Discord::new(URL).unwrap();
        let full = Notification::Sale(Sale {
            item: "Pet Food".into(),
            buyer: "Tikko".into(),
            quantity: 20,
            mesos: Some(12_400),
            item_id: Some(2_120_000),
            picture: Some(2_120_000),
            store: Some("ShopOwner".into()),
            balance: Some(39_311),
        });
        let body: Value = serde_json::from_str(&discord.requests(&[full], 0.0)[0].body).unwrap();
        let embed = &body["embeds"][0];
        assert_eq!(embed["author"]["name"], "Sold in ShopOwner's store");
        assert!(embed["thumbnail"]["url"].as_str().unwrap().ends_with("/item/2120000/icon"));
        let fields: Vec<(&str, &str)> = embed["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["name"].as_str().unwrap(), f["value"].as_str().unwrap()))
            .collect();
        assert_eq!(fields, [("Buyer", "**Tikko**"), ("Received", "🪙 **12,400**"), ("Current mesos", "39,311 mesos")]);
    }

    #[test]
    fn only_the_picture_given_is_shown() {
        // The connectors show what `Pictures::fill` gave the sale, never an id it was not checked for.
        let discord = Discord::new(URL).unwrap();
        let garnet = Notification::Sale(Sale { item: "Garnet".into(), item_id: Some(4_020_100), ..Sale::default() });
        let body: Value = serde_json::from_str(&discord.requests(&[garnet], 0.0)[0].body).unwrap();
        assert!(body["embeds"][0].get("thumbnail").is_none());
    }

    #[test]
    fn a_store_opening_and_closing_are_cards_too() {
        let discord = Discord::new(URL).unwrap();
        let change = |opened| {
            Notification::Store(StoreChange { opened, owner: Some("ShopOwner".into()), balance: Some(39_311) })
        };
        let body = |n: Notification| serde_json::from_str::<Value>(&discord.requests(&[n], 0.0)[0].body).unwrap();
        let opened = body(change(true));
        assert_eq!(opened["embeds"][0]["title"], "Store detected");
        assert_eq!(opened["embeds"][0]["description"], "MapleLens is watching ShopOwner's store.");
        assert_eq!(opened["embeds"][0]["color"], OPEN_COLOUR);
        assert_eq!(opened["embeds"][0]["fields"][0]["value"], "🪙 **39,311** mesos");
        let closed = body(change(false));
        assert_eq!(closed["embeds"][0]["title"], "Store closed");
        assert_eq!(closed["embeds"][0]["description"], "ShopOwner's store is no longer open.");
        let nameless = body(Notification::Store(StoreChange { opened: false, owner: None, balance: None }));
        assert_eq!(nameless["embeds"][0]["description"], "Your store is no longer open.");
        assert!(nameless["embeds"][0].get("fields").is_none());
    }

    #[test]
    fn long_titles_are_cut() {
        let discord = Discord::new(URL).unwrap();
        let body: Value =
            serde_json::from_str(&discord.requests(&[sale(&"x".repeat(400), "Wina", 1)], 0.0)[0].body).unwrap();
        assert_eq!(body["embeds"][0]["title"].as_str().unwrap().chars().count(), 256);
    }

    #[test]
    fn the_test_message_goes_to_the_same_url() {
        let discord = Discord::new(URL).unwrap();
        let request = discord.test_request(Some("ShopOwner"), 0.0);
        assert_eq!(request.url, URL);
        assert!(request.body.contains("MapleLens is connected") && request.body.contains("ShopOwner's store"));
        // With an example sale card, picture included.
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert!(body["embeds"][1]["thumbnail"]["url"].as_str().unwrap().ends_with("/item/2120000/icon"));
    }
}
