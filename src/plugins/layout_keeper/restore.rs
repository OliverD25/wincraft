//! When to restore each taskbar group. Runs on the plugin's one-second tick;
//! the restore itself touches the system and lives in the plugin.
//!
//! Apps open their windows at their own pace after sign-in: Telegram half a
//! minute later, Chrome in background mode only when the person first opens
//! it. So each group has its own clock, which starts when its first window
//! appears. The group is restored once its window count has held still for
//! `settle_seconds`, or `GROUP_LIMIT_SECONDS` after that first window if the
//! count never held still. A group that opens nothing is dropped
//! `give_up_seconds` after the start. After its restore a group is watched for
//! `SECOND_PASS_SECONDS` more: windows that arrive late and then hold still
//! are restored in a second pass. An app opened after all that is left where
//! the user put it.

use std::collections::{BTreeMap, BTreeSet};

/// How long a group's window count may keep changing after its first window
/// before the group is restored with what is there.
pub const GROUP_LIMIT_SECONDS: u64 = 240;
/// How long a restored group is watched for late windows.
pub const SECOND_PASS_SECONDS: u64 = 300;

/// Why a group is restored now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// First restore: the window count held still.
    Settled,
    /// First restore: the count never held still, so the time limit forced it.
    Limit,
    /// Second restore: windows came late and held still, or the watch ended
    /// while they were still arriving.
    Late,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub group: String,
    pub pass: Pass,
}

/// Groups dropped because the overall time is up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimedOut {
    /// Groups that never opened a window.
    pub never_opened: Vec<String>,
    /// Groups this run restored only because of the time limit.
    pub by_limit: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Groups to restore now.
    pub due: Vec<Due>,
    pub timed_out: Option<TimedOut>,
}

/// A group that has not had its first restore yet.
#[derive(Clone, Copy, Debug)]
struct Waiting {
    count: usize,
    stable_since: u64,
    /// When the count first became more than zero.
    first_seen: Option<u64>,
}

/// A group that was restored and may still get late windows.
#[derive(Clone, Copy, Debug)]
struct Watching {
    /// The window count when the group was restored.
    baseline: usize,
    count: usize,
    stable_since: u64,
    until: u64,
}

#[derive(Clone, Debug)]
pub struct Restore {
    started: Option<u64>,
    deadline_passed: bool,
    waiting: BTreeMap<String, Waiting>,
    watching: BTreeMap<String, Watching>,
    by_limit: Vec<String>,
    restored: usize,
    pub settle_seconds: u64,
    pub give_up_seconds: u64,
}

impl Restore {
    pub fn new(settle_seconds: u64, give_up_seconds: u64) -> Self {
        Self {
            started: None,
            deadline_passed: false,
            waiting: BTreeMap::new(),
            watching: BTreeMap::new(),
            by_limit: Vec::new(),
            restored: 0,
            settle_seconds,
            give_up_seconds,
        }
    }

    /// Starts waiting for `groups`, the ones with saved windows.
    pub fn start(&mut self, tick: u64, groups: Vec<String>) {
        self.started = Some(tick);
        self.deadline_passed = false;
        self.restored = 0;
        self.by_limit.clear();
        self.watching.clear();
        self.waiting = groups
            .into_iter()
            .map(|group| {
                (
                    group,
                    Waiting {
                        count: 0,
                        stable_since: tick,
                        first_seen: None,
                    },
                )
            })
            .collect();
    }

    /// True while any group is still waited for or watched for late windows.
    pub fn is_running(&self) -> bool {
        self.started.is_some()
    }

    /// The groups whose saved windows must not be overwritten by the
    /// scrambled ones on screen: those not restored yet and those still
    /// watched for late windows.
    pub fn pending(&self) -> Vec<String> {
        let all: BTreeSet<&String> = self.waiting.keys().chain(self.watching.keys()).collect();
        all.into_iter().cloned().collect()
    }

