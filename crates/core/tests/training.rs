//! Training: the EXP read off the status bar in both UI sizes, the session it builds, and the party quest helper.

mod common;

use common::fixture;
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::{GrayImage, RgbImage};
use maplelens_core::mapinfo::Location;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::placement::{place_beside_bar, place_in_corner};
use maplelens_core::render::theme;
use maplelens_core::render::training_card::{
    Button, CARD_WIDTH, FALLBACK_WIDTH, MAX_STRIP_WIDTH, MIN_STRIP_WIDTH, STRIP_HEIGHT, Show, Strip, drops_button,
    has_content, on_drops_button, render_training_card, strip_width,
};
use maplelens_core::settings::Corner;
use maplelens_core::training::kpq::{self, Run};
use maplelens_core::training::potions;
use maplelens_core::training::session::{GRAPH_S, GRAPH_STEP_S, Tracker};
use maplelens_core::training::watcher::{Training, TrainingWatcher};
use maplelens_core::vision::gauge::{ExpReading, read_exp, read_exp_text};
use maplelens_core::vision::hud::find_bar;
use maplelens_core::vision::item_icon::IconSet;
use maplelens_core::vision::minimap::find_title;
use maplelens_core::vision::quickslot::{self, CountFont};
use maplelens_core::vision::reader::{ImageOcr, OcrAnswer};

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn exp_is_read_in_every_ui_size() {
    for (name, exp, hundredths) in [
        ("status_bar_ideal_ratio_1920x1200.png", 12_128, 5999),
        ("status_bar_normal_1920x1200.png", 12_128, 5999),
        ("hud_status_bar.png", 1_613, 798),
        ("status_bar_exp_98_percent_1920x1200.png", 19_945, 9866),
    ] {
        let strip = fixture(name);
        let bar = find_bar(&strip).unwrap_or_else(|| panic!("{name}: no bar"));
        let text = read_exp_text(&strip, bar);
        assert_eq!(read_exp(&strip, bar), Some(ExpReading { exp, hundredths }), "{name}: {text:?}");
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_covered_gauge_is_no_reading() {
    let mut strip = fixture("status_bar_normal_1920x1200.png");
    let bar = find_bar(&strip).unwrap();
    // A window dragged over the gauge: flat colour where the numbers were.
    for y in bar.top + 40..bar.top + 90 {
        for x in bar.x + 700..bar.x + 950 {
            strip.set_pixel(x, y, [236, 236, 236]);
        }
    }
    assert_eq!(read_exp(&strip, bar), None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_minimap_title_is_cut_into_its_two_lines() {
    for name in ["minimap_free_market_1920x1200.png", "minimap_free_market_8_1920x1200.png"] {
        let title = find_title(&fixture(name), 1.0).unwrap_or_else(|| panic!("{name}: no minimap"));
        // "Hidden Street" and "Free Market <n>", bold capitals ~13 px tall at UI scale 1, after the map's icon.
        for (line, width) in [(&title.street, 124), (&title.map, 144)] {
            assert!((16..=21).contains(&line.height), "{name}: {}x{}", line.width, line.height);
            assert!(line.width.abs_diff(width) <= 12, "{name}: {}x{}", line.width, line.height);
            // Ink on paper: the letters dark, most of the line white.
            let dark = line.data.iter().filter(|&&v| v < 100).count();
            assert!(dark > 100 && dark < line.data.len() / 2, "{name}: {dark} dark of {}", line.data.len());
        }
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_minimap_title_is_found_at_ideal_ratio() {
    // The UI at 1x (0.64 of the measured size): crisp letters 9 rows tall, a map icon on the left.
    let title = find_title(&fixture("minimap_ideal_ratio_1920x1200.png"), 0.64).expect("a minimap");
    for line in [&title.street, &title.map] {
        assert!((11..=15).contains(&line.height), "{}x{}", line.width, line.height);
        assert!((60..=100).contains(&line.width), "{}x{}", line.width, line.height);
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn no_minimap_is_no_title() {
    assert_eq!(find_title(&fixture("status_bar_normal_1920x1200.png"), 1.0), None);
    assert_eq!(find_title(&RgbImage::filled(400, 120, [153, 187, 204]), 1.0), None);
}

fn read(exp: u64, hundredths: u32) -> Option<ExpReading> {
    Some(ExpReading { exp, hundredths })
}

/// Feed the tracker the same read twice a second apart (a read counts once two agree), from `t`.
fn feed(tracker: &mut Tracker, t: &mut f64, exp: u64, hundredths: u32) {
    for _ in 0..2 {
        tracker.observe(*t, read(exp, hundredths));
        *t += 1.0;
    }
}

const NEEDED: u64 = 20_000;

fn share(exp: u64) -> u32 {
    (exp * 10_000 / NEEDED) as u32
}

#[test]
fn gains_and_rates_come_from_the_bar() {
    let mut tracker = Tracker::new();
    let mut t = 1_000_000.0;
    // 100 EXP every 2 s for four minutes: 3000 a minute.
    for step in 0..120 {
        let exp = 5_000 + step * 100;
        feed(&mut tracker, &mut t, exp, share(exp));
    }
    let view = tracker.view(t, 10.0);
    assert_eq!(view.session.gained, 11_900);
    assert!(view.live);
    let average = view.average_per_min.unwrap();
    assert!((average - 3_000.0).abs() < 60.0, "{average}");
    assert!((view.last_minute.unwrap() - 3_000.0).abs() <= 100.0, "{:?}", view.last_minute);
    assert!(view.session.best_minute >= 3_000);
    // 16,900 of 20,000: 3,100 to go at 3,000 a minute.
    let to_level = view.to_level_s.unwrap();
    assert!((to_level - 62.0).abs() < 4.0, "{to_level}");
    assert!((view.percent_per_hour.unwrap() - 900.0).abs() < 20.0, "{:?}", view.percent_per_hour);
}

#[test]
fn a_level_up_adds_the_rest_of_the_old_level() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 19_000, share(19_000));
    // The next level is a 25,000 EXP one.
    feed(&mut tracker, &mut t, 300, 120);
    let view = tracker.view(t, 10.0);
    assert_eq!(view.session.levels, 1);
    assert_eq!(view.session.gained, 1_000 + 300);
}

#[test]
fn a_real_level_up_from_twenty_to_twenty_one() {
    // Read off the same character a day apart (19,945 [98.66%] at level 20, 8,385 [28.93%] at 21).
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 19_945, 9_866);
    feed(&mut tracker, &mut t, 8_385, 2_893);
    let session = tracker.view(t, 10.0).session;
    assert_eq!(session.levels, 1);
    assert_eq!(session.gained, (20_216 - 19_945) + 8_385);
}

#[test]
fn dying_loses_exp_but_gains_none() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 12_128, 5_999);
    feed(&mut tracker, &mut t, 11_000, 5_440);
    let session = tracker.view(t, 10.0).session;
    assert_eq!((session.gained, session.lost, session.levels), (0, 1_128, 0));
}

#[test]
fn switching_characters_counts_nothing() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    // A 20,216 EXP level at 98.66%, then a character far ahead (a 500,000 EXP level) at 30%...
    feed(&mut tracker, &mut t, 19_945, 9_866);
    feed(&mut tracker, &mut t, 150_000, 3_000);
    // ...then one at the same share of a smaller level.
    feed(&mut tracker, &mut t, 4_000, 3_000);
    let session = tracker.view(t, 10.0).session;
    assert_eq!((session.gained, session.levels, session.lost), (0, 0, 0));
    // The new character's gains count as usual.
    feed(&mut tracker, &mut t, 4_100, 3_075);
    assert_eq!(tracker.view(t, 10.0).session.gained, 100);
}

#[test]
fn a_single_misread_never_counts() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 1_613, 798);
    tracker.observe(t, read(7_613, 798)); // once, then back to the real value
    feed(&mut tracker, &mut t, 1_613, 798);
    assert_eq!(tracker.view(t, 10.0).session.gained, 0);
}

