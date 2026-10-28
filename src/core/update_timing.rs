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
}