    /// How many groups have not had their first restore yet.
    pub fn waiting_count(&self) -> usize {
        self.waiting.len()
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
            return Step::default();
        };
        let count_of = |group: &String| counts.get(group).copied().unwrap_or(0);
        let mut step = Step::default();

        if !self.deadline_passed && tick.saturating_sub(started) >= self.give_up_seconds {
            self.deadline_passed = true;
            let never_opened: Vec<String> = self
                .waiting
                .iter()
                .filter(|(_, watch)| watch.first_seen.is_none())
                .map(|(group, _)| group.clone())
                .collect();
            for group in &never_opened {
                self.waiting.remove(group);
            }
            if !never_opened.is_empty() {
                step.timed_out = Some(TimedOut {
                    never_opened,
                    by_limit: self.by_limit.clone(),
                });
            }
        }

        let mut finished = Vec::new();
        for (group, watch) in self.watching.iter_mut() {
            if tick >= watch.until {
                // Judged by the last tick's count: a window that appears
                // only now is too late.
                if watch.count > watch.baseline {
                    step.due.push(Due {
                        group: group.clone(),
                        pass: Pass::Late,
                    });
                }
                finished.push(group.clone());
                continue;
            }
            let count = count_of(group);
            if count != watch.count {
                watch.count = count;
                watch.stable_since = tick;
            }
            let settled = count > watch.baseline
                && tick.saturating_sub(watch.stable_since) >= self.settle_seconds;
            if settled {
                step.due.push(Due {
                    group: group.clone(),
                    pass: Pass::Late,
                });
                finished.push(group.clone());
            }
        }
        for group in &finished {
            self.watching.remove(group);
        }

        let mut closed_again = Vec::new();
        let mut restored_now = Vec::new();
        for (group, watch) in self.waiting.iter_mut() {
            let count = count_of(group);
            if count != watch.count {
                watch.count = count;
                watch.stable_since = tick;
            }
            if count > 0 && watch.first_seen.is_none() {
                watch.first_seen = Some(tick);
            }
            let Some(first_seen) = watch.first_seen else {
                continue;
            };
            let past_limit = tick.saturating_sub(first_seen) >= GROUP_LIMIT_SECONDS;
            let settled =
                count > 0 && tick.saturating_sub(watch.stable_since) >= self.settle_seconds;
            if settled || (count > 0 && past_limit) {
                let pass = if settled { Pass::Settled } else { Pass::Limit };
                restored_now.push((group.clone(), pass, count));
            } else if past_limit {
                closed_again.push(group.clone());
            }
        }
        for group in closed_again {
            self.waiting.remove(&group);
        }
        for (group, pass, count) in restored_now {
            self.waiting.remove(&group);
            self.watching.insert(
                group.clone(),
                Watching {
                    baseline: count,
                    count,
                    stable_since: tick,
                    until: tick + SECOND_PASS_SECONDS,
                },
            );
            if pass == Pass::Limit {
                self.by_limit.push(group.clone());
            }
            self.restored += 1;
            step.due.push(Due { group, pass });
        }

        if self.waiting.is_empty() && self.watching.is_empty() {
            self.started = None;
        }
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::RangeInclusive;

