//! Settle a UI scale measured on many frames (pure: time is passed in).
//!
//! One frame can measure the scale wrong (a menu over the status bar's outline, the pointer over the shop's
//! buttons, a compressed frame), and acting on it flips everything read at that scale. So the readings of the last
//! `config::SCALE_VOTE_WINDOW_S` are kept, and the scale is the median of the readings most of them agree with.

use std::collections::VecDeque;

use crate::config;

/// Readings of a scale over a sliding window of time, and the value they settled on.
#[derive(Clone, Debug, Default)]
pub struct ScaleVote {
    readings: VecDeque<(f64, f32)>,
    settled: Option<f32>,
}

impl ScaleVote {
    /// Add a reading taken at `now` (seconds on any steady clock) and return the settled scale. It changes only when
    /// a cluster of readings (each within `config::SCALE_VOTE_TOLERANCE` of its middle) is more than half the window
    /// and at least `config::SCALE_VOTE_MIN_READINGS`; otherwise the last settled value stays.
    pub fn push(&mut self, value: f32, now: f64) -> Option<f32> {
        // A clock that went back (another thread's, a restarted page) leaves nothing to compare with.
        if self.readings.back().is_some_and(|&(t, _)| t > now) {
            self.readings.clear();
        }
        self.readings.push_back((now, value));
        self.expire(now);
        if let Some(value) = self.majority() {
            self.settled = Some(value);
        }
        self.settled
    }

    pub fn settled(&self) -> Option<f32> {
        self.settled
    }

    /// Readings still inside the window at `now`.
    pub fn recent(&self, now: f64) -> usize {
        self.readings.iter().filter(|&&(t, _)| now - t <= config::SCALE_VOTE_WINDOW_S).count()
    }

    /// Forget the readings and the settled value (another window: what was measured on the old one says nothing).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn expire(&mut self, now: f64) {
        while self.readings.front().is_some_and(|&(t, _)| now - t > config::SCALE_VOTE_WINDOW_S) {
            self.readings.pop_front();
        }
    }

    /// The median of the largest cluster, when it is a clear majority of the window.
    fn majority(&self) -> Option<f32> {
        let mut values: Vec<f32> = self.readings.iter().map(|&(_, v)| v).collect();
        values.sort_by(f32::total_cmp);
        let cluster = |centre: f32| {
            values.iter().copied().filter(move |v| (v / centre - 1.0).abs() <= config::SCALE_VOTE_TOLERANCE)
        };
        let best = values.iter().copied().max_by_key(|&centre| cluster(centre).count())?;
        let members: Vec<f32> = cluster(best).collect();
        let n = members.len();
        if n < config::SCALE_VOTE_MIN_READINGS || 2 * n <= values.len() {
            return None;
        }
        Some(if n % 2 == 1 { members[n / 2] } else { (members[n / 2 - 1] + members[n / 2]) / 2.0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(vote: &mut ScaleVote, readings: &[(f64, f32)]) -> Option<f32> {
        readings.iter().fold(None, |_, &(t, v)| vote.push(v, t))
    }

    #[test]
    fn nothing_is_decided_before_enough_readings() {
        let mut vote = ScaleVote::default();
        assert_eq!(vote.push(0.9, 0.0), None);
        assert_eq!(vote.push(0.9, 1.0), None);
        assert_eq!(vote.push(0.9, 2.0), Some(0.9));
    }

    #[test]
    fn noise_settles_on_the_median() {
        let mut vote = ScaleVote::default();
        let settled = feed(&mut vote, &[(0.0, 0.901), (0.5, 0.899), (1.0, 0.900), (1.5, 0.902), (2.0, 0.898)]);
        assert_eq!(settled, Some(0.900));
    }

    #[test]
    fn one_outlier_changes_nothing() {
        let mut vote = ScaleVote::default();
        feed(&mut vote, &[(0.0, 0.64), (1.0, 0.64), (2.0, 0.64)]);
        assert_eq!(vote.push(1.0, 3.0), Some(0.64)); // a menu over the bar's outline, read as another width
        assert_eq!(vote.push(0.64, 4.0), Some(0.64));
    }

    #[test]
    fn no_majority_keeps_the_settled_value() {
        let mut vote = ScaleVote::default();
        feed(&mut vote, &[(0.0, 1.0), (1.0, 1.0), (2.0, 1.0)]);
        // Two new readings against three old: no cluster is more than half.
        assert_eq!(feed(&mut vote, &[(2.5, 1.2), (3.0, 1.2), (3.2, 0.7)]), Some(1.0));
    }

    #[test]
    fn a_real_switch_takes_over_within_the_window() {
        let mut vote = ScaleVote::default();
        let mut now = 0.0;
        while now < 10.0 {
            vote.push(0.9, now);
            now += 1.0;
        }
        // The player switches to Ideal Ratio at t = 10, one reading a second.
        let mut switched_at = None;
        while now < 20.0 {
            if vote.push(0.577, now) == Some(0.577) && switched_at.is_none() {
                switched_at = Some(now);
            }
            now += 1.0;
        }
        let after = switched_at.expect("the new scale took over") - 10.0;
        assert!(after <= config::SCALE_VOTE_WINDOW_S, "took {after} s");
        assert!(after >= 2.0, "one or two readings must not decide ({after} s)");
    }

    #[test]
    fn a_time_gap_keeps_the_value_and_lets_new_readings_decide() {
        let mut vote = ScaleVote::default();
        feed(&mut vote, &[(0.0, 1.2), (1.0, 1.2), (2.0, 1.2)]);
        // A minute without the bar (the Cash Shop): the old readings have expired, the value stays.
        assert_eq!(vote.push(0.64, 60.0), Some(1.2));
        assert_eq!(vote.recent(60.0), 1);
        assert_eq!(feed(&mut vote, &[(61.0, 0.64), (62.0, 0.64)]), Some(0.64));
    }

    #[test]
    fn a_clock_going_back_starts_over_but_keeps_the_value() {
        let mut vote = ScaleVote::default();
        feed(&mut vote, &[(100.0, 1.0), (101.0, 1.0), (102.0, 1.0)]);
        assert_eq!(feed(&mut vote, &[(0.0, 1.28), (1.0, 1.28)]), Some(1.0));
        assert_eq!(vote.push(1.28, 2.0), Some(1.28));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut vote = ScaleVote::default();
        feed(&mut vote, &[(0.0, 1.0), (1.0, 1.0), (2.0, 1.0)]);
        vote.reset();
        assert_eq!(vote.settled(), None);
        assert_eq!(vote.recent(2.0), 0);
    }
}
