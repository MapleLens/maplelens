//! From what the shop watcher reports to what is worth telling: sales, and the store being detected or closing. Pure, so
//! the Windows app and the browser say exactly the same things.
//!
//! A store found open is announced once the owner's name has been read (it takes a few looks) or after
//! `OWNER_WAIT_S` without it, whichever comes first; a store that closes before it was ever announced is not
//! announced either (a window glimpsed and gone).

use super::{Notification, Sale, StoreChange};
use crate::shop::{ShopContext, ShopEvent};

/// How long a detected store waits for its owner's name before being announced without it.
pub const OWNER_WAIT_S: f64 = 20.0;

#[derive(Clone, Debug, Default)]
pub struct Announcer {
    /// Since when a store has been found open but not announced.
    waiting: Option<f64>,
    announced: bool,
    owner: Option<String>,
}

impl Announcer {
    /// What `events` (and the time passing) make worth telling. Call with every watcher update, events or none.
    pub fn feed(&mut self, events: &[ShopEvent], context: &ShopContext, now: f64) -> Vec<Notification> {
        let mut out = Vec::new();
        if context.owner.is_some() {
            self.owner.clone_from(&context.owner);
        }
        for event in events {
            match event {
                ShopEvent::StoreOpened => {
                    self.waiting = Some(now);
                    self.announced = false;
                    self.owner = context.owner.clone();
                }
                ShopEvent::StoreClosed => {
                    if self.announced {
                        out.push(Notification::Store(StoreChange {
                            opened: false,
                            owner: self.owner.clone(),
                            balance: context.balance,
                        }));
                    }
                    self.waiting = None;
                    self.announced = false;
                }
                _ => {}
            }
        }
        if let Some(since) = self.waiting
            && (self.owner.is_some() || now - since >= OWNER_WAIT_S)
        {
            out.push(Notification::Store(StoreChange {
                opened: true,
                owner: self.owner.clone(),
                balance: context.balance,
            }));
            self.waiting = None;
            self.announced = true;
        }
        out.extend(sales(events, context));
        out
    }
}

/// One notification per sale, each with the store's owner, and the counter's reading on the last (it is the balance
/// after all of them).
pub fn sales(events: &[ShopEvent], context: &ShopContext) -> Vec<Notification> {
    let mut sales: Vec<Sale> = events
        .iter()
        .filter_map(|event| match event {
            ShopEvent::Sale { item, buyer, quantity, mesos, item_id } => Some(Sale {
                item: item.clone(),
                buyer: buyer.clone(),
                quantity: *quantity,
                mesos: *mesos,
                item_id: *item_id,
                store: context.owner.clone(),
                balance: None,
            }),
            _ => None,
        })
        .collect();
    if let Some(last) = sales.last_mut() {
        last.balance = context.balance;
    }
    sales.into_iter().map(Notification::Sale).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(owner: Option<&str>, balance: Option<i64>) -> ShopContext {
        ShopContext { owner: owner.map(str::to_string), balance }
    }

    fn stores(notes: &[Notification]) -> Vec<(bool, Option<String>)> {
        notes
            .iter()
            .filter_map(|n| match n {
                Notification::Store(s) => Some((s.opened, s.owner.clone())),
                Notification::Sale(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_store_is_announced_when_its_owner_is_known_and_again_when_it_closes() {
        let mut announcer = Announcer::default();
        assert!(announcer.feed(&[ShopEvent::StoreOpened], &context(None, None), 0.0).is_empty());
        assert!(announcer.feed(&[], &context(None, Some(100)), 5.0).is_empty(), "waits for the owner's name");
        let opened = announcer.feed(&[], &context(Some("ShopOwner"), Some(100)), 8.0);
        assert_eq!(stores(&opened), [(true, Some("ShopOwner".into()))]);
        assert!(announcer.feed(&[], &context(Some("ShopOwner"), Some(100)), 9.0).is_empty(), "only once");
        let closed = announcer.feed(&[ShopEvent::StoreClosed], &context(Some("ShopOwner"), Some(500)), 100.0);
        assert_eq!(stores(&closed), [(false, Some("ShopOwner".into()))]);
    }

    #[test]
    fn without_a_name_it_is_announced_anyway_after_a_while() {
        let mut announcer = Announcer::default();
        announcer.feed(&[ShopEvent::StoreOpened], &context(None, None), 0.0);
        assert!(announcer.feed(&[], &context(None, None), OWNER_WAIT_S - 1.0).is_empty());
        assert_eq!(stores(&announcer.feed(&[], &context(None, None), OWNER_WAIT_S)), [(true, None)]);
    }

    #[test]
    fn a_store_that_closes_before_it_was_announced_is_not_announced_at_all() {
        let mut announcer = Announcer::default();
        announcer.feed(&[ShopEvent::StoreOpened], &context(None, None), 0.0);
        assert!(announcer.feed(&[ShopEvent::StoreClosed], &context(None, None), 3.0).is_empty());
        assert!(announcer.feed(&[], &context(None, None), 60.0).is_empty());
    }

    #[test]
    fn a_new_store_after_a_closed_one_is_announced_again() {
        let mut announcer = Announcer::default();
        let known = context(Some("ShopOwner"), None);
        announcer.feed(&[ShopEvent::StoreOpened], &known, 0.0);
        announcer.feed(&[ShopEvent::StoreClosed], &known, 50.0);
        let again = announcer.feed(&[ShopEvent::StoreOpened], &known, 200.0);
        assert_eq!(stores(&again), [(true, Some("ShopOwner".into()))]);
    }

    #[test]
    fn sales_come_with_the_store_and_the_last_one_with_the_balance() {
        let sale = |item: &str, mesos| ShopEvent::Sale {
            item: item.into(),
            buyer: "Tikko".into(),
            quantity: 2,
            mesos,
            item_id: Some(2_000_000),
        };
        let notes = sales(
            &[sale("A", Some(100)), ShopEvent::StoreOpened, sale("B", None)],
            &context(Some("ShopOwner"), Some(39_311)),
        );
        let sales: Vec<&Sale> =
            notes.iter().filter_map(|n| if let Notification::Sale(s) = n { Some(s) } else { None }).collect();
        assert_eq!(sales.len(), 2);
        assert!(sales.iter().all(|s| s.store.as_deref() == Some("ShopOwner")));
        assert_eq!((sales[0].balance, sales[1].balance), (None, Some(39_311)));
        assert_eq!((sales[0].mesos, sales[1].mesos), (Some(100), None));
    }
}
