//! The Kerning City Party Quest: its stages by map, what each asks of the party, and Cloto's questions with the
//! number of coupons that answers each (pure).

use crate::market::catalog::ratio;

/// Map names OCR'd off the minimap are close to these; "1st Accompaniment <2nd Stage>" and "<3rd Stage>" differ
/// in two letters, so the best match must also clear the runner-up.
const MIN_SCORE: f64 = 85.0;
const MIN_MARGIN: f64 = 3.0;
/// The quest's own time limit.
pub const TIME_LIMIT_S: f64 = 1_800.0;

/// One stage: the map it is played on and what the party does there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    pub map: &'static str,
    pub name: &'static str,
    pub goal: &'static str,
    /// Stages solved by standing on `pick` of `of` ropes, platforms or barrels: every way to try.
    pub combination: Option<(u8, u8)>,
}

/// One of Cloto's stage-one questions, said briefly, and its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Question {
    pub asks: &'static str,
    pub coupons: u32,
}

pub const NAME: &str = "Kerning City PQ";

pub const STAGES: [Stage; 6] = [
    Stage {
        map: "1st Accompaniment <1st Stage>",
        name: "Coupons",
        goal: "Ask Cloto your question, bring exactly that many coupons from the Ligators, and give the pass to your leader.",
        combination: None,
    },
    Stage {
        map: "1st Accompaniment <2nd Stage>",
        name: "Ropes",
        goal: "Three of you hang on three of the four ropes, then the leader asks Cloto.",
        combination: Some((3, 4)),
    },
    Stage {
        map: "1st Accompaniment <3rd Stage>",
        name: "Platforms",
        goal: "Three of you stand on three of the five platforms, then the leader asks Cloto.",
        combination: Some((3, 5)),
    },
    Stage {
        map: "1st Accompaniment <4th Stage>",
        name: "Barrels",
        goal: "Three of you stand on three of the six barrels, then the leader asks Cloto.",
        combination: Some((3, 6)),
    },
    Stage {
        map: "1st Accompaniment <Last Stage>",
        name: "King Slime",
        goal: "Defeat King Slime and the monsters with it; the leader hands Cloto the passes they drop.",
        combination: None,
    },
    Stage {
        map: "1st Accompaniment <Bonus>",
        name: "Bonus",
        goal: "Defeat what is there and pick up its drops before time runs out.",
        combination: None,
    },
];

pub const EXIT: Stage = Stage {
    map: "1st Accompaniment <Exit>",
    name: "Exit",
    goal: "Talk to Nella to go back to Kerning City.",
    combination: None,
};

/// Smallest answer first, the way the coupons are counted out.
pub const QUESTIONS: [Question; 9] = [
    Question { asks: "Level for the 1st job advancement", coupons: 10 },
    Question { asks: "EXP from level 1 to level 2", coupons: 15 },
    Question { asks: "INT a Magician needs", coupons: 20 },
    Question { asks: "Lowest level for this party quest", coupons: 21 },
    Question { asks: "STR+DEX+INT+LUK at level 1", coupons: 25 },
    Question { asks: "DEX a Bowman or Thief needs", coupons: 25 },
    Question { asks: "AP from level 1 to level 6", coupons: 25 },
    Question { asks: "Level for the 2nd job advancement", coupons: 30 },
    Question { asks: "STR a Warrior needs", coupons: 35 },
];

/// Letters and digits only, lower case: OCR turns '<' into '(' or drops it.
fn normal(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// Where in the quest the map called `map` is: the stage's index (`STAGES.len()` for the exit).
pub fn stage_of(map: &str) -> Option<usize> {
    let read = normal(map);
    let mut scores: Vec<(f64, usize)> =
        STAGES.iter().chain([&EXIT]).enumerate().map(|(i, s)| (ratio(&read, &normal(s.map)), i)).collect();
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (best, index) = scores[0];
    (best >= MIN_SCORE && best - scores[1].0 >= MIN_MARGIN).then_some(index)
}

pub fn stage(index: usize) -> &'static Stage {
    STAGES.get(index).unwrap_or(&EXIT)
}

