//! The loop guard. Putting a name back after the app changed its title is
//! normally one step now and then, but an app that answers every new title
//! with its own would start a tug of war that never ends. The guard allows a
//! few re-applies per second for one window and then pauses re-applying
//! for a while; the app's title shows during the pause.

use std::collections::VecDeque;

/// Re-applies allowed within `PER_MS`.
pub const LIMIT: usize = 5;
pub const PER_MS: u64 = 1000;
/// How long a window is left alone after it went over the limit.
pub const PAUSE_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Put the name back.
    Apply,
    /// Over the limit: a pause starts now.
    Pause,
    /// In a pause: leave the title as the app set it.
    Wait,
}

#[derive(Clone, Debug, Default)]
pub struct Guard {
    recent: VecDeque<u64>,
    paused_until: Option<u64>,
    pauses: u32,
}

impl Guard {
    /// Asked each time the name should go back on; `now` in milliseconds.
    pub fn check(&mut self, now: u64) -> Verdict {
        if let Some(until) = self.paused_until {
            if now < until {
                return Verdict::Wait;
            }
            self.paused_until = None;
            self.recent.clear();
        }
        while self
            .recent
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= PER_MS)
        {
            self.recent.pop_front();
        }
        if self.recent.len() >= LIMIT {
            self.paused_until = Some(now + PAUSE_MS);
            self.pauses += 1;
            return Verdict::Pause;
        }
        self.recent.push_back(now);
        Verdict::Apply
    }

    /// How many pauses this window has had; the first is logged as a
    /// warning, later ones only in the debug log.
    pub fn pauses(&self) -> u32 {
        self.pauses
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_few_changes_a_second_are_always_answered() {
        let mut guard = Guard::default();
        for step in 0..20 {
            assert_eq!(guard.check(step * 300), Verdict::Apply, "step {step}");
        }
        assert_eq!(guard.pauses(), 0);
    }

    #[test]
    fn a_tug_of_war_pauses_after_the_limit_and_resumes_later() {
        let mut guard = Guard::default();
        for step in 0..LIMIT as u64 {
            assert_eq!(guard.check(1000 + step * 10), Verdict::Apply);
        }
        assert_eq!(guard.check(1060), Verdict::Pause);
        assert_eq!(guard.pauses(), 1);
        assert_eq!(guard.check(1070), Verdict::Wait);
        assert_eq!(guard.check(1060 + PAUSE_MS - 1), Verdict::Wait);
        assert_eq!(guard.check(1060 + PAUSE_MS), Verdict::Apply);
    }

    #[test]
    fn old_re_applies_fall_out_of_the_window() {
        let mut guard = Guard::default();
        for step in 0..LIMIT as u64 {
            assert_eq!(guard.check(step * 100), Verdict::Apply);
        }
        // The first one was PER_MS ago, so there is room for one more.
        assert_eq!(guard.check(PER_MS), Verdict::Apply);
        assert_eq!(guard.check(PER_MS + 1), Verdict::Pause);
    }

    #[test]
    fn a_second_pause_is_counted() {
        let mut guard = Guard::default();
        let mut now = 0;
        for _ in 0..2 {
            while guard.check(now) != Verdict::Pause {
                now += 1;
            }
            now += PAUSE_MS;
        }
        assert_eq!(guard.pauses(), 2);
    }
}
