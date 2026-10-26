//! Telegram bots: one POST of JSON to the Bot API (`sendMessage`), no account of ours and no server. Like a Discord
//! webhook, the player pastes one URL, `https://api.telegram.org/bot<token>/sendMessage?chat_id=<chat>`. A URL without
//! the chat, or just the token from @BotFather, works too: the chat is then found by asking the bot who wrote to it
//! last (`getUpdates`), so the player only has to send it /start, and the whole URL is built for them.

use serde_json::{Value, json};

use super::{Connector, Notification, Request, Sale, StoreChange, grouped};

const API: &str = "https://api.telegram.org/bot";
/// Notifications in one message: a burst of sales is one message, well under Telegram's 4096 characters.
const PER_MESSAGE: usize = 10;
/// An item name longer than this is cut (no game item comes close; a misread could).
const ITEM_CHARS: usize = 256;
/// What Telegram's tokens look like after the bot's id and ':' (35 today; a little room either way).
const SECRET_CHARS: std::ops::RangeInclusive<usize> = 30..=60;

pub struct Telegram {
    token: String,
    chat: i64,
}

/// `<bot id>:<secret>` and nothing else: the token as given, trimmed, when it is one.
pub fn valid_token(token: &str) -> Option<&str> {
    let token = token.trim();
    let (id, secret) = token.split_once(':')?;
    let id_ok = !id.is_empty() && id.len() <= 20 && id.chars().all(|c| c.is_ascii_digit());
    let secret_ok = SECRET_CHARS.contains(&secret.len())
        && secret.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    (id_ok && secret_ok).then_some(token)
}

/// The first token in `text`: @BotFather's whole message may be what was copied.
pub fn find_token(text: &str) -> Option<&str> {
    text.split(|c: char| c.is_whitespace() || matches!(c, '`' | '"' | '\'' | ',' | '.')).find_map(valid_token)
}

/// The token's last four characters, for showing in the settings and the log ("" when it is not a token).
pub fn masked(token: &str) -> String {
    match valid_token(token) {
        Some(valid) => {
            let tail: String = valid.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("…{tail}")
        }
        None => String::new(),
    }
}

/// A bot and its chat as one URL, the setting's form; without `?chat_id=` while the chat is not known (0).
pub fn webhook_url(token: &str, chat: i64) -> String {
    match chat {
        0 => format!("{API}{token}/sendMessage"),
        chat => format!("{API}{token}/sendMessage?chat_id={chat}"),
    }
}

/// The bot and its chat (0 = not known yet) in what was pasted: a `sendMessage` URL (other query parameters, a
/// guide's `&text=hi`, are ignored), or a token alone or inside @BotFather's message. A chat that is not a number (a
/// channel's @name) is refused: the messages would go nowhere the bot was told about.
pub fn parse_webhook(text: &str) -> Option<(&str, i64)> {
    let text = text.trim();
    let Some(rest) = text.strip_prefix(API) else { return find_token(text).map(|token| (token, 0)) };
    let (token, method) = rest.split_once('/')?;
    let token = valid_token(token).filter(|t| t.len() == token.len())?;
    let query = match method.split_once('?') {
        Some(("sendMessage", query)) => query,
        None if method == "sendMessage" => "",
        _ => return None,
    };
    let chat = match query.split('&').find_map(|pair| pair.strip_prefix("chat_id=")) {
        Some(chat) => chat.parse::<i64>().ok().filter(|&chat| chat != 0)?,
        None => 0,
    };
    Some((token, chat))
}

/// The request that asks the bot for the messages people sent it lately (`getUpdates`; Telegram keeps them a day).
pub fn link_request(token: &str) -> Option<Request> {
    valid_token(token).map(|token| Request { url: format!("{API}{token}/getUpdates"), body: "{}".into() })
}