#[test]
fn paused_time_and_exp_do_not_count() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    tracker.set_paused(true);
    feed(&mut tracker, &mut t, 2_000, share(2_000));
    tracker.set_paused(false);
    feed(&mut tracker, &mut t, 2_100, share(2_100));
    let view = tracker.view(t, 10.0);
    assert_eq!(view.session.gained, 100);
}

#[test]
fn a_minute_without_exp_ends_the_session() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_100, share(1_100));
    // Over a minute in town: the session goes into the history and the card has nothing to show.
    for _ in 0..40 {
        feed(&mut tracker, &mut t, 1_100, share(1_100));
    }
    let view = tracker.view(t, 10.0);
    assert_eq!(tracker.history().len(), 1);
    assert_eq!(tracker.history()[0].gained, 100);
    assert_eq!((view.session.gained, view.session.active_s), (0, 0.0));
    // Back to training: a new session from its first EXP.
    feed(&mut tracker, &mut t, 1_400, share(1_400));
    let session = tracker.view(t, 10.0).session;
    assert_eq!(session.gained, 300);
    assert!(session.started >= t - 2.0, "{} vs {t}", session.started);
    assert_eq!(tracker.history().len(), 1);
}

#[test]
fn moving_to_another_map_ends_the_session() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    tracker.set_map(Some("Ant Tunnel I".into()), t);
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_500, share(1_500));
    // The same map read again (OCR's spacing aside), or not read for a moment: the session goes on.
    tracker.set_map(Some("Ant  Tunnel I".into()), t);
    tracker.set_map(None, t);
    assert!(tracker.history().is_empty());
    assert_eq!(tracker.view(t, 10.0).session.gained, 500);
    // Another map: the session goes into the history and the card has nothing to show until the next EXP here.
    tracker.set_map(Some("Ant Tunnel II".into()), t);
    assert_eq!(tracker.history().len(), 1);
    assert_eq!(tracker.history()[0].maps[0].map, "Ant Tunnel I");
    assert_eq!(tracker.view(t, 10.0).session.gained, 0);
    feed(&mut tracker, &mut t, 1_800, share(1_800));
    let session = tracker.view(t, 10.0).session;
    assert_eq!(session.gained, 300);
    assert_eq!(session.maps[0].map, "Ant Tunnel II");
    // Paused, a move keeps the session.
    tracker.set_paused(true);
    tracker.set_map(Some("Ant Tunnel III".into()), t);
    assert_eq!(tracker.history().len(), 1);
}

