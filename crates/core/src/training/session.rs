//! EXP over time: the session being played, its rates, and the sessions before it (pure; time is passed in as
//! seconds since the epoch, so history has real dates).
//!
//! A reading counts once two reads in a row agree (the numbers change while the game redraws them). The EXP shown
//! is into the current level, so a big drop in the share is a level up (the rest of the old level plus the new
//! EXP is gained), and a small drop is EXP lost by dying. Time counts only while the bar is seen and EXP keeps
//! coming, and a minute without EXP (town, shopping, AFK) or a move to another map ends the session: the next EXP
//! starts a new one, so the rates are always those of the training going on now, here. EXP gained in a town (a
//! quest handed in, an event) is no training: it is followed but never counted, so no session starts there. A pause
//! holds the session as it is. The level's size (EXP
//! over share) tells characters apart: a change in it that is not a level up is another character, and counts
//! nothing.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use super::towns::is_town;
use crate::vision::gauge::ExpReading;

/// Reads in a row that must agree before a new value counts.
const CONFIRM_READS: u32 = 2;
/// The share falling by more than this (hundredths of a percent) is a level up, not a death.
const LEVEL_UP_DROP: u32 = 2_000;
/// Seen again within this, the time between counts and so does the EXP gained (the game was briefly covered).
const OBSERVED_GAP_S: f64 = 300.0;
/// Seen again after this, the old session is over and a new one starts.
const NEW_SESSION_GAP_S: f64 = 1_800.0;
/// No EXP for this long and the session is over: it goes into the history and the next EXP starts another.
const IDLE_RESET_S: f64 = 60.0;
/// The "now" rate and the time to level look at this much recent play.
const RECENT_S: f64 = 300.0;
/// Rates start after this much play (sooner, one kill swings them).
pub const MIN_RATE_S: f64 = 10.0;
const MINUTE_S: f64 = 60.0;
/// From this share up the level's size worked out from a read is good to 1%...
const PRECISE_SHARE: u32 = 100;
/// ...so sizes further apart than this are different levels.
const SAME_LEVEL: f64 = 0.02;
/// One level is at most this much bigger than the one before (level 20 to 21 measured 1.43).
const MAX_LEVEL_GROWTH: f64 = 2.0;
/// Sessions kept in the history.
const HISTORY: usize = 100;
/// The card's graph spans the last ten minutes, a point every 10 s...
pub const GRAPH_S: f64 = 600.0;
pub const GRAPH_STEP_S: f64 = 10.0;
/// ...each the rate over the minute before it: kills come in lumps, and one bucket's gains would be a comb.
const GRAPH_WINDOW_S: f64 = MINUTE_S;

/// What one map gave in a session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapStat {
    pub map: String,
    pub active_s: f64,
    pub gained: u64,
}

/// What one item in the quickslots did in a session: how many were used, how many picked up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PotionStat {
    pub id: u32,
    pub name: String,
    pub used: u32,
    pub picked: u32,
}

/// One stretch of play.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// Seconds since the epoch.
    pub started: f64,
    pub ended: f64,
    /// Time that counts: bar in view, not paused, EXP coming in.
    pub active_s: f64,
    pub gained: u64,
    /// Sum of each gain as a share of its level, in percent.
    pub percent: f64,
    pub levels: u32,
    pub lost: u64,
    /// The most EXP gained within one clock minute.
    pub best_minute: u64,
    pub maps: Vec<MapStat>,
    /// Sessions saved before potions were counted have none.
    #[serde(default)]
    pub potions: Vec<PotionStat>,
}

/// The tracker's numbers at one moment, for the cards and the page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrainingView {
    pub reading: Option<ExpReading>,
    /// The bar was read a moment ago.
    pub live: bool,
    pub paused: bool,
    pub session: Session,
    /// EXP a minute over the last minute, over the last five, and over the whole session.
    pub last_minute: Option<f64>,
    pub recent_per_min: Option<f64>,
    pub average_per_min: Option<f64>,
    pub percent_per_hour: Option<f64>,
    /// Seconds to the next level at the recent rate.
    pub to_level_s: Option<f64>,
    pub map: Option<String>,
    pub map_stat: Option<MapStat>,
    pub graph: ExpGraph,
}

/// EXP a minute over the session's last `GRAPH_S`, for the card's graph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpGraph {
    /// One point every `GRAPH_STEP_S`, oldest first, the last at `now`; none before the session's first EXP.
    pub rates: Vec<f64>,
    /// Level ups, as seconds before `now`.
    pub level_ups: Vec<f64>,
}

/// Two settled readings of the minimap name the same map: letters and digits alike, whatever OCR made of the rest.
fn same_map(a: &str, b: &str) -> bool {
    let key =
        |s: &str| s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect::<String>();
    key(a) == key(b)
}