    fn counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
        pairs
            .iter()
            .map(|(group, count)| (group.to_string(), *count))
            .collect()
    }

    fn due(pairs: &[(&str, Pass)]) -> Step {
        Step {
            due: pairs
                .iter()
                .map(|(group, pass)| Due {
                    group: group.to_string(),
                    pass: *pass,
                })
                .collect(),
            timed_out: None,
        }
    }

    /// Steps through `ticks` with the counts `counts_at` gives, and returns
    /// every tick that asked for something.
    fn drive(
        restore: &mut Restore,
        ticks: RangeInclusive<u64>,
        counts_at: impl Fn(u64) -> BTreeMap<String, usize>,
    ) -> Vec<(u64, Step)> {
        ticks
            .filter_map(|tick| {
                let step = restore.step(tick, &counts_at(tick));
                (step != Step::default()).then_some((tick, step))
            })
            .collect()
    }

    /// Chrome in background mode: its first window at 95 s, then one more
    /// every 3 to 10 s until 22 are open at 199 s.
    fn chrome_opening(tick: u64) -> usize {
        const GAPS: [u64; 21] = [
            3, 5, 7, 4, 6, 5, 3, 10, 4, 5, 6, 5, 3, 7, 4, 5, 6, 3, 5, 4, 4,
        ];
        let mut opened_at = 95;
        let mut count = 0;
        if tick >= opened_at {
            count = 1;
        }
        for gap in GAPS {
            opened_at += gap;
            if tick >= opened_at {
                count += 1;
            }
        }
        count
    }

    #[test]
    fn each_group_is_restored_once_its_own_count_holds_still() {
        let mut restore = Restore::new(3, 180);
        restore.start(0, vec!["Chrome".into(), "Telegram".into()]);
        assert_eq!(restore.step(1, &counts(&[("Chrome", 4)])), Step::default());
        assert_eq!(restore.step(2, &counts(&[("Chrome", 9)])), Step::default());
        assert_eq!(restore.step(4, &counts(&[("Chrome", 9)])), Step::default());
        assert_eq!(
            restore.step(5, &counts(&[("Chrome", 9)])),
            due(&[("Chrome", Pass::Settled)])
        );
        assert!(restore.is_running());
        assert_eq!(restore.waiting_count(), 1);

        // Telegram starts half a minute later.
        assert_eq!(
            restore.step(40, &counts(&[("Chrome", 9), ("Telegram", 1)])),
            Step::default()
        );
        assert_eq!(
            restore.step(43, &counts(&[("Chrome", 9), ("Telegram", 1)])),
            due(&[("Telegram", Pass::Settled)])
        );
        assert_eq!(restore.restored(), 2);
        assert_eq!(restore.waiting_count(), 0);
        // Both are still watched for late windows, so the run goes on.
        assert!(restore.is_running());
    }

    #[test]
    fn groups_that_settle_together_are_restored_together() {
        let mut restore = Restore::new(2, 180);
        restore.start(0, vec!["A".into(), "B".into()]);
        restore.step(1, &counts(&[("A", 1), ("B", 2)]));
        assert_eq!(
            restore.step(3, &counts(&[("A", 1), ("B", 2)])),
            due(&[("A", Pass::Settled), ("B", Pass::Settled)])
        );
    }

    #[test]
    fn an_app_that_opens_after_the_overall_time_is_left_alone() {
        let mut restore = Restore::new(3, 10);
        restore.start(100, vec!["Chrome".into(), "Telegram".into()]);
        restore.step(101, &counts(&[("Chrome", 2)]));
        assert_eq!(
            restore.step(104, &counts(&[("Chrome", 2)])),
            due(&[("Chrome", Pass::Settled)])
        );
        for tick in 105..110 {
            assert_eq!(
                restore.step(tick, &counts(&[("Chrome", 2)])),
                Step::default()
            );
        }
        let step = restore.step(110, &counts(&[("Telegram", 1)]));
        assert!(step.due.is_empty());
        assert_eq!(
            step.timed_out,
            Some(TimedOut {
                never_opened: vec!["Telegram".into()],
                by_limit: Vec::new(),
            })
        );
        assert_eq!(restore.pending(), ["Chrome"]);
        assert_eq!(
            restore.step(200, &counts(&[("Telegram", 1)])),
            Step::default()
        );
        assert!(!restore.pending().contains(&"Telegram".to_string()));
    }

    #[test]
    fn a_group_that_keeps_changing_waits() {
        let mut restore = Restore::new(3, 180);
        restore.start(0, vec!["Chrome".into()]);
        for (tick, count) in [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)] {
            assert_eq!(
                restore.step(tick, &counts(&[("Chrome", count)])),
                Step::default()
            );
        }
        assert_eq!(
            restore.step(8, &counts(&[("Chrome", 5)])),
            due(&[("Chrome", Pass::Settled)])
        );
    }

    #[test]
    fn nothing_happens_until_it_is_started() {
        let mut restore = Restore::new(3, 10);
        assert_eq!(restore.step(5, &counts(&[("Chrome", 7)])), Step::default());
        assert!(!restore.is_running());
    }

    #[test]
    fn chrome_that_opens_95_seconds_in_is_restored_when_its_windows_stop_coming() {
        let mut restore = Restore::new(15, 30 * 60);
        restore.start(0, vec!["Chrome".into(), "Telegram".into()]);
        let at = |tick: u64| counts(&[("Chrome", chrome_opening(tick))]);
        assert_eq!(chrome_opening(94), 0);
        assert_eq!(chrome_opening(199), 22);

        // Nothing is restored before Chrome has anything, or while its
        // windows are still coming.
        assert!(drive(&mut restore, 1..=213, at).is_empty());
        assert_eq!(restore.pending(), ["Chrome", "Telegram"]);

        // The last window opened at 199 s; 15 s of no change later it is time.
        let events = drive(&mut restore, 214..=215, at);
        assert_eq!(events, [(214, due(&[("Chrome", Pass::Settled)]))]);
        // Chrome is still protected (second-pass watch), and so is Telegram.
        assert_eq!(restore.pending(), ["Chrome", "Telegram"]);
        assert_eq!(restore.waiting_count(), 1);
    }

    #[test]
    fn an_app_that_never_opens_is_dropped_after_the_overall_time_and_named() {
        let mut restore = Restore::new(15, 30 * 60);
        restore.start(0, vec!["Chrome".into(), "Ghost".into()]);
        let at = |_| counts(&[("Chrome", 3)]);
        let events = drive(&mut restore, 1..=1799, at);
        assert_eq!(events, [(16, due(&[("Chrome", Pass::Settled)]))]);
        assert!(restore.pending().contains(&"Ghost".to_string()));

        let step = restore.step(1800, &at(1800));
        assert!(step.due.is_empty());
        assert_eq!(
            step.timed_out,
            Some(TimedOut {
                never_opened: vec!["Ghost".into()],
                by_limit: Vec::new(),
            })
        );
        assert!(!restore.pending().contains(&"Ghost".to_string()));
        // Chrome's second-pass watch ended long ago, so the run is over.
        assert!(!restore.is_running());
    }

    #[test]
    fn an_app_that_opens_at_minute_20_is_still_restored() {
        let mut restore = Restore::new(15, 30 * 60);
        restore.start(0, vec!["Chrome".into(), "Telegram".into()]);
        let at = |tick: u64| {
            if tick >= 1200 {
                counts(&[("Chrome", 5), ("Telegram", 1)])
            } else {
                counts(&[("Chrome", 5)])
            }
        };
        let events = drive(&mut restore, 1..=1300, at);
        assert_eq!(
            events,
            [
                (16, due(&[("Chrome", Pass::Settled)])),
                (1215, due(&[("Telegram", Pass::Settled)])),
            ]
        );
    }

    #[test]
    fn a_count_that_never_holds_still_is_restored_four_minutes_after_the_first_window() {
        let mut restore = Restore::new(15, 600);
        restore.start(0, vec!["Chrome".into(), "Ghost".into()]);
        // A window every 10 s from second 10, without end.
        let at = |tick: u64| counts(&[("Chrome", (tick / 10) as usize)]);
        let events = drive(&mut restore, 1..=249, at);
        assert!(events.is_empty());
        assert_eq!(restore.step(250, &at(250)), due(&[("Chrome", Pass::Limit)]));

        // Still growing when the 5-minute watch ends: restored once more.
        let events = drive(&mut restore, 251..=600, at);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], (550, due(&[("Chrome", Pass::Late)])));
        assert_eq!(events[1].0, 600);
        assert_eq!(
            events[1].1.timed_out,
            Some(TimedOut {
                never_opened: vec!["Ghost".into()],
                by_limit: vec!["Chrome".into()],
            })
        );
        assert!(!restore.is_running());
    }

    #[test]
    fn a_window_two_minutes_after_the_restore_gets_a_second_pass() {
        let mut restore = Restore::new(15, 1800);
        restore.start(0, vec!["Chrome".into()]);
        let at = |tick: u64| counts(&[("Chrome", if tick >= 136 { 6 } else { 5 })]);
        // Count 5 from the first tick: restored at 15 (stable since 1).
        assert_eq!(
            drive(&mut restore, 1..=135, at),
            [(16, due(&[("Chrome", Pass::Settled)]))]
        );
        assert_eq!(restore.pending(), ["Chrome"]);

        // The late window arrives at 136; 15 s later the group is done again.
        assert!(drive(&mut restore, 136..=150, at).is_empty());
        assert_eq!(
            drive(&mut restore, 151..=152, at),
            [(151, due(&[("Chrome", Pass::Late)]))]
        );
        assert!(restore.pending().is_empty());
        assert!(!restore.is_running());
    }

    #[test]
    fn a_window_after_the_second_pass_watch_is_left_alone() {
        let mut restore = Restore::new(15, 1800);
        restore.start(0, vec!["Chrome".into()]);
        // The watch started at 16 and ends at 316.
        let at = |tick: u64| counts(&[("Chrome", if tick >= 316 { 6 } else { 5 })]);
        let events = drive(&mut restore, 1..=400, at);
        assert_eq!(events, [(16, due(&[("Chrome", Pass::Settled)]))]);
        assert!(restore.pending().is_empty());
        assert!(!restore.is_running());
    }

    #[test]
    fn windows_still_arriving_when_the_watch_ends_are_restored_then() {
        let mut restore = Restore::new(15, 1800);
        restore.start(0, vec!["Chrome".into()]);
        let at = |tick: u64| counts(&[("Chrome", if tick >= 315 { 6 } else { 5 })]);
        let events = drive(&mut restore, 1..=400, at);
        assert_eq!(
            events,
            [
                (16, due(&[("Chrome", Pass::Settled)])),
                (316, due(&[("Chrome", Pass::Late)])),
            ]
        );
    }

    #[test]
    fn a_group_that_loses_its_windows_again_is_dropped_after_the_limit() {
        let mut restore = Restore::new(15, 1800);
        restore.start(0, vec!["Chrome".into()]);
        let at = |tick: u64| counts(&[("Chrome", if tick == 10 { 2 } else { 0 })]);
        assert!(drive(&mut restore, 1..=300, at).is_empty());
        assert!(restore.pending().is_empty());
        assert!(!restore.is_running());
    }

    #[test]
    fn a_group_is_protected_through_every_phase() {
        let mut restore = Restore::new(15, 1800);
        restore.start(0, vec!["Chrome".into()]);
        let at = |tick: u64| counts(&[("Chrome", if tick >= 100 { 3 } else { 0 })]);
        // Waiting for the first window.
        drive(&mut restore, 1..=99, at);
        assert_eq!(restore.pending(), ["Chrome"]);
        // Settling.
        drive(&mut restore, 100..=114, at);
        assert_eq!(restore.pending(), ["Chrome"]);
        // Restored at 115, now in the second-pass watch until 415.
        assert_eq!(
            restore.step(115, &at(115)),
            due(&[("Chrome", Pass::Settled)])
        );
        drive(&mut restore, 116..=414, at);
        assert_eq!(restore.pending(), ["Chrome"]);
        // The watch has ended.
        drive(&mut restore, 415..=416, at);
        assert!(restore.pending().is_empty());
    }

    #[test]
    fn starting_again_forgets_the_last_run() {
        let mut restore = Restore::new(3, 10);
        restore.start(0, vec!["A".into()]);
        restore.step(1, &counts(&[("A", 1)]));
        restore.step(4, &counts(&[("A", 1)]));
        assert_eq!(restore.restored(), 1);
        restore.start(50, vec!["B".into()]);
        assert_eq!(restore.restored(), 0);
        assert_eq!(restore.pending(), ["B"]);
    }
}
