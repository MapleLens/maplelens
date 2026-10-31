//! Which status the pill at the game's bottom-left shows (pure): the store's while our shop window is up, else what
//! keeps training from being read (the EXP bar covered, the minimap closed) or a map whose drops are unknown. One at a
//! time by priority, never rotated (rotating blinks). A hint shows only once its cause has held for a few seconds, the
//! way the shop's statuses are voted, and stays a moment after a cause ends, so one odd frame changes nothing.

use crate::shop::ShopStatus;
use crate::training::towns::is_town;
use crate::training::watcher::Training;

/// The bar is read once a second: five looks without its EXP are a window or a tooltip over it, not a missed read.
const EXP_HOLD_S: f64 = 5.0;
/// Once told, a look more without EXP keeps it: one good read under the window must not take the hint away for the
/// five seconds it takes to hold again.
const EXP_KEEP_S: f64 = 1.5;
/// The bar found after its EXP was last read, this recently: something is over the numbers, not the bar.
const BAR_FRESH_S: f64 = 2.0;
/// The minimap is looked at every two seconds; its title found this recently is on screen. Without it the frame is
/// not the game world (the Cash Shop, the login screen), where no bar is a reason to move nothing.
const MINIMAP_FRESH_S: f64 = 5.0;
/// The EXP read this recently: the player is in the game world, where a closed minimap means no map.
const EXP_FRESH_S: f64 = 3.0;
/// Five looks at the minimap without its title.
const MINIMAP_HOLD_S: f64 = 10.0;
/// Some players close the minimap on purpose: told for this long each time it goes, then left alone.
const MINIMAP_SHOW_S: f64 = 30.0;
/// A map's monsters are fetched in parallel when it is first met: a few seconds with nothing known is a download.
const NO_DROPS_HOLD_S: f64 = 10.0;
/// Nothing to do about it, so it is said once a map.
const NO_DROPS_SHOW_S: f64 = 8.0;
/// A hint whose cause ended stays this long: a cause that comes back at once (one good read under a window) changes
/// nothing on screen.
const LINGER_S: f64 = 2.0;

/// What the pill says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pill {
    Store(ShopStatus),
    /// No status bar found on a field: a window over it.
    ExpBarHidden,
    /// The bar is found, its EXP not read: a tooltip or a window over the numbers.
    ExpNumbersHidden,
    /// No minimap title: the map is unknown, and the drops panel and the towns rule with it.
    MinimapClosed,
    /// The map is known but nothing it drops.
    NoDrops,
}

impl From<ShopStatus> for Pill {
    fn from(status: ShopStatus) -> Self {
        Pill::Store(status)
    }
}

/// What the pill is chosen from.
#[derive(Clone, Copy, Debug)]
pub struct Facts<'a> {
    /// The store watcher's status (Hidden without our shop window).
    pub store: ShopStatus,
    /// The EXP card is switched on.
    pub exp_card: bool,
    /// The map drops panel is open: the player asked what the map drops.
    pub map_drops: bool,
    /// The training reader's latest (None before its first).
    pub training: Option<&'a Training>,
    /// The map being played, when the drops panel knows nothing it drops (not while it is still looking).
    pub no_drops: Option<&'a str>,
}

#[derive(Debug, Default)]
pub struct PillPicker {
    /// The pill shown, and when its cause last held.
    shown: Option<(Pill, f64)>,
    /// The map with no drops known, and since when.
    no_drops: Option<(String, f64)>,
}

impl PillPicker {
    /// The pill at `now` (Unix seconds, the training reader's clock), or None for nothing to say.
    pub fn pick(&mut self, facts: &Facts<'_>, now: f64) -> Option<Pill> {
        match self.cause(facts, now) {
            Some(pill) => self.shown = Some((pill, now)),
            None if self.shown.is_some_and(|(_, at)| now - at > LINGER_S) => self.shown = None,
            None => {}
        }
        self.shown.map(|(pill, _)| pill)
    }

    /// The most important status whose cause holds at `now`.
    fn cause(&mut self, facts: &Facts<'_>, now: f64) -> Option<Pill> {
        if facts.store != ShopStatus::Hidden {
            return Some(Pill::Store(facts.store));
        }
        let training = facts.training?;
        let seen = training.seen;
        let since = seen.since?;
        let missing_for = |at: Option<f64>| now - at.unwrap_or(since);
        let recent = |at: Option<f64>, within: f64| at.is_some_and(|at| now - at <= within);
        let town = training.location.as_ref().is_some_and(|l| is_town(&l.name));
        let told = matches!(self.shown, Some((Pill::ExpBarHidden | Pill::ExpNumbersHidden, _)));
        let hold = if told { EXP_KEEP_S } else { EXP_HOLD_S };
        if facts.exp_card && !town && recent(seen.minimap, MINIMAP_FRESH_S) && missing_for(seen.exp) >= hold {
            let bar = recent(seen.bar, BAR_FRESH_S) && seen.bar > seen.exp;
            return Some(if bar { Pill::ExpNumbersHidden } else { Pill::ExpBarHidden });
        }
        let gone = missing_for(seen.minimap);
        if (facts.exp_card || facts.map_drops)
            && recent(seen.exp, EXP_FRESH_S)
            && (MINIMAP_HOLD_S..MINIMAP_HOLD_S + MINIMAP_SHOW_S).contains(&gone)
        {
            return Some(Pill::MinimapClosed);
        }
        let map = facts.no_drops.filter(|_| facts.map_drops && !town)?;
        if self.no_drops.as_ref().is_none_or(|(m, _)| m != map) {
            self.no_drops = Some((map.to_string(), now));
        }
        let age = self.no_drops.as_ref().map_or(0.0, |(_, at)| now - at);
        (NO_DROPS_HOLD_S..NO_DROPS_HOLD_S + NO_DROPS_SHOW_S).contains(&age).then_some(Pill::NoDrops)
    }
}
