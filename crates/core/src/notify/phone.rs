//! The player's phone: a push notification through MapleLens's relay (`relay/push`, a Cloudflare Worker). The phone
//! pairs by scanning a QR code of `phone_url(key)`; MapleLens then posts a title and a line of text with the same key
//! and the relay pushes it to every phone paired with it. The key is the only credential, so it is never logged.

use serde_json::json;

use super::{Connector, Notification, Request, Sale, StoreChange, grouped, item_icon_url};

/// The relay; `docs/app/phone/relay.js` has the same address.
pub const RELAY_URL: &str = "https://push.maple.observer";
/// The phone page the QR code opens.
pub const PHONE_PAGE: &str = "https://maplelens.github.io/maplelens/app/phone/";
/// The relay keeps this much of a title and a body (a push message holds ~4 KB).
const TITLE_CHARS: usize = 120;
const BODY_CHARS: usize = 600;
/// Lines a batch of sales lists before "and N more": a phone shows four or five lines of a notification.
const LISTED_SALES: usize = 5;
/// Store messages share a tag, so "closed" replaces "detected" on the phone instead of piling up.
const STORE_TAG: &str = "store";

pub struct Phone {
    key: String,
}

/// `<id>.<secret>` (base64url, 16-32 and 22-64 characters: the relay checks the same), trimmed, when `key` is one.
pub fn valid_key(key: &str) -> Option<&str> {
    let key = key.trim();
    let (id, secret) = key.split_once('.')?;
    let base64url = |part: &str, sizes: std::ops::RangeInclusive<usize>| {
        sizes.contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    (base64url(id, 16..=32) && base64url(secret, 22..=64)).then_some(key)
}

/// The share of a drawn QR code's width the meso coin's tile covers (docs/app/mapleqr.js, examples/phone_qr.rs):
/// with high error correction a fifth of the width (4% of the modules) is well within what the code recovers.
pub const LOGO_SHARE: f64 = 0.2;

/// What the QR code carries: the phone page with the key.
pub fn phone_url(key: &str) -> String {
    format!("{PHONE_PAGE}?pair={key}")
}

/// The QR code of `text` as rows of dark (true) and light modules, `width` by `width`, without the quiet zone.
/// High error correction: the drawn code has the meso coin over its centre (`LOGO_SHARE` of its width), which the
/// other 30% of recovery covers with room to spare for a monitor read at an angle.
pub fn qr_modules(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::H).ok()?;
    let modules = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some((code.width(), modules))
}

impl Phone {
    pub fn new(key: &str) -> Option<Self> {
        valid_key(key).map(|key| Self { key: key.to_string() })
    }

    fn message(&self, title: &str, body: &str, icon: Option<String>, tag: Option<&str>) -> Request {
        let mut message =
            json!({ "key": self.key, "title": truncate(title, TITLE_CHARS), "body": truncate(body, BODY_CHARS) });
        if let Some(icon) = icon {
            message["icon"] = json!(icon);
        }
        if let Some(tag) = tag {
            message["tag"] = json!(tag);
        }
        Request { url: format!("{RELAY_URL}/notify"), body: message.to_string(), fallback: None }
    }

    fn store(&self, change: &StoreChange) -> Request {
        let whose = change.owner.as_ref().map_or_else(|| "your store".to_string(), |owner| format!("{owner}'s store"));
        let (title, mut body) = if change.opened {
            ("Store detected", format!("MapleLens is watching {whose}."))
        } else {
            ("Store closed", format!("{} is no longer open.", capitalised(&whose)))
        };
        if let Some(balance) = change.balance {
            body.push_str(&format!(" Current mesos: {}.", grouped(balance)));
        }
        self.message(title, &body, None, Some(STORE_TAG))
    }

    /// One notification for every sale in the batch: a phone buzzing ten times for one buyer's spree is noise.
    fn sales(&self, sales: &[&Sale]) -> Option<Request> {
        let (first, rest) = sales.split_first()?;
        let store = sales.iter().find_map(|s| s.store.as_deref());
        let balance = sales.iter().rev().find_map(|s| s.balance);
        if rest.is_empty() {
            let mut parts = vec![format!("Bought by {}", first.buyer)];
            if let Some(mesos) = first.mesos {
                parts.push(format!("+{} mesos", grouped(mesos)));
            }
            if let Some(balance) = balance {
                parts.push(format!("{} mesos now", grouped(balance)));
            }
            let title = match store {
                Some(owner) => format!("Sold: {} · {owner}'s store", what(first)),
                None => format!("Sold: {}", what(first)),
            };
            // The picture `Pictures::fill` gave it (its own id, or one found by name).
            return Some(self.message(&title, &parts.join(" · "), first.picture.map(item_icon_url), None));
        }
        let mut lines: Vec<String> = sales
            .iter()
            .take(LISTED_SALES)
            .map(|s| match s.mesos {
                Some(mesos) => format!("{} to {} (+{})", what(s), s.buyer, grouped(mesos)),
                None => format!("{} to {}", what(s), s.buyer),
            })
            .collect();
        if sales.len() > LISTED_SALES {
            lines.push(format!("and {} more", sales.len() - LISTED_SALES));
        }
        if let Some(balance) = balance {
            lines.push(format!("Current mesos: {}", grouped(balance)));
        }
        let title = match store {
            Some(owner) => format!("{} items sold · {owner}'s store", sales.len()),
            None => format!("{} items sold", sales.len()),
        };
        Some(self.message(&title, &lines.join("\n"), None, None))
    }
}

