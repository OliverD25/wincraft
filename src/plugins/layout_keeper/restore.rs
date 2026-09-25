//! When to restore each taskbar group. Runs on the plugin's one-second tick;
//! the restore itself touches the system and lives in the plugin.
//!
//! Apps open their windows at their own pace after sign-in: Chrome at once,
//! Telegram half a minute later. So each group with something saved is
//! restored on its own, once its window count has held still for
//! `settle_seconds`, if that happens within `give_up_seconds` of the start.
//! An app opened after that is left where the user put it.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Wait,
    /// These groups have settled; restore them now.
    Apply(Vec<String>),
    /// The time is up. These groups never settled and are left alone.
    TimedOut(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Watch {
    count: usize,
    stable_since: u64,
}

#[derive(Clone, Debug)]
pub struct Restore {
    started: Option<u64>,
    /// The groups still waiting, each with its last count.
    waiting: BTreeMap<String, Watch>,
    restored: usize,
    pub settle_seconds: u64,
    pub give_up_seconds: u64,
}

impl Restore {
    pub fn new(settle_seconds: u64, give_up_seconds: u64) -> Self {
        Self {
            started: None,
            waiting: BTreeMap::new(),
            restored: 0,
            settle_seconds,
            give_up_seconds,
        }
    }

    /// Starts waiting for `groups`, the ones with saved windows.
    pub fn start(&mut self, tick: u64, groups: Vec<String>) {
        self.started = Some(tick);
        self.restored = 0;
        self.waiting = groups
            .into_iter()
            .map(|group| {
                (
                    group,
                    Watch {
                        count: 0,
                        stable_since: tick,
                    },
                )
            })
            .collect();
    }

    pub fn is_running(&self) -> bool {
        self.started.is_some()
    }

    /// The groups not restored yet, whose saved windows must not be
    /// overwritten by the scrambled ones on screen.
    pub fn pending(&self) -> Vec<String> {
        self.waiting.keys().cloned().collect()
    }

    /// How many groups this run has restored so far.
    pub fn restored(&self) -> usize {
        self.restored
    }

    /// Feeds each group's window count at `tick`. Zero windows never count
    /// as settled: right after sign-in WinCraft often runs before the app
    /// has opened anything.
    pub fn step(&mut self, tick: u64, counts: &BTreeMap<String, usize>) -> Step {
        let Some(started) = self.started else {
            return Step::Nothing;
        };
        if tick.saturating_sub(started) >= self.give_up_seconds {
            self.started = None;
            let left = self.pending();
            self.waiting.clear();
            return Step::TimedOut(left);
        }
        let mut due = Vec::new();
        for (group, watch) in self.waiting.iter_mut() {
            let count = counts.get(group).copied().unwrap_or(0);
            if count != watch.count {
                watch.count = count;
                watch.stable_since = tick;
            }
            if count > 0 && tick.saturating_sub(watch.stable_since) >= self.settle_seconds {
                due.push(group.clone());
            }
        }
        for group in &due {
            self.waiting.remove(group);
        }
        self.restored += due.len();
        if self.waiting.is_empty() {
            self.started = None;
        }
        if due.is_empty() {
            Step::Wait
        } else {
            Step::Apply(due)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
        pairs
            .iter()
            .map(|(group, count)| (group.to_string(), *count))
            .collect()
    }

    #[test]
    fn each_group_is_restored_once_its_own_count_holds_still() {
        let mut restore = Restore::new(3, 180);
        restore.start(0, vec!["Chrome".into(), "Telegram".into()]);
        assert_eq!(restore.step(1, &counts(&[("Chrome", 4)])), Step::Wait);
        assert_eq!(restore.step(2, &counts(&[("Chrome", 9)])), Step::Wait);
        assert_eq!(restore.step(4, &counts(&[("Chrome", 9)])), Step::Wait);
        assert_eq!(
            restore.step(5, &counts(&[("Chrome", 9)])),
            Step::Apply(vec!["Chrome".into()])
        );
        assert!(restore.is_running());
        assert_eq!(restore.pending(), ["Telegram"]);

        // Telegram starts half a minute later.
        assert_eq!(
            restore.step(40, &counts(&[("Chrome", 9), ("Telegram", 1)])),
            Step::Wait
        );
        assert_eq!(
            restore.step(43, &counts(&[("Chrome", 9), ("Telegram", 1)])),
            Step::Apply(vec!["Telegram".into()])
        );
        assert!(!restore.is_running());
        assert_eq!(restore.restored(), 2);
        assert_eq!(restore.step(44, &counts(&[("Telegram", 1)])), Step::Nothing);
    }

    #[test]
    fn groups_that_settle_together_are_restored_together() {
        let mut restore = Restore::new(2, 180);
        restore.start(0, vec!["A".into(), "B".into()]);
        restore.step(1, &counts(&[("A", 1), ("B", 2)]));
        assert_eq!(
            restore.step(3, &counts(&[("A", 1), ("B", 2)])),
            Step::Apply(vec!["A".into(), "B".into()])
        );
        assert!(!restore.is_running());
    }

    #[test]
    fn an_app_opened_after_the_restore_window_is_left_alone() {
        let mut restore = Restore::new(3, 10);
        restore.start(100, vec!["Chrome".into(), "Telegram".into()]);
        restore.step(101, &counts(&[("Chrome", 2)]));
        assert_eq!(
            restore.step(104, &counts(&[("Chrome", 2)])),
            Step::Apply(vec!["Chrome".into()])
        );
        for tick in 105..110 {
            assert_eq!(restore.step(tick, &counts(&[("Chrome", 2)])), Step::Wait);
        }
        assert_eq!(
            restore.step(110, &counts(&[("Telegram", 1)])),
            Step::TimedOut(vec!["Telegram".into()])
        );
        assert!(!restore.is_running());
        assert!(restore.pending().is_empty());
        assert_eq!(
            restore.step(200, &counts(&[("Telegram", 1)])),
            Step::Nothing
        );
    }

    #[test]
    fn a_group_that_keeps_changing_waits() {
        let mut restore = Restore::new(3, 180);
        restore.start(0, vec!["Chrome".into()]);
        for (tick, count) in [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)] {
            assert_eq!(
                restore.step(tick, &counts(&[("Chrome", count)])),
                Step::Wait
            );
        }
        assert_eq!(
            restore.step(8, &counts(&[("Chrome", 5)])),
            Step::Apply(vec!["Chrome".into()])
        );
    }

    #[test]
    fn nothing_happens_until_it_is_started() {
        let mut restore = Restore::new(3, 10);
        assert_eq!(restore.step(5, &counts(&[("Chrome", 7)])), Step::Nothing);
        assert!(!restore.is_running());
    }
}
