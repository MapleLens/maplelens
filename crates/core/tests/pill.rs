//! The status pill's choice: which status, and only once its cause has held.

use maplelens_core::mapinfo::Location;
use maplelens_core::pill::{Facts, Pill, PillPicker};
use maplelens_core::render::status_pill::text_for;
use maplelens_core::shop::ShopStatus;
use maplelens_core::training::watcher::{HudSeen, Training};

const FIELD: &str = "Ant Tunnel I";

/// A reader that started at 0 and found the bar, its EXP and the minimap last at the times given.
fn training(bar: Option<f64>, exp: Option<f64>, minimap: Option<f64>) -> Training {
    Training {
        seen: HudSeen { since: Some(0.0), bar, exp, minimap },
        location: Some(Location::new("Dungeon", FIELD)),
        ..Training::default()
    }
}

fn facts(training: &Training) -> Facts<'_> {
    Facts { store: ShopStatus::Hidden, exp_card: true, map_drops: true, training: Some(training), no_drops: None }
}

/// The pill at each second from `from` to `to`, the reader seeing what `at(t)` says.
fn run(picker: &mut PillPicker, from: u32, to: u32, at: impl Fn(f64) -> Training) -> Vec<Option<Pill>> {
    (from..=to).map(|t| t as f64).map(|t| picker.pick(&facts(&at(t)), t)).collect()
}

#[test]
fn a_reader_seeing_everything_says_nothing() {
    let mut picker = PillPicker::default();
    let pills = run(&mut picker, 0, 60, |t| training(Some(t), Some(t), Some(t)));
    assert!(pills.iter().all(Option::is_none));
}

#[test]
fn a_window_over_the_bar_is_told_after_five_seconds_and_gone_soon_after() {
    let mut picker = PillPicker::default();
    run(&mut picker, 0, 20, |t| training(Some(t), Some(t), Some(t)));
    // Covered from 21: no bar, no EXP; the minimap still in view.
    let covered = run(&mut picker, 21, 40, |t| training(Some(20.0), Some(20.0), Some(t)));
    assert_eq!(covered[..4], [None; 4], "one to four seconds may be a missed read");
    assert!(covered[4..].iter().all(|p| *p == Some(Pill::ExpBarHidden)), "{covered:?}");
    // Moved off at 41: the hint lingers a moment, then goes.
    let moved = run(&mut picker, 41, 45, |t| training(Some(t), Some(t), Some(t)));
    assert_eq!(moved, [Some(Pill::ExpBarHidden), Some(Pill::ExpBarHidden), None, None, None]);
}

#[test]
fn a_bar_found_without_its_numbers_says_so() {
    let mut picker = PillPicker::default();
    let pills = run(&mut picker, 0, 12, |t| training(Some(t), Some(0.0), Some(t)));
    assert_eq!(pills.last().copied().flatten(), Some(Pill::ExpNumbersHidden));
}

#[test]
fn one_good_read_under_a_window_does_not_blink() {
    let mut picker = PillPicker::default();
    run(&mut picker, 0, 20, |t| training(None, Some(0.0), Some(t)));
    // One read at 21 makes the EXP fresh for that look only.
    let pills = run(&mut picker, 21, 23, |t| training(None, Some(21.0f64.min(t)), Some(t)));
    assert!(pills.iter().all(|p| *p == Some(Pill::ExpBarHidden)), "{pills:?}");
}

#[test]
fn no_bar_without_the_minimap_is_not_a_window_over_it() {
    // The Cash Shop, the login screen: neither is there.
    let mut picker = PillPicker::default();
    let pills = run(&mut picker, 0, 60, |_| training(None, None, None));
    assert!(pills.iter().all(Option::is_none), "{pills:?}");
}

#[test]
fn a_town_or_the_exp_card_off_says_nothing_of_the_bar() {
    let mut picker = PillPicker::default();
    let mut town = training(None, None, Some(0.0));
    town.location = Some(Location::new("Victoria Road", "Henesys"));
    for t in 0..30 {
        let mut town = town.clone();
        town.seen.minimap = Some(t as f64);
        assert_eq!(picker.pick(&facts(&town), t as f64), None);
    }
    let mut picker = PillPicker::default();
    for t in 0..30 {
        let field = training(None, None, Some(t as f64));
        let off = Facts { exp_card: false, ..facts(&field) };
        assert_eq!(picker.pick(&off, t as f64), None);
    }
}

#[test]
fn a_closed_minimap_is_told_for_thirty_seconds_each_time() {
    let mut picker = PillPicker::default();
    run(&mut picker, 0, 10, |t| training(Some(t), Some(t), Some(t)));
    let closed = run(&mut picker, 11, 70, |t| training(Some(t), Some(t), Some(10.0)));
    let shown: Vec<u32> = (11..=70).zip(&closed).filter(|(_, p)| p.is_some()).map(|(t, _)| t).collect();
    assert!(closed.iter().flatten().all(|p| *p == Pill::MinimapClosed));
    // Missing from 10: told from 20 to 49, and the linger.
    assert_eq!((shown[0], *shown.last().unwrap()), (20, 51), "{shown:?}");
    // Opened and closed again: told again.
    run(&mut picker, 71, 75, |t| training(Some(t), Some(t), Some(t)));
    let again = run(&mut picker, 76, 90, |t| training(Some(t), Some(t), Some(75.0)));
    assert_eq!(again.last().copied().flatten(), Some(Pill::MinimapClosed));
}

#[test]
fn the_store_comes_first() {
    let mut picker = PillPicker::default();
    let covered = training(None, None, Some(30.0));
    let store = Facts { store: ShopStatus::NeedSoldWindow, ..facts(&covered) };
    assert_eq!(picker.pick(&store, 30.0), Some(Pill::Store(ShopStatus::NeedSoldWindow)));
}

#[test]
fn a_map_with_no_drops_is_said_once() {
    let mut picker = PillPicker::default();
    let seen = |t: f64| training(Some(t), Some(t), Some(t));
    let pills: Vec<_> = (0..40)
        .map(|t| {
            let training = seen(t as f64);
            picker.pick(&Facts { no_drops: Some(FIELD), ..facts(&training) }, t as f64)
        })
        .collect();
    let shown: Vec<usize> = pills.iter().enumerate().filter(|(_, p)| p.is_some()).map(|(t, _)| t).collect();
    // Held 10 s (the drops may be downloading), told 8, lingering 2.
    assert_eq!((shown[0], *shown.last().unwrap()), (10, 19), "{shown:?}");
    assert!(pills.iter().flatten().all(|p| *p == Pill::NoDrops));
}

#[test]
fn every_hint_has_words() {
    for pill in [Pill::ExpBarHidden, Pill::ExpNumbersHidden, Pill::MinimapClosed, Pill::NoDrops] {
        assert!(text_for(pill).is_some_and(|t| t.len() < 40), "{pill:?}");
    }
}
