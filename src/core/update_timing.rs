//! When WinCraft asks GitHub for a newer release on its own: each time the
//! hourly timer comes round, about 30 seconds after the PC wakes from sleep,
//! and when the palette opens after a quiet while. The user's switch for
//! automatic checks turns all three off; About's "Check for updates" is not
//! one of these and always asks.

use std::time::Duration;

/// The timer that asks GitHub.
pub const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// A wake or a palette opening asks only when no check started for this
/// long, so sleeping and waking often, or opening the palette all day, does
/// not ask GitHub again and again.
pub const RECHECK_AFTER: Duration = Duration::from_secs(15 * 60);
/// After a wake the network needs a moment to come back.
pub const WAKE_DELAY: Duration = Duration::from_secs(30);

/// What asks for an automatic check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// The hourly timer: it asks every time it comes round.
    Timer,
    /// The PC woke from sleep a moment ago.
    Wake,
    /// The palette opened. The answer shows from the next opening, so the
    /// list never changes while the user looks at it.
    PaletteOpen,
}

/// Whether `trigger` should start a check now. `last` is when the last
/// check started (or, before any this run, when GitHub last answered), in
/// unix seconds, 0 for never. A clock that went back counts as due, or the
/// checks would stop for as long.
pub fn due(trigger: Trigger, now: u64, last: u64, automatic: bool, checking: bool) -> bool {
    if !automatic || checking {
        return false;
    }
    match trigger {
        Trigger::Timer => true,
        Trigger::Wake | Trigger::PaletteOpen => {
            last == 0 || now < last || now - last >= RECHECK_AFTER.as_secs()
        }
    }
}

/// The most requests WinCraft sends to GitHub's API in any hour. GitHub
/// allows 60 an hour without a login, shared by every program and PC
/// behind the same router, so WinCraft keeps to a tenth of that.
pub const API_CALLS_PER_HOUR: usize = 6;
/// Automatic checks stop at this many, so what the user asks for still has
/// requests left.
pub const AUTOMATIC_API_CALLS: usize = 4;
const HOUR: u64 = 60 * 60;

/// The requests sent to GitHub's API in the last hour, in this run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApiBudget {
    calls: Vec<u64>,
}

impl ApiBudget {
    /// How many requests went out in the hour before `now`.
    pub fn used(&mut self, now: u64) -> usize {
        // A clock that went back keeps nothing: better one request too many
        // than checks that stop for as long.
        self.calls.retain(|&at| at <= now && now - at < HOUR);
        self.calls.len()
    }

    /// Whether one more request may go now: automatic checks stop earlier
    /// than what the user asks for.
    pub fn allows(&mut self, now: u64, asked_by_user: bool) -> bool {
        let most = if asked_by_user {
            API_CALLS_PER_HOUR
        } else {
            AUTOMATIC_API_CALLS
        };
        self.used(now) < most
    }

    pub fn spend(&mut self, now: u64) {
        self.calls.push(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;
    const MINUTE: u64 = 60;

    #[test]
    fn the_timer_asks_every_hour_it_comes_round() {
        assert_eq!(CHECK_EVERY.as_secs(), 60 * MINUTE);
        assert!(due(Trigger::Timer, NOW, NOW - MINUTE, true, false));
        assert!(due(Trigger::Timer, NOW, 0, true, false));
    }

    #[test]
    fn a_wake_or_an_opening_asks_only_after_fifteen_quiet_minutes() {
        for trigger in [Trigger::Wake, Trigger::PaletteOpen] {
            assert!(
                due(trigger, NOW, 0, true, false),
                "{trigger:?}: never checked"
            );
            assert!(
                !due(trigger, NOW, NOW - 14 * MINUTE, true, false),
                "{trigger:?}"
            );
            assert!(
                due(trigger, NOW, NOW - 15 * MINUTE, true, false),
                "{trigger:?}"
            );
            assert!(
                due(trigger, NOW, NOW + MINUTE, true, false),
                "{trigger:?}: clock went back"
            );
        }
        assert_eq!(WAKE_DELAY.as_secs(), 30);
    }

    #[test]
    fn nothing_asks_when_switched_off_or_while_a_check_runs() {
        for trigger in [Trigger::Timer, Trigger::Wake, Trigger::PaletteOpen] {
            assert!(
                !due(trigger, NOW, 0, false, false),
                "{trigger:?}: switched off"
            );
            assert!(
                !due(trigger, NOW, 0, true, true),
                "{trigger:?}: one is running"
            );
        }
    }

    #[test]
    fn the_api_budget_is_six_an_hour_and_four_for_automatic_checks() {
        let mut budget = ApiBudget::default();
        for minute in 0..4 {
            assert!(budget.allows(NOW + minute * MINUTE, false));
            budget.spend(NOW + minute * MINUTE);
        }
        let later = NOW + 5 * MINUTE;
        assert!(
            !budget.allows(later, false),
            "automatic checks stop at four"
        );
        assert!(budget.allows(later, true), "the user still has two");
        budget.spend(later);
        budget.spend(later);
        assert!(!budget.allows(later, true), "six is the most in an hour");
        assert_eq!(budget.used(later), API_CALLS_PER_HOUR);
        assert_eq!(
            budget.used(NOW + 60 * MINUTE),
            5,
            "the first one is an hour old"
        );
        assert!(budget.allows(NOW + 64 * MINUTE, false));
        assert_eq!(budget.used(NOW + 66 * MINUTE), 0);
    }

    #[test]
    fn a_clock_that_went_back_forgets_the_budget() {
        let mut budget = ApiBudget::default();
        for _ in 0..6 {
            budget.spend(NOW);
        }
        assert!(budget.allows(NOW - MINUTE, true));
    }

    #[test]
    fn the_hourly_timer_and_fifteen_minute_rechecks_fit_the_automatic_budget() {
        // The most a quiet hour can ask: the timer once, and a wake or an
        // opening every fifteen minutes after any check.
        let most = 1 + (CHECK_EVERY.as_secs() / RECHECK_AFTER.as_secs()) as usize;
        assert!(most - 1 <= AUTOMATIC_API_CALLS, "{most}");
    }
}