/// Every way to put `pick` members on `of` spots, numbered from 1 left to right, in order.
pub fn combinations(pick: u8, of: u8) -> Vec<Vec<u8>> {
    fn extend(from: u8, of: u8, left: u8, current: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if left == 0 {
            out.push(current.clone());
            return;
        }
        for spot in from..=of {
            current.push(spot);
            extend(spot + 1, of, left - 1, current, out);
            current.pop();
        }
    }
    let mut out = Vec::new();
    extend(1, of, pick, &mut Vec::new(), &mut out);
    out
}

/// A run through the quest: when its first stage was seen, and where the party is now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Run {
    pub started: f64,
    pub stage: usize,
    /// Last seen on a quest map.
    pub seen: f64,
}

/// Follows the quest from the map names: a run starts on the first stage and ends on leaving the quest's maps.
#[derive(Clone, Debug, Default)]
pub struct QuestTracker {
    run: Option<Run>,
}

/// Off the quest's maps for this long (a map change, a minimap read that failed) and the run is over.
const LEFT_AFTER_S: f64 = 60.0;

impl QuestTracker {
    /// The map the player is on at `now` (None: unknown this time).
    pub fn update(&mut self, map: Option<&str>, now: f64) -> Option<Run> {
        match map.and_then(stage_of) {
            Some(stage) => {
                let run = self.run.get_or_insert(Run { started: now, stage, seen: now });
                // Coming back to the first stage later is another run.
                if stage == 0 && run.stage > 0 {
                    *run = Run { started: now, stage, seen: now };
                }
                run.stage = stage;
                run.seen = now;
            }
            None => {
                if self.run.is_some_and(|r| now - r.seen > LEFT_AFTER_S) {
                    self.run = None;
                }
            }
        }
        self.run
    }

    /// The run while the player is on a quest map (not in the minute after leaving it).
    pub fn current(&self, now: f64) -> Option<Run> {
        self.run.filter(|r| now - r.seen <= LEFT_AFTER_S)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_are_known_through_ocr_noise() {
        assert_eq!(stage_of("1st Accompaniment <1st Stage>"), Some(0));
        assert_eq!(stage_of("1st Accompaniment (2nd Stage>"), Some(1));
        assert_eq!(stage_of("lst Accompaniment <3rd Stage"), Some(2));
        assert_eq!(stage_of("1st Accompaniment <4th stage>"), Some(3));
        assert_eq!(stage_of("1st Accompaniment <Last Stage>"), Some(4));
        assert_eq!(stage_of("1st Accompaniment <Bonus>"), Some(5));
        assert_eq!(stage_of("1st Accompaniment <Exit>"), Some(6));
        assert_eq!(stage_of("Free Market <3>"), None);
        assert_eq!(stage_of("Kerning City"), None);
    }

    #[test]
    fn every_way_to_stand_is_listed() {
        assert_eq!(combinations(3, 4), vec![vec![1, 2, 3], vec![1, 2, 4], vec![1, 3, 4], vec![2, 3, 4]]);
        assert_eq!(combinations(3, 5).len(), 10);
        assert_eq!(combinations(3, 6).len(), 20);
    }

    #[test]
    fn a_run_goes_from_the_first_stage_until_the_party_leaves() {
        let mut quest = QuestTracker::default();
        assert_eq!(quest.update(Some("Kerning City"), 0.0), None);
        let run = quest.update(Some("1st Accompaniment <1st Stage>"), 10.0).unwrap();
        assert_eq!((run.started, run.stage), (10.0, 0));
        assert_eq!(quest.update(None, 20.0).map(|r| r.stage), Some(0)); // a failed read changes nothing
        let run = quest.update(Some("1st Accompaniment <2nd Stage>"), 300.0).unwrap();
        assert_eq!((run.started, run.stage), (10.0, 1));
        assert!(quest.current(330.0).is_some());
        assert_eq!(quest.update(Some("Kerning City"), 400.0), None);
        assert_eq!(quest.current(400.0), None);
    }
}
