//! When Win+number may count the taskbar buttons read earlier instead of
//! asking UI Automation again, which took 110-150 ms of the key press.
//!
//! Once Win+number has been used, the list is kept: a window that came or
//! went, or a restarted Explorer, marks it stale, and the host reads it
//! again on a thread of its own once things settle. A press that counted
//! the kept list is checked against the next read, which also catches a
//! button the user dragged to another place: if the number now names
//! another app, WindowDeck is opened again on the right one.

use std::time::{Duration, Instant};

use crate::core::taskbar::Button;
use crate::core::taskbar_number;

/// Before Win+number is first used, a list this old still counts for a
/// digit typed after Win+Alt+A.
pub const FRESH: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct Cache {
    /// Win+number has been used since start, so the list is kept fresh.
    watched: bool,
    /// The list may no longer match the taskbar.
    stale: bool,
    /// The taskbar number a press counted on the kept list, and its app.
    check: Option<(usize, String)>,
}

impl Cache {
    /// Whether a list read at `read_at` may be counted now.
    pub fn usable(&self, read_at: Option<Instant>, now: Instant) -> bool {
        read_at.is_some_and(|at| {
            !self.stale && (self.watched || now.saturating_duration_since(at) < FRESH)
        })
    }

    /// Win and a digit came: from now on the list is kept fresh.
    pub fn press(&mut self) {
        self.watched = true;
    }

    /// A press counted the kept list: `number` was taken to be `app`.
    pub fn counted(&mut self, number: usize, app: &str) {
        self.check = Some((number, app.to_string()));
    }

    /// A window came or went, or Explorer started again. True when the
    /// list is to be read again (only once Win+number has been used).
    pub fn changed(&mut self) -> bool {
        self.stale = true;
        self.watched
    }

    /// A list was just read. The taskbar number to open WindowDeck on again,
    /// when the last press counted a list on which it named another app.
    pub fn read(&mut self, buttons: &[Button]) -> Option<usize> {
        self.stale = false;
        let (number, app) = self.check.take()?;
        let now = taskbar_number::button_number(buttons, number).map(|(button, _)| &button.app);
        match now {
            Some(now) if *now == app => None,
            _ => Some(number),
        }
    }
}

/// A shell message after which the taskbar's buttons may have changed:
/// a window came or went. A window that only took the front changes no
/// button, and it comes far too often to read the taskbar each time.
pub fn buttons_may_change(code: usize) -> bool {
    const CREATED: usize = 1;
    const DESTROYED: usize = 2;
    const REPLACED: usize = 13;
    matches!(code, CREATED | DESTROYED | REPLACED)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(app: &str, left: i32) -> Button {
        Button {
            app: app.to_string(),
            rect: [left, 1040, left + 48, 1088],
        }
    }

    #[test]
    fn before_win_number_is_used_the_list_counts_for_ten_seconds() {
        let cache = Cache::default();
        let now = Instant::now();
        assert!(!cache.usable(None, now), "never read");
        assert!(cache.usable(Some(now), now));
        assert!(!cache.usable(Some(now - FRESH), now));
    }

    #[test]
    fn once_used_the_list_counts_until_a_window_comes_or_goes() {
        let mut cache = Cache::default();
        let now = Instant::now();
        let long_ago = Some(now - FRESH * 10);
        assert!(
            !cache.changed(),
            "nothing is read before Win+number is used"
        );
        cache.read(&[]);
        cache.press();
        assert!(cache.usable(long_ago, now), "kept, however old");
        assert!(cache.changed(), "read again");
        assert!(!cache.usable(long_ago, now), "stale until then");
        assert!(!cache.usable(Some(now), now));
        assert_eq!(cache.read(&[]), None);
        assert!(cache.usable(long_ago, now));
    }

    #[test]
    fn a_press_on_a_list_that_was_wrong_is_opened_again() {
        let mut cache = Cache::default();
        cache.press();
        let list = [button("Terminal", 0), button("Chrome", 48)];
        cache.counted(2, "Chrome");
        assert_eq!(cache.read(&list), None, "right");
        assert_eq!(cache.read(&list), None, "checked once only");
        // The user dragged Chrome to the first place.
        cache.counted(2, "Chrome");
        let moved = [button("Chrome", 0), button("Terminal", 48)];
        assert_eq!(cache.read(&moved), Some(2));
        // The button is gone.
        cache.counted(2, "Chrome");
        assert_eq!(cache.read(&list[..1]), Some(2));
    }

    #[test]
    fn only_a_window_that_comes_or_goes_changes_the_buttons() {
        for code in [1, 2, 13] {
            assert!(buttons_may_change(code), "{code}");
        }
        for code in [4, 0x8004, 6, 0x8006, 16] {
            assert!(!buttons_may_change(code), "{code}");
        }
    }
}