fn what(sale: &Sale) -> String {
    if sale.quantity > 1 { format!("{} ×{}", sale.item, sale.quantity) } else { sale.item.clone() }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

fn truncate(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars { text.to_string() } else { text.chars().take(chars - 1).chain(['…']).collect() }
}

impl Connector for Phone {
    fn name(&self) -> &'static str {
        "Phone"
    }

    fn requests(&self, batch: &[Notification], _unix_now: f64) -> Vec<Request> {
        let mut requests: Vec<Request> = batch
            .iter()
            .filter_map(|n| match n {
                Notification::Store(change) => Some(self.store(change)),
                Notification::Sale(_) => None,
            })
            .collect();
        let sales: Vec<&Sale> = batch
            .iter()
            .filter_map(|n| match n {
                Notification::Sale(sale) => Some(sale),
                Notification::Store(_) => None,
            })
            .collect();
        requests.extend(self.sales(&sales));
        requests
    }

    fn test_request(&self, owner: Option<&str>, _unix_now: f64) -> Request {
        let whose = owner.map_or_else(|| "your store".to_string(), |owner| format!("{owner}'s store"));
        self.message(
            "MapleLens is connected",
            &format!("Sold items and openings of {whose} will show up here."),
            None,
            Some("test"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const KEY: &str = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";

    fn body(request: &Request) -> Value {
        serde_json::from_str(&request.body).unwrap()
    }

    fn sale(item: &str, buyer: &str, quantity: u64, mesos: Option<i64>) -> Notification {
        Notification::Sale(Sale { item: item.into(), buyer: buyer.into(), quantity, mesos, ..Sale::default() })
    }

    #[test]
    fn only_well_formed_keys_are_accepted() {
        assert_eq!(valid_key(&format!(" {KEY}\n")), Some(KEY));
        for bad in [
            "",
            "nodot",
            "short.qRsTuVwXyZ0123456789-_ab",
            "AbCdEfGhIjKlMnOp.short",
            "AbCdEfGhIjKlMnOp.qRsTuVwXyZ01234567/9+_ab",
        ] {
            assert_eq!(valid_key(bad), None, "{bad}");
        }
        assert_eq!(phone_url(KEY), format!("{PHONE_PAGE}?pair={KEY}"));
    }

    #[test]
    fn the_pairing_address_fits_a_small_qr_code() {
        let (width, modules) = qr_modules(&phone_url(KEY)).unwrap();
        assert_eq!(modules.len(), width * width);
        // Version 10 (57 modules) or smaller: modules stay big enough to read off a monitor from across a desk.
        assert!(width <= 57, "{width}");
        // The finder pattern's corner is dark.
        assert!(modules[0] && modules[width - 1]);
    }

    #[test]
    fn a_sale_names_the_item_buyer_and_takings() {
        let phone = Phone::new(KEY).unwrap();
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
        let requests = phone.requests(&[full], 0.0);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, format!("{RELAY_URL}/notify"));
        let message = body(&requests[0]);
        assert_eq!(message["key"], KEY);
        assert_eq!(message["title"], "Sold: Pet Food ×20 · ShopOwner's store");
        assert_eq!(message["body"], "Bought by Tikko · +12,400 mesos · 39,311 mesos now");
        assert!(message["icon"].as_str().unwrap().ends_with("/item/2120000/icon"));
        assert!(message.get("tag").is_none());
    }

    #[test]
    fn a_burst_of_sales_is_one_notification() {
        let phone = Phone::new(KEY).unwrap();
        let burst: Vec<Notification> = (0..7).map(|i| sale("Arrow", &format!("Buyer{i}"), 100, Some(500))).collect();
        let requests = phone.requests(&burst, 0.0);
        assert_eq!(requests.len(), 1);
        let message = body(&requests[0]);
        assert_eq!(message["title"], "7 items sold");
        let lines: Vec<&str> = message["body"].as_str().unwrap().lines().collect();
        assert_eq!(lines.len(), LISTED_SALES + 1);
        assert_eq!(lines[0], "Arrow ×100 to Buyer0 (+500)");
        assert_eq!(lines[LISTED_SALES], "and 2 more");
    }

    #[test]
    fn store_changes_are_their_own_tagged_notifications() {
        let phone = Phone::new(KEY).unwrap();
        let opened =
            Notification::Store(StoreChange { opened: true, owner: Some("ShopOwner".into()), balance: Some(1_000) });
        let closed = Notification::Store(StoreChange { opened: false, owner: None, balance: None });
        let requests = phone.requests(&[opened, sale("Arrow", "Wina", 1, None), closed], 0.0);
        let messages: Vec<Value> = requests.iter().map(body).collect();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["title"], "Store detected");
        assert_eq!(messages[0]["body"], "MapleLens is watching ShopOwner's store. Current mesos: 1,000.");
        assert_eq!(messages[0]["tag"], STORE_TAG);
        assert_eq!(messages[1]["body"], "Your store is no longer open.");
        assert_eq!(messages[2]["title"], "Sold: Arrow");
        assert!(phone.requests(&[], 0.0).is_empty());
    }

    #[test]
    fn long_text_is_cut_for_the_relay() {
        let phone = Phone::new(KEY).unwrap();
        let message = body(&phone.requests(&[sale(&"x".repeat(400), "Wina", 1, None)], 0.0)[0]);
        assert_eq!(message["title"].as_str().unwrap().chars().count(), TITLE_CHARS);
    }
}