/// The level's EXP worked out from `reading`, when its share is big enough for that to be precise.
fn precise_size(reading: ExpReading) -> Option<u64> {
    (reading.hundredths >= PRECISE_SHARE).then(|| reading.needed()).flatten()
}

#[derive(Clone, Copy, Debug)]
struct Seen {
    at: f64,
    reading: ExpReading,
}

/// Follows the EXP bar into sessions.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    session: Session,
    history: Vec<Session>,
    /// The last value that counted, and when it was last seen.
    last: Option<Seen>,
    candidate: Option<(ExpReading, u32)>,
    paused: bool,
    map: Option<String>,
    /// The map is a town: nothing gained there counts.
    town: bool,
    /// Gains of the last `RECENT_S`, and the time that counted in the same window.
    recent: VecDeque<(f64, u64)>,
    recent_time: VecDeque<(f64, f64)>,
    last_gain: Option<f64>,
    /// Resumed since the last look: the minute without EXP starts over.
    resumed: bool,
    /// Each item's last total in the quickslots, and when the quickslots were last read.
    potion_totals: HashMap<u32, u32>,
    potions_seen: Option<f64>,
    /// The current clock minute and what it has given so far.
    minute: (i64, u64),
    /// The level's EXP as worked out from the highest share seen in it (the most precise).
    needed: Option<(u64, u32)>,
    /// Gains and level ups of the graph's span (and the minute before it), for `ExpGraph`.
    timeline: VecDeque<(f64, u64)>,
    level_ups: VecDeque<f64>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    session: Session,
    history: Vec<Session>,
    paused: bool,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// The tracker as it was saved (`to_json`); a damaged file starts afresh.
    pub fn from_json(text: &str) -> Self {
        match serde_json::from_str::<Saved>(text) {
            Ok(saved) => {
                Self { session: saved.session, history: saved.history, paused: saved.paused, ..Self::default() }
            }
            Err(_) => Self::default(),
        }
    }

    pub fn to_json(&self) -> String {
        let saved = Saved { session: self.session.clone(), history: self.history.clone(), paused: self.paused };
        serde_json::to_string(&saved).unwrap_or_default()
    }

    pub fn history(&self) -> &[Session] {
        &self.history
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Pause or resume: while paused neither time nor EXP counts (the bar is still followed, so nothing gained
    /// meanwhile is counted on resume).
    pub fn set_paused(&mut self, paused: bool) {
        // Resuming gives the session a fresh minute: the time paused is not time without EXP.
        self.resumed |= self.paused && !paused;
        self.paused = paused;
    }

    /// A minute without EXP (not paused) ends the session.
    fn expire(&mut self, now: f64) {
        if self.resumed {
            self.resumed = false;
            self.last_gain = self.last_gain.map(|_| now);
        }
        if !self.paused && self.last_gain.is_some_and(|at| now - at > IDLE_RESET_S) {
            self.new_session(now);
        }
    }

    /// End this session (into the history, if anything happened in it) and start another.
    pub fn new_session(&mut self, now: f64) {
        if self.session.gained > 0 || self.session.active_s >= MINUTE_S {
            self.session.ended = self.session.ended.max(self.session.started);
            self.history.push(std::mem::take(&mut self.session));
            let over = self.history.len().saturating_sub(HISTORY);
            self.history.drain(..over);
        }
        self.session = Session { started: now, ended: now, ..Session::default() };
        self.recent.clear();
        self.recent_time.clear();
        self.timeline.clear();
        self.level_ups.clear();
        self.last_gain = None;
        self.minute = (0, 0);
    }

    /// The map the player is on, when known: gains and time go to it. Moving to another map, or into a town, ends the
    /// session (not while paused). No map (the minimap not read for a while, or not yet) keeps the last one for the
    /// move, but counts as a field: a player training with the minimap closed still has a session.
    pub fn set_map(&mut self, map: Option<String>, now: f64) {
        let Some(map) = map else {
            self.town = false;
            return;
        };
        let moved = self.map.as_deref().is_some_and(|was| !same_map(was, &map));
        let town = is_town(&map);
        // A town settled before any map was (a quest handed in while the minimap was still being read) ends what
        // its EXP had started.
        if (moved || (town && !self.town)) && !self.paused {
            self.new_session(now);
        }
        self.town = town;
        self.map = Some(map);
    }

    /// The quickslots' totals per item at `now` (only the items read this time; `name` names an id). A total that
    /// fell is that many used, one that rose that many picked up; nothing counts while paused or after a while
    /// unseen (the totals moved meanwhile).
    pub fn observe_potions(&mut self, now: f64, totals: &HashMap<u32, u32>, name: impl Fn(u32) -> String) {
        self.expire(now);
        let counted = !self.paused && !self.town && self.potions_seen.is_some_and(|at| now - at <= OBSERVED_GAP_S);
        for (&id, &total) in totals {
            let was = self.potion_totals.insert(id, total);
            let Some(was) = was.filter(|&was| counted && was != total) else { continue };
            let stats = &mut self.session.potions;
            let index = match stats.iter().position(|p| p.id == id) {
                Some(index) => index,
                None => {
                    stats.push(PotionStat { id, name: name(id), ..PotionStat::default() });
                    stats.len() - 1
                }
            };
            if total < was {
                stats[index].used += was - total;
            } else {
                stats[index].picked += total - was;
            }
        }
        self.potions_seen = Some(now);
    }

    /// A read of the bar at `now` (None: not readable this time).
    pub fn observe(&mut self, now: f64, reading: Option<ExpReading>) {
        self.expire(now);
        let Some(reading) = reading else { return };
        let votes = match self.candidate {
            Some((candidate, n)) if candidate == reading => n + 1,
            _ => 1,
        };
        self.candidate = Some((reading, votes));
        if votes < CONFIRM_READS {
            return;
        }
        let Some(last) = self.last else {
            self.begin(now, reading);
            return;
        };
        let gap = now - last.at;
        if gap > NEW_SESSION_GAP_S {
            self.new_session(now);
            self.begin(now, reading);
            return;
        }
        if self.town {
            // Nothing counts, but the bar is followed so what was gained here is not counted after leaving; the
            // level's size is learnt afresh, as the gain may have been a level up.
            if reading != last.reading {
                self.rebase(reading);
            }
            self.last = Some(Seen { at: now, reading });
            return;
        }
        let counted = gap <= OBSERVED_GAP_S && !self.paused;
        let levels = self.session.levels;
        let gain = if reading == last.reading { 0 } else { self.change(last.reading, reading) };
        if counted && self.session.levels > levels {
            self.level_ups.push_back(now);
        }
        // The clock runs while EXP keeps coming; the stretch before the first gain after a pause in play counts
        // as play too (the monster took that long).
        if counted && (gain > 0 || self.last_gain.is_some()) {
            self.count_time(now, gap);
        }
        if counted && gain > 0 {
            self.gain(now, gain, reading);
        }
        self.last = Some(Seen { at: now, reading });
        self.session.ended = now;
    }

    fn begin(&mut self, now: f64, reading: ExpReading) {
        if self.session.started == 0.0 {
            self.session.started = now;
        }
        self.session.ended = now;
        self.last = Some(Seen { at: now, reading });
        self.note_needed(reading);
    }

    /// EXP gained going from `old` to `new` (0 for a loss, which is counted as lost, or another character).
    fn change(&mut self, old: ExpReading, new: ExpReading) -> u64 {
        let old_size = self.needed.map(|(n, _)| n).or_else(|| precise_size(old));
        let new_size = precise_size(new);
        let other_level =
            matches!((old_size, new_size), (Some(a), Some(b)) if (a as f64 - b as f64).abs() > a as f64 * SAME_LEVEL);
        if new.hundredths + LEVEL_UP_DROP < old.hundredths && new.exp < old.exp {
            // The next level is bigger, but not many times bigger: anything else is another character.
            if let (Some(old_size), Some(new_size)) = (old_size, new_size)
                && (new_size <= old_size || new_size as f64 > old_size as f64 * MAX_LEVEL_GROWTH)
            {
                return self.rebase(new);
            }
            let needed = old_size.or_else(|| old.needed()).unwrap_or(old.exp);
            self.session.levels += 1;
            self.needed = None;
            self.note_needed(new);
            return needed.saturating_sub(old.exp) + new.exp;
        }
        if other_level {
            return self.rebase(new);
        }
        self.note_needed(new);
        if new.exp < old.exp {
            self.session.lost += old.exp - new.exp;
            return 0;
        }
        new.exp - old.exp
    }

    /// Another character: follow its bar from here, counting nothing for the switch.
    fn rebase(&mut self, reading: ExpReading) -> u64 {
        self.needed = None;
        self.note_needed(reading);
        0
    }

    fn note_needed(&mut self, reading: ExpReading) {
        if let Some(needed) = reading.needed()
            && self.needed.is_none_or(|(_, share)| reading.hundredths > share)
        {
            self.needed = Some((needed, reading.hundredths));
        }
    }

    fn count_time(&mut self, now: f64, gap: f64) {
        self.session.active_s += gap;
        self.recent_time.push_back((now, gap));
        if let Some(stat) = self.map_stat_mut() {
            stat.active_s += gap;
        }
    }

    fn gain(&mut self, now: f64, gain: u64, reading: ExpReading) {
        if self.session.gained == 0 {
            self.session.started = now; // a session starts with its first EXP, not when the last one ended
        }
        self.session.gained += gain;
        if let Some((needed, _)) = self.needed.or(reading.needed().map(|n| (n, 0)))
            && needed > 0
        {
            self.session.percent += gain as f64 * 100.0 / needed as f64;
        }
        self.last_gain = Some(now);
        self.recent.push_back((now, gain));
        self.timeline.push_back((now, gain));
        let minute = (now / MINUTE_S).floor() as i64;
        self.minute = if self.minute.0 == minute { (minute, self.minute.1 + gain) } else { (minute, gain) };
        self.session.best_minute = self.session.best_minute.max(self.minute.1);
        if let Some(stat) = self.map_stat_mut() {
            stat.gained += gain;
        }
    }

    fn map_stat_mut(&mut self) -> Option<&mut MapStat> {
        let map = self.map.clone()?;
        let maps = &mut self.session.maps;
        let index = match maps.iter().position(|m| m.map == map) {
            Some(index) => index,
            None => {
                maps.push(MapStat { map, ..MapStat::default() });
                maps.len() - 1
            }
        };
        maps.get_mut(index)
    }

    fn trim(&mut self, now: f64) {
        while self.recent.front().is_some_and(|(at, _)| now - at > RECENT_S) {
            self.recent.pop_front();
        }
        while self.recent_time.front().is_some_and(|(at, _)| now - at > RECENT_S) {
            self.recent_time.pop_front();
        }
        while self.timeline.front().is_some_and(|(at, _)| now - at > GRAPH_S + GRAPH_WINDOW_S) {
            self.timeline.pop_front();
        }
        while self.level_ups.front().is_some_and(|at| now - at > GRAPH_S) {
            self.level_ups.pop_front();
        }
    }

    /// The graph at `now`: each point the EXP a minute over the minute before it (over the time since the session's
    /// first EXP while that is shorter, so the line does not climb from nothing); none until `MIN_RATE_S` of it.
    fn graph(&self, now: f64) -> ExpGraph {
        if self.session.gained == 0 {
            return ExpGraph::default();
        }
        let start = self.session.started;
        let points = (GRAPH_S / GRAPH_STEP_S) as usize;
        let rates = (0..=points)
            .rev()
            .map(|k| now - k as f64 * GRAPH_STEP_S)
            .filter_map(|at| {
                let window = GRAPH_WINDOW_S.min(at - start);
                (window >= MIN_RATE_S).then(|| {
                    let gained: u64 =
                        self.timeline.iter().filter(|(t, _)| *t >= at - window && *t <= at).map(|(_, g)| g).sum();
                    gained as f64 * MINUTE_S / window
                })
            })
            .collect();
        ExpGraph { rates, level_ups: self.level_ups.iter().map(|at| now - at).collect() }
    }

    /// The numbers at `now`; `live_s` is how recent a read must be for the bar to count as in view.
    pub fn view(&mut self, now: f64, live_s: f64) -> TrainingView {
        self.expire(now);
        self.trim(now);
        let session = &self.session;
        let per_min = |gained: f64, seconds: f64| (seconds >= MIN_RATE_S).then(|| gained * MINUTE_S / seconds);
        let recent_s: f64 = self.recent_time.iter().map(|(_, s)| s).sum();
        let recent_gain: u64 = self.recent.iter().map(|(_, g)| g).sum();
        let last_minute_gain: u64 = self.recent.iter().filter(|(at, _)| now - at <= MINUTE_S).map(|(_, g)| g).sum();
        let average_per_min = per_min(session.gained as f64, session.active_s);
        let recent_per_min = per_min(recent_gain as f64, recent_s).or(average_per_min);
        let stalled = self.last_gain.is_none_or(|at| now - at > IDLE_RESET_S);
        let last_minute = (session.active_s >= MINUTE_S && !stalled).then_some(last_minute_gain as f64);
        let reading = self.last.map(|s| s.reading);
        let to_level_s = match (reading, self.needed, recent_per_min) {
            (Some(r), Some((needed, _)), Some(rate)) if rate > 0.0 && !stalled => {
                Some(needed.saturating_sub(r.exp) as f64 / rate * MINUTE_S)
            }
            _ => None,
        };
        let map_stat = self.map.as_ref().and_then(|m| session.maps.iter().find(|s| &s.map == m)).cloned();
        TrainingView {
            reading,
            live: self.last.is_some_and(|s| now - s.at <= live_s),
            paused: self.paused,
            session: session.clone(),
            last_minute,
            recent_per_min,
            average_per_min,
            percent_per_hour: (session.active_s >= MIN_RATE_S).then(|| session.percent * 3_600.0 / session.active_s),
            to_level_s,
            map: self.map.clone(),
            map_stat,
            graph: self.graph(now),
        }
    }
}
