/// When to restore. Runs on the plugin's one-second tick; the restore itself
/// touches the system and lives in the plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Settling {
        started: u64,
        count: usize,
        stable_since: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Wait,
    Apply,
    TimedOut,
}

#[derive(Clone, Copy, Debug)]
pub struct Restore {
    phase: Phase,
    pub settle_seconds: u64,
    pub give_up_seconds: u64,
}

impl Restore {
    pub fn new(settle_seconds: u64, give_up_seconds: u64) -> Self {
        Self {
            phase: Phase::Idle,
            settle_seconds,
            give_up_seconds,
        }
    }

    pub fn start(&mut self, tick: u64) {
        self.phase = Phase::Settling {
            started: tick,
            count: 0,
            stable_since: tick,
        };
    }

    pub fn is_running(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Feeds the number of watched windows seen at `tick`. The restore goes
    /// ahead once that number has held for `settle_seconds`; zero windows
    /// never count as settled, because right after sign-in WinCraft is often
    /// running before the browser has opened anything.
    pub fn step(&mut self, tick: u64, count: usize) -> Step {
        let Phase::Settling {
            started,
            count: last,
            stable_since,
        } = self.phase
        else {
            return Step::Nothing;
        };
        if tick.saturating_sub(started) >= self.give_up_seconds {
            self.phase = Phase::Idle;
            return Step::TimedOut;
        }
        let stable_since = if count == last { stable_since } else { tick };
        if count > 0 && tick.saturating_sub(stable_since) >= self.settle_seconds {
            self.phase = Phase::Idle;
            return Step::Apply;
        }
        self.phase = Phase::Settling {
            started,
            count,
            stable_since,
        };
        Step::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_waits_for_the_window_count_to_hold_still() {
        let mut restore = Restore::new(3, 180);
        restore.start(0);
        assert_eq!(restore.step(1, 4), Step::Wait);
        assert_eq!(restore.step(2, 9), Step::Wait);
        assert_eq!(restore.step(3, 9), Step::Wait);
        assert_eq!(restore.step(4, 9), Step::Wait);
        assert_eq!(restore.step(5, 9), Step::Apply);
        assert!(!restore.is_running());
        assert_eq!(restore.step(6, 9), Step::Nothing);
    }

    #[test]
    fn no_windows_is_never_settled_and_it_gives_up() {
        let mut restore = Restore::new(3, 10);
        restore.start(100);
        for tick in 101..110 {
            assert_eq!(restore.step(tick, 0), Step::Wait);
        }
        assert_eq!(restore.step(110, 0), Step::TimedOut);
        assert!(!restore.is_running());
    }

    #[test]
    fn nothing_happens_until_it_is_started() {
        let mut restore = Restore::new(3, 10);
        assert_eq!(restore.step(5, 7), Step::Nothing);
        assert!(!restore.is_running());
    }
}
