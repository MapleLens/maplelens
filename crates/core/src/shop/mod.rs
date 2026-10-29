//! Store Permit watcher: reads our own shop window (Sold Items list, chat log, meso counter, buttons) from screen
//! pixels and reports sales, visitors, chat and the store opening and closing. Frame in, events out
//! (`watcher::ShopWatcher`); everything it calls is pure.
//!
//! Double counting is the one thing that must never happen: entries are never deduplicated by content
//! (identical sold rows are real sales), see `align`.

pub mod align;
pub mod buttons;
pub mod chatlog;
mod held;
pub mod layout;
pub mod lines;
pub mod markers;
pub mod names;
pub mod observation;
pub mod parse;
pub mod scrollbar;
pub mod soldlist;
pub mod text;
pub mod watcher;

#[cfg(test)]
mod tests;

/// What the watcher reports. Plain data, so the UI side never touches pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShopEvent {
    Sale {
        item: String,
        buyer: String,
        quantity: u64,
        /// What the store's meso counter gained for it, when this was the only sale of the look and the counter
        /// was readable.
        mesos: Option<i64>,
        /// The game's item id, when the name is one of the catalog's.
        item_id: Option<u64>,
    },
    /// The meso balance moved while the Sold Items dialog was closed: something sold, details unknown. The sale
    /// itself is reported as a normal `Sale` when the dialog is opened. `mesos` is None when the digits were
    /// unreadable.
    SaleUnseen {
        mesos: Option<i64>,
    },
    /// The balance rose but every visible sold row looks like the one before, so the number of sales is unknown
    /// (the whole visible list is one repeated row).
    AmbiguousSale,
    PlayerEntered(String),
    PlayerLeft(String),
    ChatMessage(String),
    /// Our own store is open (also reported once when the watcher starts and finds it already open).
    StoreOpened,
    StoreClosed,
    /// A list no longer lines up with the last one seen (scrolled, covered, or more than a page of news).
    LostSync(ListKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Sold,
    Chat,
}

impl ShopEvent {
    /// One console / log line.
    pub fn describe(&self) -> String {
        match self {
            Self::Sale { item, buyer, quantity, mesos, .. } => {
                let received = mesos.map_or(String::new(), |m| format!("  (+{m} mesos)"));
                format!("SALE      {item} x{quantity}  to {buyer}{received}")
            }
            Self::SaleUnseen { mesos } => {
                let gained = mesos.map_or("meso balance changed".to_string(), |m| format!("+{m} mesos"));
                format!("SALE?     {gained} but the Sold Items window is closed; open it to see what sold")
            }
            Self::AmbiguousSale => {
                "SALE?     meso balance rose but the sold list looks unchanged (repeat of the same sale?)".into()
            }
            Self::PlayerEntered(name) => format!("ENTERED   {name}"),
            Self::PlayerLeft(name) => format!("LEFT      {name}"),
            Self::ChatMessage(text) => format!("CHAT      {text}"),
            Self::StoreOpened => "OPENED    your store is open; watching it".into(),
            Self::StoreClosed => "CLOSED    your store is no longer open".into(),
            Self::LostSync(kind) => {
                let list = if *kind == ListKind::Sold { "sold" } else { "chat" };
                format!("LOST SYNC the {list} list no longer lines up with the last one; something may be missed")
            }
        }
    }

    /// What happened without whom or what: for a log a player may send, which must not carry other players' names.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sale { .. } => "sale",
            Self::SaleUnseen { .. } => "sale while Sold Items was closed",
            Self::AmbiguousSale => "sale, count unknown",
            Self::PlayerEntered(_) => "visitor entered",
            Self::PlayerLeft(_) => "visitor left",
            Self::ChatMessage(_) => "chat line",
            Self::StoreOpened => "store opened",
            Self::StoreClosed => "store closed",
            Self::LostSync(ListKind::Sold) => "sold list lost sync",
            Self::LostSync(ListKind::Chat) => "chat lost sync",
        }
    }

    /// Cases worth keeping a screenshot of when something looks off.
    pub fn wants_evidence(&self) -> bool {
        matches!(self, Self::LostSync(_) | Self::AmbiguousSale)
    }
}

/// What the watcher knows about the store besides its events: whose it is and what is in the till.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShopContext {
    /// The player who opened the store (the name plate under their character in the shop window).
    pub owner: Option<String>,
    /// The meso counter's last reading.
    pub balance: Option<i64>,
}

/// What the watcher can tell the player about its own state: the status pill shows exactly this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShopStatus {
    /// Not in our own store: nothing to say.
    Hidden,
    /// Our shop window, but the store is not open (yet / any more).
    NotOpen,
    /// Open, Sold Items and chat readable: everything is detected.
    Watching,
    /// Open, but the Sold Items window is not on screen.
    NeedSoldWindow,
    /// A list is scrolled away from its newest end.
    Scrolled,
    /// Something lies over a part we need to read.
    Covered,
}

/// OpenCV's 8-bit HSV of an RGB pixel: hue 0..180 (degrees / 2), saturation and value 0..255.
pub(crate) fn hsv([r, g, b]: [u8; 3]) -> (u8, u8, u8) {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    let v = r.max(g).max(b);
    let delta = v - r.min(g).min(b);
    let s = if v > 0.0 { delta * 255.0 / v } else { 0.0 };
    let mut h = if delta == 0.0 {
        0.0
    } else if v == r {
        60.0 * (g - b) / delta
    } else if v == g {
        120.0 + 60.0 * (b - r) / delta
    } else {
        240.0 + 60.0 * (r - g) / delta
    };
    if h < 0.0 {
        h += 360.0;
    }
    ((h / 2.0).round() as u8 % 180, s.round() as u8, v as u8)
}