/// The chat that wrote to the bot last, from a `getUpdates` answer: a private chat after /start, or a group or
/// channel the bot was added to.
pub fn chat_from_updates(answer: &str) -> Option<i64> {
    let answer: Value = serde_json::from_str(answer).ok()?;
    let updates = answer["result"].as_array()?;
    updates.iter().rev().find_map(|update| {
        ["message", "channel_post", "my_chat_member", "edited_message"]
            .iter()
            .find_map(|kind| update[kind]["chat"]["id"].as_i64())
            .filter(|&id| id != 0)
    })
}

impl Telegram {
    /// A bot and the chat it writes to; None until both are known.
    pub fn new(token: &str, chat: i64) -> Option<Self> {
        let token = valid_token(token)?;
        (chat != 0).then(|| Self { token: token.to_string(), chat })
    }

    fn text(notification: &Notification) -> String {
        match notification {
            Notification::Sale(sale) => sale_text(sale),
            Notification::Store(change) => store_text(change),
        }
    }

    fn message(&self, text: String) -> Request {
        let body = json!({
            "chat_id": self.chat,
            "text": text,
            "parse_mode": "HTML",
            "link_preview_options": { "is_disabled": true },
        });
        Request { url: format!("{API}{}/sendMessage", self.token), body: body.to_string() }
    }
}

/// "ShopOwner's store", or "your store" while the name is not read.
fn whose(owner: Option<&str>) -> String {
    owner.map_or_else(|| "your store".to_string(), |owner| format!("{}'s store", escape(owner)))
}

/// A sale: what sold on top, then buyer, what came in, what is in the till and whose store it is.
fn sale_text(sale: &Sale) -> String {
    let Sale { item, buyer, quantity, mesos, store, balance, .. } = sale;
    let item = escape(&truncate(item, ITEM_CHARS));
    let what = if *quantity > 1 { format!("{item} ×{quantity}") } else { item };
    let mut lines = vec![format!("💰 <b>{what}</b> sold"), format!("Buyer: <b>{}</b>", escape(buyer))];
    if let Some(mesos) = mesos {
        lines.push(format!("Received: 🪙 <b>{}</b> mesos", grouped(*mesos)));
    }
    if let Some(balance) = balance {
        lines.push(format!("Store balance: {} mesos", grouped(*balance)));
    }
    lines.push(format!("<i>In {}</i>", whose(store.as_deref())));
    lines.join("\n")
}

/// The store being found open or closing, with whose it is and what is in the till.
fn store_text(change: &StoreChange) -> String {
    let whose = whose(change.owner.as_deref());
    let mut lines = if change.opened {
        vec!["🟢 <b>Store detected</b>".to_string(), format!("MapleLens is watching {whose}.")]
    } else {
        vec!["⚪ <b>Store closed</b>".to_string(), format!("{} is no longer open.", capitalised(&whose))]
    };
    if let Some(balance) = change.balance {
        lines.push(format!("Store balance: 🪙 <b>{}</b> mesos", grouped(balance)));
    }
    lines.join("\n")
}

/// HTML is the message's markup, so a name's '<' or '&' must stay text; an '@' is kept from becoming a mention (a
/// read name could be someone's username, and in a group that pings them).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '@' => out.push_str("@\u{2060}"),
            c => out.push(c),
        }
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

/// The item the test message's example sale shows.
const EXAMPLE_ITEM: &str = "Pet Food";

