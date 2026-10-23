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
/// How long a group counts as "being moved" after its restore: the buttons
/// are re-added on the tick that decides, the windows move on the next.
const MOVING_SECONDS: u64 = 2;
/// The longest an app's line stays on the please-wait panel after its last
/// new window: an app that opens some of its windows and then no more (or
/// whose open window flickers) must not hold the panel.
pub const LINE_CAP_SECONDS: u64 = 20;

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

/// Where a group stands, for the notice that asks the person to wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Saved, but the app has opened no window yet.
    NotOpened,
    /// Windows are opening; the group is restored when their number holds.
    Settling,
    /// The group was just restored and its windows are being moved.
    Moving,
    /// A restored group is getting more windows, which will be moved too.
    LateWindows,
    /// Restored; the watch for late windows may still be running.
    Done,
    /// Has windows open but no new one for `LINE_CAP_SECONDS`, and is not
    /// restored yet; it waits without a line on the panel.
    Quiet,
    /// Dropped: it never opened a window, or lost the ones it had.
    Skipped,
}

impl Phase {
    /// Phases the person should wait for.
    pub fn is_active(self) -> bool {
        matches!(self, Phase::Settling | Phase::Moving | Phase::LateWindows)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub group: String,
    pub phase: Phase,
    /// Windows the group has now (or had when it was restored).
    pub count: usize,
    /// Windows saved for the group.
    pub expected: usize,
}

/// A group that has not had its first restore yet.
#[derive(Clone, Copy, Debug)]
struct Waiting {
    count: usize,
    stable_since: u64,
    /// When the count first became more than zero.
    first_seen: Option<u64>,
    /// The most windows seen at once, and when that number last grew: a
    /// window that closes and opens again is not a new one.
    most: usize,
    grew_at: Option<u64>,
}

/// A group that was restored and may still get late windows.
#[derive(Clone, Copy, Debug)]
struct Watching {
    /// The window count when the group was restored.
    baseline: usize,
    count: usize,
    stable_since: u64,
    until: u64,
    /// The most windows seen above the baseline, and when that last grew.
    most: usize,
    grew_at: Option<u64>,
}

/// Keeps `most` and `grew_at` up to date with a new count.
fn note_growth(most: &mut usize, grew_at: &mut Option<u64>, count: usize, tick: u64) {
    if count > *most {
        *most = count;
        *grew_at = Some(tick);
    }
}

/// Whether a line for a group whose window count last grew at `grew_at` may
/// still be on the panel at `tick`.
pub fn line_shown(grew_at: Option<u64>, tick: u64) -> bool {
    grew_at.is_some_and(|at| tick.saturating_sub(at) < LINE_CAP_SECONDS)
}

#[derive(Clone, Debug)]
pub struct Restore {
    started: Option<u64>,
    deadline_passed: bool,
    waiting: BTreeMap<String, Waiting>,
    watching: BTreeMap<String, Watching>,
    by_limit: Vec<String>,
    /// Groups restored in this run, with the windows they had when they were
    /// last restored; kept after the watch ends for the final summary.
    done: BTreeMap<String, usize>,
    /// The tick of each group's latest restore.
    moved_at: BTreeMap<String, u64>,
    skipped: BTreeSet<String>,
    restored: usize,
    /// Windows saved for each group. A group with all of them open is
    /// restored at once, without waiting for its count to hold still.
    expected: BTreeMap<String, usize>,
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
            done: BTreeMap::new(),
            moved_at: BTreeMap::new(),
            skipped: BTreeSet::new(),
            restored: 0,
            expected: BTreeMap::new(),
            settle_seconds,
            give_up_seconds,
        }
    }

    /// Starts waiting for `groups`, the ones with saved windows.
    pub fn start(&mut self, tick: u64, groups: Vec<String>) {
        self.started = Some(tick);
        self.expected.clear();
        self.deadline_passed = false;
        self.restored = 0;
        self.by_limit.clear();
        self.done.clear();
        self.moved_at.clear();
        self.skipped.clear();
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
                        most: 0,
                        grew_at: None,
                    },
                )
            })
            .collect();
    }

    /// How many windows are saved for each group, set after `start`.
    pub fn expect(&mut self, expected: BTreeMap<String, usize>) {
        self.expected = expected;
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

    /// Where every group of this run stands at `tick`, given the window
    /// counts now and the number of windows saved for each group. Also valid
    /// after the run ended, for its summary.
    pub fn progress(
        &self,
        tick: u64,
        counts: &BTreeMap<String, usize>,
        expected: &BTreeMap<String, usize>,
    ) -> Vec<Progress> {
        let now = |group: &String| counts.get(group).copied().unwrap_or(0);
        let wanted = |group: &String| expected.get(group).copied().unwrap_or(0);
        let just_moved = |group: &String| {
            self.moved_at
                .get(group)
                .is_some_and(|at| tick.saturating_sub(*at) < MOVING_SECONDS)
        };
        let mut all = Vec::new();
        for (group, watch) in &self.waiting {
            // An app with no window open has nothing to show, whether it
            // never opened one or closed the one it had.
            let (phase, count) = match now(group) {
                0 => (Phase::NotOpened, 0),
                count if line_shown(watch.grew_at, tick) => (Phase::Settling, count),
                count => (Phase::Quiet, count),
            };
            all.push(Progress {
                group: group.clone(),
                phase,
                count,
                expected: wanted(group),
            });
        }
        for (group, restored_count) in &self.done {
            let (phase, count) = match self.watching.get(group) {
                _ if just_moved(group) => (Phase::Moving, now(group).max(*restored_count)),
                Some(watch) if now(group) > watch.baseline && line_shown(watch.grew_at, tick) => {
                    (Phase::LateWindows, now(group))
                }
                _ => (Phase::Done, *restored_count),
            };
            all.push(Progress {
                group: group.clone(),
                phase,
                count,
                expected: wanted(group),
            });
        }
        for group in &self.skipped {
            all.push(Progress {
                group: group.clone(),
                phase: Phase::Skipped,
                count: 0,
                expected: wanted(group),
            });
        }
        all.sort_by(|a, b| a.group.cmp(&b.group));
        all
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
        let deadline = started + self.give_up_seconds;
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
                self.skipped.insert(group.clone());
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
                    self.done.insert(group.clone(), watch.count);
                    self.moved_at.insert(group.clone(), tick);
                }
                finished.push(group.clone());
                continue;
            }
            let count = count_of(group);
            if count != watch.count {
                watch.count = count;
                watch.stable_since = tick;
            }
            note_growth(&mut watch.most, &mut watch.grew_at, count, tick);
            let settled = count > watch.baseline
                && tick.saturating_sub(watch.stable_since) >= self.settle_seconds;
            if settled {
                step.due.push(Due {
                    group: group.clone(),
                    pass: Pass::Late,
                });
                self.done.insert(group.clone(), count);
                self.moved_at.insert(group.clone(), tick);
                // Watched again from here, so windows that come later
                // still get their order, but never past the run's deadline.
                watch.baseline = count;
                watch.most = count;
                watch.grew_at = None;
                watch.until = (tick + SECOND_PASS_SECONDS).min(deadline).max(tick + 1);
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
            note_growth(&mut watch.most, &mut watch.grew_at, count, tick);
            let Some(first_seen) = watch.first_seen else {
                continue;
            };
            let past_limit = tick.saturating_sub(first_seen) >= GROUP_LIMIT_SECONDS;
            let complete = self
                .expected
                .get(group)
                .is_some_and(|expected| *expected > 0 && count >= *expected);
            let settled = count > 0
                && (complete || tick.saturating_sub(watch.stable_since) >= self.settle_seconds);
            if settled || (count > 0 && past_limit) {
                let pass = if settled { Pass::Settled } else { Pass::Limit };
                restored_now.push((group.clone(), pass, count));
            } else if past_limit {
                closed_again.push(group.clone());
            }
        }
        for group in closed_again {
            self.waiting.remove(&group);
            self.skipped.insert(group);
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
                    most: count,
                    grew_at: None,
                },
            );
            if pass == Pass::Limit {
                self.by_limit.push(group.clone());
            }
            self.restored += 1;
            self.done.insert(group.clone(), count);
            self.moved_at.insert(group.clone(), tick);
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
        // Watched 5 more minutes from the second pass, then left alone.
        assert_eq!(restore.pending(), ["Chrome"]);
        assert!(drive(&mut restore, 153..=451, at).is_empty());
        assert!(restore.pending().is_empty());
        assert!(!restore.is_running());
    }

    #[test]
    fn every_batch_of_late_windows_gets_its_own_pass_until_the_deadline() {
        let mut restore = Restore::new(3, 400);
        restore.start(0, vec!["Chrome".into()]);
        let at = |tick: u64| {
            let count = match tick {
                0..=49 => 5,
                50..=99 => 6,
                100..=389 => 8,
                _ => 9,
            };
            counts(&[("Chrome", count)])
        };
        let events = drive(&mut restore, 1..=450, at);
        assert_eq!(
            events,
            [
                (4, due(&[("Chrome", Pass::Settled)])),
                (53, due(&[("Chrome", Pass::Late)])),
                (103, due(&[("Chrome", Pass::Late)])),
                (393, due(&[("Chrome", Pass::Late)])),
            ]
        );
        assert!(
            !restore.is_running(),
            "the watch ends at the run's deadline"
        );
    }

    #[test]
    fn a_group_with_all_its_saved_windows_open_is_restored_at_once() {
        let mut restore = Restore::new(3, 1800);
        restore.start(0, vec!["Chrome".into(), "Telegram".into()]);
        restore.expect(counts(&[("Chrome", 3), ("Telegram", 1)]));
        assert_eq!(restore.step(1, &counts(&[("Chrome", 2)])), Step::default());
        assert_eq!(
            restore.step(2, &counts(&[("Chrome", 3), ("Telegram", 1)])),
            due(&[("Chrome", Pass::Settled), ("Telegram", Pass::Settled)])
        );
    }

    #[test]
    fn an_app_whose_window_never_matches_or_flickers_loses_its_line_after_the_cap() {
        let mut restore = Restore::new(3, 1800);
        restore.start(0, vec!["ShareX".into(), "Surfshark".into()]);
        let saved = expected(&[("ShareX", 1), ("Surfshark", 1)]);
        let phase = |restore: &Restore, tick: u64, now: &[(&str, usize)]| {
            restore
                .progress(tick, &counts(now), &saved)
                .into_iter()
                .map(|entry| (entry.group, entry.phase))
                .collect::<Vec<_>>()
        };
        // Surfshark opens a window for one second and hides it again; ShareX's
        // window comes and goes every other second, so it never holds still.
        let at = |tick: u64| {
            let mut now = vec![];
            if tick.is_multiple_of(2) {
                now.push(("ShareX", 1));
            }
            if tick == 5 {
                now.push(("Surfshark", 1));
            }
            counts(&now)
        };
        drive(&mut restore, 1..=10, at);
        assert_eq!(
            phase(&restore, 10, &[("ShareX", 1)]),
            [
                ("ShareX".to_string(), Phase::Settling),
                ("Surfshark".to_string(), Phase::NotOpened)
            ]
        );
        drive(&mut restore, 11..=22, at);
        assert_eq!(
            phase(&restore, 22, &[("ShareX", 1)])[0],
            ("ShareX".to_string(), Phase::Quiet),
            "its window opened first at 2 and never grew in number"
        );
        assert!(restore
            .progress(22, &at(22), &saved)
            .iter()
            .all(|e| !e.phase.is_active()));
    }

    #[test]
    fn a_line_is_shown_only_for_a_while_after_the_last_new_window() {
        assert!(!line_shown(None, 5));
        assert!(line_shown(Some(5), 5));
        assert!(line_shown(Some(5), 5 + LINE_CAP_SECONDS - 1));
        assert!(!line_shown(Some(5), 5 + LINE_CAP_SECONDS));
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

    fn expected(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
        counts(pairs)
    }

    fn phases(restore: &Restore, tick: u64, now: &[(&str, usize)]) -> Vec<(String, Phase, usize)> {
        restore
            .progress(
                tick,
                &counts(now),
                &expected(&[("Chrome", 22), ("Telegram", 1)]),
            )
            .into_iter()
            .map(|entry| (entry.group, entry.phase, entry.count))
            .collect()
    }

    fn entry(group: &str, phase: Phase, count: usize) -> (String, Phase, usize) {
        (group.to_string(), phase, count)
    }

    #[test]
    fn progress_follows_a_group_from_not_opened_to_done() {
        let mut restore = Restore::new(15, 100);
        restore.start(0, vec!["Chrome".into(), "Ghost".into()]);
        let none: [(&str, usize); 0] = [];
        restore.step(1, &counts(&none));
        assert_eq!(
            phases(&restore, 1, &none),
            [
                entry("Chrome", Phase::NotOpened, 0),
                entry("Ghost", Phase::NotOpened, 0)
            ]
        );

        // Windows are opening.
        restore.step(5, &counts(&[("Chrome", 12)]));
        assert_eq!(
            phases(&restore, 5, &[("Chrome", 12)]),
            [
                entry("Chrome", Phase::Settling, 12),
                entry("Ghost", Phase::NotOpened, 0)
            ]
        );
        let progress =
            restore.progress(5, &counts(&[("Chrome", 12)]), &expected(&[("Chrome", 22)]));
        assert_eq!(progress[0].expected, 22);

        // The count holds at 22 from tick 6, so the group settles at 21:
        // moving for two ticks, then done.
        restore.step(6, &counts(&[("Chrome", 22)]));
        let step = restore.step(21, &counts(&[("Chrome", 22)]));
        assert_eq!(step.due.len(), 1);
        for tick in [21, 22] {
            assert_eq!(
                phases(&restore, tick, &[("Chrome", 22)])[0],
                entry("Chrome", Phase::Moving, 22)
            );
        }
        assert_eq!(
            phases(&restore, 23, &[("Chrome", 22)])[0],
            entry("Chrome", Phase::Done, 22)
        );
    }

    #[test]
    fn progress_shows_late_windows_and_their_second_pass() {
        let mut restore = Restore::new(15, 1800);
        restore.start(0, vec!["Chrome".into()]);
        restore.step(1, &counts(&[("Chrome", 5)]));
        restore.step(16, &counts(&[("Chrome", 5)]));
        assert_eq!(
            phases(&restore, 30, &[("Chrome", 5)]),
            [entry("Chrome", Phase::Done, 5)]
        );
        // A window comes late: the group is active again.
        restore.step(100, &counts(&[("Chrome", 6)]));
        assert_eq!(
            phases(&restore, 100, &[("Chrome", 6)]),
            [entry("Chrome", Phase::LateWindows, 6)]
        );
        // The second pass moves it, and then the group is done at 6 windows.
        let step = restore.step(115, &counts(&[("Chrome", 6)]));
        assert_eq!(step, due(&[("Chrome", Pass::Late)]));
        assert_eq!(
            phases(&restore, 115, &[("Chrome", 6)]),
            [entry("Chrome", Phase::Moving, 6)]
        );
        assert_eq!(
            phases(&restore, 117, &[("Chrome", 6)]),
            [entry("Chrome", Phase::Done, 6)]
        );
    }

    #[test]
    fn progress_keeps_the_summary_after_the_run_and_names_what_was_skipped() {
        let mut restore = Restore::new(15, 100);
        restore.start(0, vec!["Chrome".into(), "Ghost".into()]);
        restore.step(1, &counts(&[("Chrome", 3)]));
        restore.step(16, &counts(&[("Chrome", 3)]));
        let step = restore.step(100, &counts(&[("Chrome", 3)]));
        assert!(step.timed_out.is_some());
        assert_eq!(
            phases(&restore, 101, &[("Chrome", 3)]),
            [
                entry("Chrome", Phase::Done, 3),
                entry("Ghost", Phase::Skipped, 0)
            ]
        );
        // The watch for late windows ends at 316; the summary stays.
        restore.step(320, &counts(&[("Chrome", 3)]));
        assert!(!restore.is_running());
        assert_eq!(
            phases(&restore, 330, &[]),
            [
                entry("Chrome", Phase::Done, 3),
                entry("Ghost", Phase::Skipped, 0)
            ]
        );
        // A new run starts from nothing.
        restore.start(400, vec!["Telegram".into()]);
        assert_eq!(
            phases(&restore, 401, &[]),
            [entry("Telegram", Phase::NotOpened, 0)]
        );
    }

    #[test]
    fn only_settling_moving_and_late_phases_are_active() {
        assert!(Phase::Settling.is_active());
        assert!(Phase::Moving.is_active());
        assert!(Phase::LateWindows.is_active());
        assert!(!Phase::NotOpened.is_active());
        assert!(!Phase::Done.is_active());
        assert!(!Phase::Skipped.is_active());
    }
}
