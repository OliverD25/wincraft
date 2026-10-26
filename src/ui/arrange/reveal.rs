//! Bringing the strip on screen in one step, fully drawn and in its place.
//!
//! winit shows a new window before it gives it its size, at a place Windows
//! picks, and before anything is drawn in it; the strip then jumped to its
//! place a frame later. So the strip's window is made hidden, and once it
//! exists it is moved to its place and cloaked (DWM keeps a cloaked window
//! off the screen, but it still draws), then shown. It is uncloaked when two
//! frames in a row began with the window already in place: the first of them
//! was laid out and drawn at the final size, and its DWM pictures were in.

use std::time::{Duration, Instant};

/// Never longer than this cloaked or waiting: a strip that cannot draw is
/// shown anyway rather than left invisible with the keyboard.
pub const LONGEST: Duration = Duration::from_millis(500);
/// Frames in a row that must begin with the window in place.
const SETTLED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reveal {
    /// On screen, or no strip at all.
    #[default]
    Shown,
    /// The strip was asked for; its hidden window may not exist yet.
    Waiting { since: Instant },
    /// Shown to Windows but cloaked, until it has drawn itself in place.
    Cloaked { since: Instant, settled: u8 },
}

impl Reveal {
    pub fn waiting(now: Instant) -> Self {
        Reveal::Waiting { since: now }
    }

    pub fn pending(&self) -> bool {
        !matches!(self, Reveal::Shown)
    }

    /// The hidden window was found: true when the caller is to place it,
    /// cloak it and show it.
    pub fn found(&mut self, now: Instant) -> bool {
        if !matches!(self, Reveal::Waiting { .. }) {
            return false;
        }
        *self = Reveal::Cloaked {
            since: now,
            settled: 0,
        };
        true
    }

    /// A strip frame begins, with the window `placed` where it belongs
    /// already or just moved there. True when it is to be uncloaked now.
    pub fn frame(&mut self, placed: bool, now: Instant) -> bool {
        let Reveal::Cloaked { since, settled } = self else {
            return false;
        };
        *settled = if placed { settled.saturating_add(1) } else { 0 };
        if *settled >= SETTLED || now.saturating_duration_since(*since) >= LONGEST {
            *self = Reveal::Shown;
            return true;
        }
        false
    }

    /// For the frames of the palette's thread, which go on when the strip
    /// does not draw: true when it took too long. A cloaked window is then
    /// to be uncloaked; a window never found has nothing to show.
    pub fn overdue(&mut self, now: Instant) -> bool {
        let since = match *self {
            Reveal::Shown => return false,
            Reveal::Waiting { since } | Reveal::Cloaked { since, .. } => since,
        };
        if now.saturating_duration_since(since) < LONGEST {
            return false;
        }
        *self = Reveal::Shown;
        true
    }

    /// How long until `overdue` is true.
    pub fn left(&self, now: Instant) -> Option<Duration> {
        match *self {
            Reveal::Shown => None,
            Reveal::Waiting { since } | Reveal::Cloaked { since, .. } => {
                Some(LONGEST.saturating_sub(now.saturating_duration_since(since)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn the_window_is_cloaked_once_found_and_shown_after_two_frames_in_place() {
        let start = Instant::now();
        let mut reveal = Reveal::waiting(start);
        assert!(reveal.pending());
        assert!(!reveal.frame(true, start), "nothing to uncloak yet");
        assert!(reveal.found(ms(start, 10)));
        assert!(!reveal.found(ms(start, 11)), "only once");
        assert!(!reveal.frame(true, ms(start, 20)), "the first frame");
        assert!(reveal.frame(true, ms(start, 36)), "drawn in place");
        assert_eq!(reveal, Reveal::Shown);
        assert!(!reveal.frame(true, ms(start, 50)));
    }

    #[test]
    fn a_window_moved_at_the_start_of_a_frame_waits_for_two_more() {
        let start = Instant::now();
        let mut reveal = Reveal::waiting(start);
        reveal.found(start);
        assert!(
            !reveal.frame(false, ms(start, 10)),
            "moved: winit resized it"
        );
        assert!(!reveal.frame(true, ms(start, 26)));
        assert!(
            !reveal.frame(false, ms(start, 42)),
            "its pictures changed its size"
        );
        assert!(!reveal.frame(true, ms(start, 58)));
        assert!(reveal.frame(true, ms(start, 74)));
    }

    #[test]
    fn a_strip_that_takes_too_long_is_shown_anyway() {
        let start = Instant::now();
        let mut reveal = Reveal::waiting(start);
        reveal.found(start);
        assert!(!reveal.overdue(ms(start, 100)));
        assert_eq!(
            reveal.left(ms(start, 100)),
            Some(Duration::from_millis(400))
        );
        assert!(reveal.overdue(ms(start, 500)));
        assert!(!reveal.pending());
        assert_eq!(reveal.left(ms(start, 600)), None);

        let mut moving = Reveal::waiting(start);
        moving.found(start);
        assert!(moving.frame(false, ms(start, 600)), "never settles");

        let mut never_found = Reveal::waiting(start);
        assert!(never_found.overdue(ms(start, 700)));
        assert!(!never_found.found(ms(start, 701)), "given up");
    }
}