impl Connector for Telegram {
    fn name(&self) -> &'static str {
        "Telegram"
    }

    fn requests(&self, batch: &[Notification], _unix_now: f64) -> Vec<Request> {
        batch
            .chunks(PER_MESSAGE)
            .map(|chunk| self.message(chunk.iter().map(Self::text).collect::<Vec<_>>().join("\n\n")))
            .collect()
    }

    fn test_request(&self, owner: Option<&str>, _unix_now: f64) -> Request {
        let example = Sale {
            item: EXAMPLE_ITEM.to_string(),
            buyer: "Buyer".to_string(),
            quantity: 8,
            mesos: Some(388),
            store: owner.map(str::to_string),
            ..Sale::default()
        };
        self.message(format!(
            "✅ <b>MapleLens is connected</b>\nSold items and openings of {} will show up here, like this:\n\n{}",
            whose(owner),
            sale_text(&example)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "123456789:AAFakeTokenForTests_abcdefghijklmnop-Q";

    fn body(request: &Request) -> Value {
        serde_json::from_str(&request.body).unwrap()
    }

    fn sale(item: &str, buyer: &str, quantity: u64) -> Notification {
        Notification::Sale(Sale { item: item.into(), buyer: buyer.into(), quantity, ..Sale::default() })
    }

    #[test]
    fn only_bot_tokens_are_accepted() {
        assert_eq!(valid_token(&format!("  {TOKEN}\n")), Some(TOKEN));
        for bad in [
            "",
            "123456789",
            ":AAFakeTokenForTests_abcdefghijklmnop-Q",
            "abc:AAFakeTokenForTests_abcdefghijklmnop-Q",
            "123456789:short",
            "123456789:AAFakeTokenForTests_abcdefghijklmnop/Q",
            "123456789:AAFakeTokenForTests_abcdefghijklmnop-Q/sendMessage",
            "https://api.telegram.org/bot123456789:AAFakeTokenForTests_abcdefghijklmnop-Q",
        ] {
            assert_eq!(valid_token(bad), None, "{bad}");
        }
        assert!(Telegram::new(TOKEN, 0).is_none(), "no chat yet");
        assert!(Telegram::new("nonsense", 42).is_none());
    }

    #[test]
    fn the_token_is_found_in_botfathers_message() {
        let message = format!("Done! Use this token to access the HTTP API:\n{TOKEN}\nKeep your token secure.");
        assert_eq!(find_token(&message), Some(TOKEN));
        assert_eq!(find_token(&format!("`{TOKEN}`")), Some(TOKEN));
        assert_eq!(find_token("no token here: 12:ab"), None);
    }

    #[test]
    fn one_url_holds_the_bot_and_its_chat() {
        let url = format!("https://api.telegram.org/bot{TOKEN}/sendMessage?chat_id=-100222");
        assert_eq!(parse_webhook(&format!(" {url}\n")), Some((TOKEN, -100_222)));
        assert_eq!(webhook_url(TOKEN, -100_222), url);
        assert_eq!(parse_webhook(&format!("{url}&text=hi")), Some((TOKEN, -100_222)));
        let unlinked = webhook_url(TOKEN, 0);
        assert_eq!(unlinked, format!("https://api.telegram.org/bot{TOKEN}/sendMessage"));
        assert_eq!(parse_webhook(&unlinked), Some((TOKEN, 0)));
        assert_eq!(parse_webhook(&format!("Use this token to access the HTTP API:\n{TOKEN}")), Some((TOKEN, 0)));
        for bad in [
            format!("https://api.telegram.org/bot{TOKEN}/sendMessage?chat_id=@mychannel"),
            format!("https://api.telegram.org/bot{TOKEN}/sendMessage?chat_id=0"),
            format!("https://api.telegram.org/bot{TOKEN}/deleteMessage?chat_id=1"),
            format!("https://api.telegram.org/bot{TOKEN}!/sendMessage?chat_id=1"),
            format!("http://api.telegram.org/bot{TOKEN}/sendMessage?chat_id=1"),
            format!("https://api.telegram.org.evil.example/bot{TOKEN}/sendMessage?chat_id=1"),
            "https://discord.com/api/webhooks/12/abc".to_string(),
        ] {
            assert_eq!(parse_webhook(&bad), None, "{bad}");
        }
    }

    #[test]
    fn the_token_is_hidden_when_shown() {
        assert_eq!(masked(TOKEN), "…op-Q");
        assert_eq!(masked("nonsense"), "");
    }

    #[test]
    fn the_chat_is_whoever_wrote_last() {
        let request = link_request(TOKEN).unwrap();
        assert_eq!(request.url, format!("https://api.telegram.org/bot{TOKEN}/getUpdates"));
        let answer = r#"{"ok":true,"result":[
            {"update_id":1,"message":{"chat":{"id":111,"type":"private"},"text":"/start"}},
            {"update_id":2,"my_chat_member":{"chat":{"id":-100222,"type":"channel"}}},
            {"update_id":3,"callback_query":{"id":"x"}}]}"#;
        assert_eq!(chat_from_updates(answer), Some(-100_222));
        assert_eq!(chat_from_updates(r#"{"ok":true,"result":[]}"#), None);
        assert_eq!(chat_from_updates(r#"{"ok":false,"error_code":401}"#), None);
        assert_eq!(chat_from_updates("not json"), None);
        assert!(link_request("nonsense").is_none());
    }

    #[test]
    fn a_sale_becomes_a_message_to_the_chat() {
        let telegram = Telegram::new(TOKEN, 111).unwrap();
        let full = Notification::Sale(Sale {
            item: "Pet Food".into(),
            buyer: "Tikko".into(),
            quantity: 20,
            mesos: Some(12_400),
            item_id: Some(2_120_000),
            store: Some("ShopOwner".into()),
            balance: Some(39_311),
        });
        let requests = telegram.requests(&[full], 0.0);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, format!("https://api.telegram.org/bot{TOKEN}/sendMessage"));
        let body = body(&requests[0]);
        assert_eq!(body["chat_id"], 111);
        assert_eq!(body["parse_mode"], "HTML");
        assert_eq!(
            body["text"],
            "💰 <b>Pet Food ×20</b> sold\nBuyer: <b>Tikko</b>\nReceived: 🪙 <b>12,400</b> mesos\n\
             Store balance: 39,311 mesos\n<i>In ShopOwner's store</i>"
        );
    }

    #[test]
    fn a_burst_is_batched_ten_to_a_message() {
        let telegram = Telegram::new(TOKEN, 111).unwrap();
        let burst: Vec<Notification> = (0..23).map(|i| sale("Pet Food", &format!("Buyer{i}"), 1)).collect();
        let sizes: Vec<usize> = telegram
            .requests(&burst, 0.0)
            .iter()
            .map(|r| body(r)["text"].as_str().unwrap().matches(" sold").count())
            .collect();
        assert_eq!(sizes, [10, 10, 3]);
        assert!(telegram.requests(&[], 0.0).is_empty());
    }

    #[test]
    fn names_cannot_restyle_or_mention() {
        let telegram = Telegram::new(TOKEN, 111).unwrap();
        let hostile = sale("<a href=\"https://evil.example\">x</a>", "@everyone & co", 1);
        let text = body(&telegram.requests(&[hostile], 0.0)[0])["text"].as_str().unwrap().to_string();
        assert!(!text.contains("<a ") && text.contains("&lt;a href="), "{text}");
        assert!(text.contains("@\u{2060}everyone &amp; co"), "{text}");
    }

    #[test]
    fn a_store_opening_and_closing_are_messages_too() {
        let telegram = Telegram::new(TOKEN, 111).unwrap();
        let text = |n: Notification| body(&telegram.requests(&[n], 0.0)[0])["text"].as_str().unwrap().to_string();
        let opened =
            text(Notification::Store(StoreChange { opened: true, owner: Some("ShopOwner".into()), balance: Some(1) }));
        assert_eq!(
            opened,
            "🟢 <b>Store detected</b>\nMapleLens is watching ShopOwner's store.\nStore balance: 🪙 <b>1</b> mesos"
        );
        let closed = text(Notification::Store(StoreChange::default()));
        assert_eq!(closed, "⚪ <b>Store closed</b>\nYour store is no longer open.");
    }

    #[test]
    fn the_test_message_shows_an_example_sale() {
        let telegram = Telegram::new(TOKEN, 111).unwrap();
        let text = body(&telegram.test_request(Some("ShopOwner"), 0.0))["text"].as_str().unwrap().to_string();
        assert!(text.starts_with("✅ <b>MapleLens is connected</b>") && text.contains("ShopOwner's store"), "{text}");
        assert!(text.contains("💰 <b>Pet Food ×8</b> sold"), "{text}");
    }
}