/// EXP gained in a town (a quest handed in) starts no session and shows no card; the same on a field does.
#[test]
fn exp_in_a_town_is_no_training() {
    let both = Show { exp: true, quest: true, strip: None };
    let card = |tracker: &mut Tracker, t: f64| {
        let training = Training { exp: tracker.view(t, 10.0), ..Training::default() };
        (has_content(&training, both), render_training_card(common::fonts(), &training, both, t, 1.0).is_some())
    };
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    tracker.set_map(Some("Kerning City".into()), t);
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_233, share(1_233));
    let view = tracker.view(t, 10.0);
    assert!(view.live);
    assert_eq!((view.session.gained, view.session.active_s), (0, 0.0));
    assert_eq!(card(&mut tracker, t), (false, false));
    // Out to a field: the town's EXP is not counted there, the field's is, and the card shows.
    tracker.set_map(Some("Kerning City Middle Forest I".into()), t);
    feed(&mut tracker, &mut t, 1_233, share(1_233));
    feed(&mut tracker, &mut t, 1_333, share(1_333));
    assert_eq!(tracker.view(t, 10.0).session.gained, 100);
    assert_eq!(card(&mut tracker, t), (true, true));
    assert!(tracker.history().is_empty(), "the town made no session");
}

#[test]
fn walking_into_town_ends_the_session() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    tracker.set_map(Some("Henesys Hunting Ground I".into()), t);
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_500, share(1_500));
    tracker.set_map(Some("Henesys".into()), t);
    assert_eq!(tracker.history().len(), 1);
    assert_eq!(tracker.history()[0].gained, 500);
    // A quest handed in there that levels the character up: nothing counts, and the new level's size is learnt.
    feed(&mut tracker, &mut t, 1_900, share(1_900));
    feed(&mut tracker, &mut t, 400, 100);
    let view = tracker.view(t, 10.0);
    assert_eq!((view.session.gained, view.session.levels), (0, 0));
    // Back on a field, the next kill counts from the town's last reading.
    tracker.set_map(Some("Henesys Hunting Ground II".into()), t);
    feed(&mut tracker, &mut t, 1_200, 300);
    let session = tracker.view(t, 10.0).session;
    assert_eq!((session.gained, session.levels), (800, 0));
    assert_eq!(tracker.history().len(), 1);
}

#[test]
fn a_town_read_late_ends_what_its_exp_started() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    // A quest's EXP before the minimap was read: a field for all the tracker knows.
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_500, share(1_500));
    assert_eq!(tracker.view(t, 10.0).session.gained, 500);
    tracker.set_map(Some("Free Market <3>".into()), t);
    assert_eq!(tracker.view(t, 10.0).session.gained, 0);
}

#[test]
fn an_unread_map_counts_as_a_field() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    tracker.set_map(Some("Ellinia".into()), t);
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    // The minimap closed long enough for the watcher to forget the town: EXP counts, so training is never hidden.
    tracker.set_map(None, t);
    feed(&mut tracker, &mut t, 1_100, share(1_100));
    assert_eq!(tracker.view(t, 10.0).session.gained, 100);
}

#[test]
fn potions_bought_in_a_town_do_not_count() {
    let mut tracker = Tracker::new();
    tracker.set_map(Some("Ellinia".into()), 0.0);
    let totals = |n| std::collections::HashMap::from([(2_000_000, n)]);
    tracker.observe_potions(0.0, &totals(10), |_| "Red Potion".into());
    tracker.observe_potions(1.0, &totals(110), |_| "Red Potion".into());
    assert!(tracker.view(1.0, 10.0).session.potions.is_empty());
}

#[test]
fn a_pause_holds_the_session_however_long() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_100, share(1_100));
    tracker.set_paused(true);
    for _ in 0..100 {
        feed(&mut tracker, &mut t, 1_100, share(1_100));
    }
    tracker.set_paused(false);
    feed(&mut tracker, &mut t, 1_100, share(1_100));
    feed(&mut tracker, &mut t, 1_200, share(1_200));
    assert!(tracker.history().is_empty());
    assert_eq!(tracker.view(t, 10.0).session.gained, 200);
}

#[test]
fn sessions_end_into_the_history_and_survive_a_restart() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    tracker.set_map(Some("Ant Tunnel I".into()), t);
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_500, share(1_500));
    tracker.new_session(t);
    assert_eq!(tracker.history().len(), 1);
    assert_eq!(tracker.history()[0].gained, 500);
    assert_eq!(tracker.history()[0].maps[0].map, "Ant Tunnel I");
    assert_eq!(tracker.history()[0].maps[0].gained, 500);
    let restored = Tracker::from_json(&tracker.to_json());
    assert_eq!(restored.history(), tracker.history());
    assert_eq!(Tracker::from_json("not json").history().len(), 0);
}

#[test]
fn coming_back_hours_later_starts_a_new_session() {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    feed(&mut tracker, &mut t, 1_500, share(1_500));
    t += 4.0 * 3_600.0;
    feed(&mut tracker, &mut t, 9_000, share(9_000));
    assert_eq!(tracker.history().len(), 1);
    assert_eq!(tracker.view(t, 10.0).session.gained, 0);
}

