//! When Explorer restarts, it rebuilds every taskbar button in its own order.
//! This decides when to put LayoutKeeper's order back: not at once, because
//! the taskbar is still being built, but a few seconds later, and once more
//! if the taskbar was slow or windows were still coming.

/// Seconds after the restart before the first try.
const FIRST_AFTER: u64 = 5;
/// Seconds after the restart for the second try, and the time the taskbar
/// has to come back before WinCraft gives up.
const SECOND_AFTER: u64 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    /// Put the order back now.
    Apply,
    /// The taskbar did not come back in time; leave it alone.
    GiveUp,
}

#[derive(Clone, Copy, Debug)]
struct First {
    /// How many windows there were at the first try.
    windows: usize,
    /// The taskbar was not there at the first moment it was looked for.
    late: bool,
}

#[derive(Debug, Default)]
pub struct Rebuild {
    since: Option<u64>,
    first: Option<First>,
}

impl Rebuild {
    /// Explorer restarted at this tick. A second restart starts the wait
    /// again.
    pub fn restarted(&mut self, tick: u64) {
        self.since = Some(tick);
        self.first = None;
    }

    /// Whether `step` has anything to decide yet, so the caller looks at the
    /// windows only when it must.
    pub fn due(&self, tick: u64) -> bool {
        self.since
            .is_some_and(|since| tick.saturating_sub(since) >= FIRST_AFTER)
    }

    /// What to do at this tick, given whether the taskbar window exists and
    /// how many windows there are.
    pub fn step(&mut self, tick: u64, taskbar_back: bool, windows: usize) -> Step {
        let Some(since) = self.since else {
            return Step::Wait;
        };
        let elapsed = tick.saturating_sub(since);
        match self.first {
            None if elapsed < FIRST_AFTER => Step::Wait,
            None if !taskbar_back => {
                if elapsed >= SECOND_AFTER {
                    *self = Self::default();
                    Step::GiveUp
                } else {
                    Step::Wait
                }
            }
            None => {
                self.first = Some(First {
                    windows,
                    late: elapsed > FIRST_AFTER,
                });
                Step::Apply
            }
            Some(_) if elapsed < SECOND_AFTER => Step::Wait,
            Some(first) => {
                *self = Self::default();
                if first.late || first.windows != windows {
                    Step::Apply
                } else {
                    Step::Wait
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_happens_without_a_restart() {
        let mut rebuild = Rebuild::default();
        assert!(!rebuild.due(100));
        assert_eq!(rebuild.step(100, true, 5), Step::Wait);
    }

    #[test]
    fn the_order_goes_back_five_seconds_after_the_restart() {
        let mut rebuild = Rebuild::default();
        rebuild.restarted(100);
        assert!(!rebuild.due(104));
        assert_eq!(rebuild.step(104, true, 5), Step::Wait);
        assert!(rebuild.due(105));
        assert_eq!(rebuild.step(105, true, 5), Step::Apply);
        // Once only, when nothing changed since.
        assert_eq!(rebuild.step(106, true, 5), Step::Wait);
        assert_eq!(rebuild.step(115, true, 5), Step::Wait);
        assert!(!rebuild.due(120), "the wait is over");
    }

    #[test]
    fn it_tries_again_at_fifteen_seconds_when_windows_were_still_coming() {
        let mut rebuild = Rebuild::default();
        rebuild.restarted(0);
        assert_eq!(rebuild.step(5, true, 20), Step::Apply);
        assert_eq!(rebuild.step(14, true, 23), Step::Wait);
        assert_eq!(rebuild.step(15, true, 23), Step::Apply);
        assert!(!rebuild.due(16));
    }

    #[test]
    fn a_taskbar_that_came_back_late_gets_a_second_try() {
        let mut rebuild = Rebuild::default();
        rebuild.restarted(0);
        assert_eq!(rebuild.step(5, false, 8), Step::Wait);
        assert_eq!(rebuild.step(8, true, 8), Step::Apply);
        assert_eq!(rebuild.step(15, true, 8), Step::Apply);
    }

    #[test]
    fn it_gives_up_when_the_taskbar_does_not_come_back() {
        let mut rebuild = Rebuild::default();
        rebuild.restarted(0);
        assert_eq!(rebuild.step(14, false, 8), Step::Wait);
        assert_eq!(rebuild.step(15, false, 8), Step::GiveUp);
        assert!(!rebuild.due(16));
    }

    #[test]
    fn a_second_restart_starts_the_wait_again() {
        let mut rebuild = Rebuild::default();
        rebuild.restarted(0);
        assert_eq!(rebuild.step(5, true, 8), Step::Apply);
        rebuild.restarted(9);
        assert_eq!(rebuild.step(13, true, 8), Step::Wait);
        assert_eq!(rebuild.step(14, true, 8), Step::Apply);
    }
}