/// Answers the minimap's two lines in turn, as OCR would.
struct Lines(Vec<&'static str>, usize);

impl ImageOcr for Lines {
    fn recognize_image(&mut self, _image: &GrayImage) -> OcrAnswer {
        self.1 += 1;
        OcrAnswer::Text(self.0[(self.1 - 1) % self.0.len()].to_string())
    }
}

/// A 1920x1200 game: the minimap in the corner, the status bar at the bottom.
fn game_frame(minimap: &str, bar: &str) -> RgbImage {
    let mut frame = RgbImage::filled(1920, 1200, [40, 60, 90]);
    for (image, top) in [(fixture(minimap), 0), (fixture(bar), 1200 - 130)] {
        for y in 0..image.height {
            for x in 0..image.width {
                frame.set_pixel(x, top + y, image.pixel(x, y));
            }
        }
    }
    frame
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_watcher_reads_the_exp_and_the_map() {
    let frame = game_frame("minimap_free_market_1920x1200.png", "status_bar_exp_98_percent_1920x1200.png");
    let ocr = Lines(vec!["Hidden Street", "Free Market <3>"], 0);
    let mut watcher = TrainingWatcher::new(Box::new(ocr), Tracker::new());
    let bounds = Rect::new(0, 0, 1920, 1200);
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
    for i in 0..8 {
        watcher.step(&mut screen, bounds, 100.0 + i as f64);
    }
    let training = watcher.training(108.0);
    assert_eq!(training.exp.reading, read(19_945, 9_866));
    assert_eq!(training.exp.map.as_deref(), Some("Free Market <3>"));
    // The street comes with it, for the map drops panel's lookup.
    assert_eq!(training.location, Some(Location::new("Hidden Street", "Free Market <3>")));
    assert_eq!(training.quest, None);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_watcher_follows_the_party_quest() {
    let frame = game_frame("minimap_free_market_1920x1200.png", "status_bar_normal_1920x1200.png");
    let ocr = Lines(vec!["Hidden Street", "1st Accompaniment <2nd Stage>"], 0);
    let mut watcher = TrainingWatcher::new(Box::new(ocr), Tracker::new());
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
    for i in 0..8 {
        watcher.step(&mut screen, Rect::new(0, 0, 1920, 1200), i as f64);
    }
    let quest = watcher.training(8.0).quest.expect("in the quest");
    assert_eq!(kpq::stage(quest.stage).name, "Ropes");
}

#[test]
fn the_card_goes_in_its_corner_clear_of_the_minimap_and_the_status_bar() {
    let client = Rect::new(100, 50, 1920, 1200);
    let bar = Rect::new(100 + 560, 50 + 1200 - 68, 800, 68);
    let card = (200, 300);
    let at = |corner, minimap| place_in_corner(client, card, corner, 10, Some(bar), minimap);
    assert_eq!(at(Corner::TopRight, None), Some((100 + 1920 - 210, 60)));
    // Beside the status bar: level with it, its foot on the bar's.
    assert_eq!(at(Corner::BottomRight, None), Some((100 + 1920 - 210, 50 + 1200 - 300)));
    assert_eq!(at(Corner::BottomLeft, None), Some((110, 50 + 1200 - 300)));
    // Top left: beside the minimap while it is on screen.
    assert_eq!(at(Corner::TopLeft, None), Some((110, 60)));
    assert_eq!(at(Corner::TopLeft, Some(232)), Some((100 + 232 + 10, 60)));
    // A client so narrow that a bottom card would cover the status bar: above it.
    let narrow = Rect::new(0, 0, 1000, 800);
    let bar = Rect::new(100, 800 - 68, 800, 68);
    assert_eq!(place_in_corner(narrow, card, Corner::BottomLeft, 10, Some(bar), None), Some((10, 800 - 68 - 310)));
    // Never outside the game.
    assert_eq!(place_in_corner(Rect::new(0, 0, 150, 150), card, Corner::TopRight, 10, None, None), None);
}

fn training_after_play() -> Training {
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    for step in 0..60 {
        let exp = 5_000 + step * 100;
        feed(&mut tracker, &mut t, exp, share(exp));
    }
    Training { exp: tracker.view(t, 10.0), ..Training::default() }
}

#[test]
fn the_card_shows_only_what_there_is_and_what_is_wanted() {
    let both = Show { exp: true, quest: true, strip: None };
    assert_eq!(render_training_card(common::fonts(), &Training::default(), both, 0.0, 1.0), None);
    let training = training_after_play();
    let exp = render_training_card(common::fonts(), &training, both, 0.0, 1.0).expect("EXP card");
    // As wide as the design at CARD_SCALE, and translucent like the game's tooltips.
    assert_eq!(exp.width, (CARD_WIDTH * theme::CARD_SCALE).round() as usize);
    assert!(exp.pixel(exp.width / 2, exp.height - 3)[3] < 255);
    assert_eq!(
        render_training_card(common::fonts(), &training, Show { exp: false, quest: true, strip: None }, 0.0, 1.0),
        None
    );
    // The quest's first stage lists every Cloto answer: a taller card.
    let quest = Training { quest: Some(Run { started: 0.0, stage: 0, seen: 10.0 }), ..training };
    let with_quest = render_training_card(common::fonts(), &quest, both, 10.0, 1.0).expect("card");
    assert!(with_quest.height > exp.height + 150, "{} vs {}", with_quest.height, exp.height);
    let quest_only =
        render_training_card(common::fonts(), &quest, Show { exp: false, quest: true, strip: None }, 10.0, 1.0);
    assert!(quest_only.is_some_and(|c| c.height < with_quest.height));
    // Ideal Ratio draws it smaller, like the rest of the UI.
    let small = render_training_card(common::fonts(), &quest, both, 10.0, 0.64).expect("card");
    assert!(small.width < with_quest.width);
}

#[test]
fn the_graph_is_the_pace_of_the_last_minutes_with_its_level_ups() {
    let mut tracker = Tracker::new();
    let mut t = 1_000_000.0;
    feed(&mut tracker, &mut t, 5_000, share(5_000));
    assert!(tracker.view(t, 10.0).graph.rates.is_empty(), "no EXP yet");
    // 100 EXP every 2 s for four minutes: 3000 a minute, every point of it.
    for step in 1..=120 {
        let exp = 5_000 + step * 100;
        feed(&mut tracker, &mut t, exp, share(exp));
    }
    let graph = tracker.view(t, 10.0).graph;
    // A point every 10 s from 10 s after the first EXP: none climbs from nothing.
    assert_eq!(graph.rates.len(), 23);
    assert!(graph.rates.iter().all(|r| (2_700.0..=3_400.0).contains(r)), "{:?}", graph.rates);
    assert!(graph.level_ups.is_empty());
    // A level up is marked where it happened.
    // The next level is half as big again: 300 of it is 1%.
    feed(&mut tracker, &mut t, 300, 100);
    for _ in 0..40 {
        tracker.observe(t, read(300, 100));
        t += 1.0;
    }
    let graph = tracker.view(t, 10.0).graph;
    assert_eq!(graph.level_ups.len(), 1);
    assert!((39.0..=42.0).contains(&graph.level_ups[0]), "{:?}", graph.level_ups);
    // Ten minutes on, only the graph's span is kept.
    for step in 0..400 {
        let exp = 400 + step * 10;
        feed(&mut tracker, &mut t, exp, (exp / 3) as u32);
    }
    let graph = tracker.view(t, 10.0).graph;
    assert_eq!(graph.rates.len(), (GRAPH_S / GRAPH_STEP_S) as usize + 1);
    assert!(graph.level_ups.is_empty(), "the level up is older than the graph");
}

#[test]
fn the_strip_reaches_the_pup_button_and_is_denser_than_the_tall_card() {
    let training = training_after_play();
    let strip = |drops, width| Show { exp: true, quest: false, strip: Some(Strip { drops, width }) };
    // The Pup quickslot's top above the client's foot, measured on the game's renders: 139 px at 1:1 (UI scale 0.64),
    // 195 at 1080p's Default Ratio (0.9), 217 at 1200p's (1.0).
    for (scale, pup) in [(0.64, 139), (0.9, 195), (1.0, 217)] {
        let card = render_training_card(common::fonts(), &training, strip(None, Some(300)), 0.0, scale).expect("strip");
        assert!(card.height.abs_diff(pup) <= 1, "{} against the Pup button's {pup} at {scale}", card.height);
        assert_eq!(card.width, 300, "as wide as the room it was given");
        assert!(card.height.abs_diff((STRIP_HEIGHT * theme::CARD_SCALE * scale).round() as usize) <= 1);
    }
    let fallback = render_training_card(common::fonts(), &training, strip(None, None), 0.0, 1.0).expect("strip");
    assert_eq!(fallback.width, (FALLBACK_WIDTH * theme::CARD_SCALE).round() as usize);
    // 10% less see-through than the tall card (below the gauge, where nothing else is drawn).
    let card = render_training_card(common::fonts(), &training, strip(None, Some(400)), 0.0, 1.0).expect("strip");
    let tall = render_training_card(common::fonts(), &training, Show { strip: None, ..strip(None, None) }, 0.0, 1.0);
    let tall = tall.expect("tall card");
    let (fill, old) = (card.pixel(card.width / 2, card.height - 4)[3], tall.pixel(tall.width / 2, tall.height - 3)[3]);
    assert!(fill < 255 && fill >= old + 15, "{fill} against {old}");
    // Every width has a layout: narrow, medium and wide each draw the numbers, the graph and the gauge.
    for width in [165, 330, 440] {
        let card = render_training_card(common::fonts(), &training, strip(None, Some(width)), 0.0, 1.0).expect("strip");
        assert_eq!(card.width, width as usize);
        assert_eq!(card.alpha_bbox(), Some((0, 0, card.width, card.height)), "nothing drawn past the frame");
    }
    // The drops' button shows only when offered, and lights up under the pointer and while the panel is open.
    let drawn = [None, Some(Button::Idle), Some(Button::Lit), Some(Button::On)]
        .map(|b| render_training_card(common::fonts(), &training, strip(b, Some(400)), 0.0, 1.0).expect("strip"));
    for (i, a) in drawn.iter().enumerate() {
        assert!(drawn[i + 1..].iter().all(|b| b != a), "button state {i} looks like another");
    }
    // The party quest hangs below the strip.
    let quest = Training { quest: Some(Run { started: 0.0, stage: 3, seen: 10.0 }), ..training };
    let both = Show { quest: true, ..strip(None, Some(400)) };
    let with_quest = render_training_card(common::fonts(), &quest, both, 10.0, 1.0).expect("card");
    assert!(with_quest.height > card.height + 60);
}

#[test]
fn the_strip_fills_the_room_beside_the_status_bar_up_to_16_9() {
    // (client, the UI against 1:1) as the game draws them; the bar is 800 px at 1:1.
    let width = |(w, h): (i32, i32), k: f32| {
        let scale = 0.64 * k;
        strip_width((w, h), (800.0 * k).round() as i32, theme::CARD_SCALE * scale)
    };
    let room = |(w, _): (i32, i32), k: f32| (w - (800.0 * k).round() as i32) / 2;
    // The room each side of the bar, less a pixel or three clear of its outline.
    for (client, k) in [((1920, 1080), 1.40625), ((1366, 768), 1.0), ((1280, 720), 1.0), ((1920, 1200), 1.5625)] {
        let got = width(client, k).expect("room beside the bar") as i32;
        assert!(
            (room(client, k) - 4..=room(client, k) - 2).contains(&got),
            "{got} in {} at {client:?}",
            room(client, k)
        );
    }
    // Wider than 16:9, the strip is as wide as at 16:9 (2560x1080 is 1920x1080 with more sky).
    assert_eq!(width((2560, 1080), 1.40625), width((1920, 1080), 1.40625));
    assert_eq!(width((3440, 1440), 1.875), width((2560, 1440), 1.875));
    // Never wider than at 16:9 in Default Ratio: Ideal Ratio leaves twice that at 1080p, and the strip keeps to the
    // corner, as wide as at 1080p's Default Ratio for its UI size.
    let most = (MAX_STRIP_WIDTH * theme::CARD_SCALE * 0.64).floor() as u32;
    assert_eq!(width((1920, 1080), 1.0), Some(most));
    assert_eq!(width((2560, 1440), 1.0), Some(most));
    let at_1080p = width((1920, 1080), 1.40625).expect("room") as f32 / 1.40625;
    assert!((at_1080p - most as f32).abs() <= 1.5, "{at_1080p} against {most}");
    // 4:3 has a narrow strip; 800x600 has no room at all.
    assert!(width((1024, 768), 1.0).is_some_and(|w| w >= (MIN_STRIP_WIDTH * 0.96) as u32));
    assert_eq!(width((800, 600), 1.0), None);
    assert_eq!(width((1280, 1024), 1024.0 / 768.0), None, "5:4, drawn 768 high and scaled");
}

#[test]
fn the_strip_sits_flush_beside_the_bar_or_above_the_hud() {
    let client = Rect::new(100, 50, 1920, 1080);
    let bar = Rect::new(100 + 397, 50 + 1080 - 95, 1125, 95);
    let card = (394, 195);
    let right = place_beside_bar(client, card, Corner::BottomRight, bar);
    assert_eq!(right, Some((100 + 1920 - 394, 50 + 1080 - 195)));
    assert_eq!(place_beside_bar(client, card, Corner::BottomLeft, bar), Some((100, 50 + 1080 - 195)));
    assert_eq!(place_beside_bar(client, card, Corner::TopRight, bar), None, "nothing beside the bar up there");
    assert_eq!(place_beside_bar(client, (420, 195), Corner::BottomRight, bar), None, "it would reach the bar");
}

#[test]
fn the_drops_button_is_pressed_only_inside_it() {
    for width in [115.0, 290.0, 580.0] {
        let (x, y, w, h) = drops_button(width);
        assert!(on_drops_button(width, x + w / 2.0, y + h / 2.0));
        assert!(on_drops_button(width, x, y));
        assert!(!on_drops_button(width, x + w, y + h / 2.0), "past its right edge");
        assert!(!on_drops_button(width, x - 1.0, y + h / 2.0));
        assert!(!on_drops_button(width, x + w / 2.0, y + h + 0.5), "under it: the numbers");
        assert!(x + w <= width - 4.0 && y >= 3.0 && y + h <= 24.0, "in the header, inside the frame");
    }
}

#[test]
fn the_exp_card_shows_from_the_first_kill_and_its_rates_after_ten_seconds() {
    let both = Show { exp: true, quest: true, strip: None };
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 5_000, share(5_000));
    let before = Training { exp: tracker.view(t, 10.0), ..Training::default() };
    assert!(!has_content(&before, both));
    // The first kill: the card, its rates still to come.
    feed(&mut tracker, &mut t, 5_100, share(5_100));
    let first = Training { exp: tracker.view(t, 10.0), ..Training::default() };
    assert!(has_content(&first, both));
    assert_eq!(first.exp.recent_per_min, None);
    for exp in [5_200, 5_300, 5_400, 5_500] {
        feed(&mut tracker, &mut t, exp, share(exp));
    }
    // Ten seconds in: the rates.
    let later = Training { exp: tracker.view(t, 10.0), ..Training::default() };
    assert!(later.exp.recent_per_min.is_some() && later.exp.percent_per_hour.is_some());
}

/// The counts in each quickslot of a capture (its status bar found first), left to right, top row first.
fn quickslot_counts(name: &str) -> Vec<Option<u32>> {
    let strip = fixture(name);
    let bar = find_bar(&strip).expect("a status bar");
    let font = CountFont::new(quickslot::scale_of(bar));
    (0..quickslot::SLOTS).map(|i| font.read(&strip, quickslot::slot_rect(bar, (0, 0), i))).collect()
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn quickslot_counts_are_read_in_both_ui_sizes() {
    // Ideal Ratio, drawn 1:1: Lemon 16, an empty slot, a skill, Red Potion 30, a chair, a milk carton 10.
    assert_eq!(
        quickslot_counts("quickslot_ideal_ratio_1920x1200.png"),
        [Some(16), None, None, None, None, Some(30), None, Some(10)]
    );
    // Normal, scaled 1.5625x: Blue Potion 0, Red Potion 16, the milk carton 100 (its white shows through the 0s).
    assert_eq!(
        quickslot_counts("quickslot_normal_1920x1200.png"),
        [Some(0), None, None, None, None, Some(16), None, Some(100)]
    );
}

/// The potions and foods maplestory.io lists (`tests/fixtures/icons`: its item list and their icons, kept with the
/// local captures: they are the game's artwork).
fn potion_icons() -> IconSet {
    let dir = common::repo().join("tests/fixtures/icons");
    let list = std::fs::read_to_string(dir.join("consumables.json")).unwrap();
    let mut icons = IconSet::default();
    for (id, name) in potions::candidates(&list) {
        if let Ok(png) = std::fs::read(dir.join(format!("{id}.png"))) {
            icons.add_png(id, &name, &png);
        }
    }
    icons
}

fn quickslot_items(name: &str) -> Vec<Option<String>> {
    let strip = fixture(name);
    let bar = find_bar(&strip).expect("a status bar");
    let k = quickslot::scale_of(bar);
    let mut icons = potion_icons();
    (0..quickslot::SLOTS)
        .map(|i| {
            let slot = quickslot::slot_rect(bar, (0, 0), i);
            let reach = (4.0 * k).ceil() as i32;
            let area = Rect::new(slot.x - reach, slot.y - reach, slot.w + 2 * reach, slot.h + 2 * reach);
            icons.identify(&strip, area, k).and_then(|f| icons.icon(f.id)).map(|i| i.name.clone())
        })
        .collect()
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn quickslot_potions_are_known_by_their_icons() {
    let some = |s: &str| Some(s.to_string());
    assert_eq!(
        quickslot_items("quickslot_ideal_ratio_1920x1200.png"),
        [some("Lemon"), None, None, None, None, some("Red Potion"), None, None]
    );
    assert_eq!(
        quickslot_items("quickslot_normal_1920x1200.png"),
        [some("Blue Potion"), None, None, None, None, some("Red Potion"), None, None]
    );
}

#[test]
fn potions_used_and_picked_up_follow_the_quickslot_totals() {
    const RED: u32 = 2_000_000;
    const WHITE: u32 = 2_000_002;
    let name = |id: u32| if id == RED { "Red Potion".to_string() } else { "White Potion".to_string() };
    let totals = |pairs: &[(u32, u32)]| pairs.iter().copied().collect::<std::collections::HashMap<u32, u32>>();
    let mut tracker = Tracker::new();
    let mut t = 0.0;
    feed(&mut tracker, &mut t, 1_000, share(1_000));
    tracker.observe_potions(t, &totals(&[(RED, 30), (WHITE, 100)]), name);
    feed(&mut tracker, &mut t, 1_100, share(1_100));
    tracker.observe_potions(t, &totals(&[(RED, 26), (WHITE, 100)]), name);
    // A White Potion stack dragged out of the quickslots for a moment says nothing.
    tracker.observe_potions(t, &totals(&[(RED, 25)]), name);
    tracker.observe_potions(t, &totals(&[(RED, 31), (WHITE, 100)]), name);
    let potions = tracker.view(t, 10.0).session.potions;
    assert_eq!(potions.len(), 1);
    assert_eq!((potions[0].name.as_str(), potions[0].used, potions[0].picked), ("Red Potion", 5, 6));
    // Paused, nothing counts; resumed, it counts from the totals as they are.
    tracker.set_paused(true);
    tracker.observe_potions(t, &totals(&[(RED, 10)]), name);
    tracker.set_paused(false);
    tracker.observe_potions(t, &totals(&[(RED, 9)]), name);
    assert_eq!(tracker.view(t, 10.0).session.potions[0].used, 6);
    // A new map, a new session: the counts start over, the totals carry on.
    tracker.set_map(Some("Ant Tunnel I".into()), t);
    tracker.set_map(Some("Ant Tunnel II".into()), t);
    assert!(tracker.view(t, 10.0).session.potions.is_empty());
    tracker.observe_potions(t, &totals(&[(RED, 7)]), name);
    assert_eq!(tracker.view(t, 10.0).session.potions[0].used, 2);
    // Saved sessions from before potions were counted still load.
    let old = r#"{"session":{"started":0,"ended":0,"active_s":0,"gained":0,"percent":0,"levels":0,"lost":0,"best_minute":0,"maps":[]},"history":[],"paused":false}"#;
    assert_eq!(Tracker::from_json(old).session().potions, []);
}

/// A 1920x1200 frame whose bottom is the Ideal Ratio quickslot capture, with `red` drawn as the Red Potion's count.
fn quickslot_frame(red: u32) -> RgbImage {
    let capture = fixture("quickslot_ideal_ratio_1920x1200.png");
    let mut frame = RgbImage::filled(1920, 1200, [40, 60, 90]);
    let top = 1200 - capture.height;
    for y in 0..capture.height {
        for x in 0..capture.width {
            frame.set_pixel(x, top + y, capture.pixel(x, y));
        }
    }
    let bar = find_bar(&frame.rows_from(top)).unwrap();
    let slot = quickslot::slot_rect(bar, (0, top as i32), 5);
    // The slot's own count is wiped by its icon's colour first, so only the drawn one is read.
    for y in slot.y + 18..slot.bottom() {
        for x in slot.x..slot.x + 24 {
            frame.set_pixel(x as usize, y as usize, [140, 30, 30]);
        }
    }
    quickslot::draw_count(&mut frame, slot, red);
    frame
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_watcher_counts_the_potions_used_from_the_quickslots() {
    let dir = common::repo().join("tests/fixtures/icons");
    let mut watcher = TrainingWatcher::new(Box::new(Lines(vec!["Hidden Street", "Ant Tunnel I"], 0)), Tracker::new());
    for (id, name) in potions::candidates(&std::fs::read_to_string(dir.join("consumables.json")).unwrap()) {
        if let Ok(png) = std::fs::read(dir.join(format!("{id}.png"))) {
            watcher.add_potion_icon(id, &name, &png);
        }
    }
    let bounds = Rect::new(0, 0, 1920, 1200);
    let mut t = 100.0;
    for (red, looks) in [(30, 3), (28, 3)] {
        let frame = quickslot_frame(red);
        let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor: None };
        for _ in 0..looks {
            watcher.step(&mut screen, bounds, t);
            t += 1.0;
        }
    }
    let potions = watcher.training(t).potions;
    let lines: Vec<(&str, u32, u32)> = potions.lines.iter().map(|l| (l.name.as_str(), l.used, l.picked)).collect();
    assert_eq!(lines, [("Red Potion", 2, 0)]);
    assert!(potions.lines[0].icon.is_some());
}

/// OCR answering the minimap's street, then the map named in `map` now (the test moves the player).
struct Moving(std::sync::Arc<std::sync::Mutex<&'static str>>, usize);

impl ImageOcr for Moving {
    fn recognize_image(&mut self, _image: &GrayImage) -> OcrAnswer {
        self.1 += 1;
        let text = if self.1 % 2 == 1 { "Hidden Street" } else { *self.0.lock().unwrap() };
        OcrAnswer::Text(text.to_string())
    }
}

/// `seconds` of the game with `bar` at the bottom and the pointer at `cursor`.
fn play(watcher: &mut TrainingWatcher, bar: &str, cursor: Option<Point>, seconds: u32, t: &mut f64) {
    let frame = game_frame("minimap_free_market_1920x1200.png", bar);
    let mut screen = FrameScreen { frame: &frame, origin: Point::default(), cursor };
    for _ in 0..seconds {
        watcher.step(&mut screen, Rect::new(0, 0, 1920, 1200), *t);
        *t += 1.0;
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn moving_to_the_next_map_ends_the_session_and_hides_the_card() {
    let map = std::sync::Arc::new(std::sync::Mutex::new("East Rocky Mountain V"));
    let mut watcher = TrainingWatcher::new(Box::new(Moving(map.clone(), 0)), Tracker::new());
    let show = Show { exp: true, quest: true, strip: None };
    let mut t = 0.0;
    play(&mut watcher, "status_bar_normal_1920x1200.png", None, 6, &mut t);
    play(&mut watcher, "status_bar_exp_98_percent_1920x1200.png", None, 14, &mut t);
    let before = watcher.training(t);
    assert_eq!(before.exp.map.as_deref(), Some("East Rocky Mountain V"));
    assert!(before.exp.session.gained > 0);
    assert!(has_content(&before, show));
    // Through the portal to VI, a numeral away: within a few seconds the session is over and the card gone.
    *map.lock().unwrap() = "East Rocky Mountain VI";
    play(&mut watcher, "status_bar_exp_98_percent_1920x1200.png", None, 5, &mut t);
    let after = watcher.training(t);
    assert_eq!(after.exp.map.as_deref(), Some("East Rocky Mountain VI"));
    assert_eq!(after.exp.session.gained, 0);
    assert!(!has_content(&after, show));
    assert_eq!(watcher.tracker().history().len(), 1);
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_pointer_resting_on_the_minimap_title_is_not_a_new_map() {
    let map = std::sync::Arc::new(std::sync::Mutex::new("East Rocky Mountain V"));
    let mut watcher = TrainingWatcher::new(Box::new(Moving(map.clone(), 0)), Tracker::new());
    let mut t = 0.0;
    play(&mut watcher, "status_bar_normal_1920x1200.png", None, 6, &mut t);
    play(&mut watcher, "status_bar_exp_98_percent_1920x1200.png", None, 6, &mut t);
    // What OCR makes of the title with the pointer on it.
    *map.lock().unwrap() = "Rocky";
    play(&mut watcher, "status_bar_exp_98_percent_1920x1200.png", Some(Point { x: 60, y: 20 }), 10, &mut t);
    *map.lock().unwrap() = "East Rocky Mountain V";
    play(&mut watcher, "status_bar_exp_98_percent_1920x1200.png", None, 4, &mut t);
    let training = watcher.training(t);
    assert_eq!(training.exp.map.as_deref(), Some("East Rocky Mountain V"));
    assert!(training.exp.session.gained > 0);
    assert!(watcher.tracker().history().is_empty());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn misreads_of_the_map_name_never_end_the_session() {
    let map = std::sync::Arc::new(std::sync::Mutex::new("East Rocky Mountain III"));
    let mut watcher = TrainingWatcher::new(Box::new(Moving(map.clone(), 0)), Tracker::new());
    let mut t = 0.0;
    play(&mut watcher, "status_bar_normal_1920x1200.png", None, 6, &mut t);
    play(&mut watcher, "status_bar_exp_98_percent_1920x1200.png", None, 6, &mut t);
    // What Tesseract makes of a video frame of the same title, each held long enough to be read twice in a row.
    for misread in
        ["East Rocky Mountain Ill", "East Rocky Mountaln lll", "East Rocky Mountain", "East Rocky Mountain III"]
    {
        *map.lock().unwrap() = misread;
        play(&mut watcher, "status_bar_exp_98_percent_1920x1200.png", None, 6, &mut t);
    }
    let training = watcher.training(t);
    assert_eq!(training.exp.map.as_deref(), Some("East Rocky Mountain III"));
    assert!(training.exp.session.gained > 0);
    assert!(watcher.tracker().history().is_empty());
}
