//! When saved names are matched again. Matching reads every app window,
//! which asks the window manager and Explorer about each one and costs
//! 5–10 ms, more while they are busy. So it runs only when something it
//! looks at changed: a window came, went, was shown or hidden, a window of a
//! group that waits for a name changed its title, or the waiting names
//! changed. A cheap fingerprint of those facts tells. For what the
//! fingerprint does not see (a window that only moved to another desktop),
//! the names are matched anyway every `FULL_EVERY` looks.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};

/// Looks between two matches made without a change.
pub const FULL_EVERY: u64 = 15;

/// A fingerprint of the visible top-level windows. Their order does not
/// count, so switching between windows changes nothing. The title counts only for a window whose group is in `waiting`: titles of
/// other apps change all the time and cannot give them a name. A window not
/// seen at the last match has no group yet; its arrival alone changes the
/// fingerprint.
pub fn fingerprint(
    visible: &[isize],
    group_of: &HashMap<isize, (u32, String)>,
    waiting: &BTreeSet<&str>,
    title_of: &dyn Fn(isize) -> String,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    waiting.hash(&mut hasher);
    let mut visible = visible.to_vec();
    visible.sort_unstable();
    for hwnd in &visible {
        hwnd.hash(&mut hasher);
        let waits = group_of
            .get(hwnd)
            .is_some_and(|(_, group)| waiting.contains(group.as_str()));
        if waits {
            title_of(*hwnd).hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Remembers the fingerprint at the last match.
#[derive(Debug, Default)]
pub struct Lookout {
    seen: Option<u64>,
    /// Looks since the last match.
    quiet: u64,
}

impl Lookout {
    /// Whether to match now, given the fingerprint of this look.
    pub fn due(&mut self, fingerprint: u64) -> bool {
        self.quiet += 1;
        if self.seen == Some(fingerprint) && self.quiet < FULL_EVERY {
            return false;
        }
        self.seen = Some(fingerprint);
        self.quiet = 0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> HashMap<isize, (u32, String)> {
        HashMap::from([
            (1, (10, "chrome".to_string())),
            (2, (20, "terminal".to_string())),
        ])
    }

    #[test]
    fn only_titles_of_waiting_groups_count() {
        let groups = groups();
        let waiting = BTreeSet::from(["chrome"]);
        let print = |terminal: &str, chrome: &str| {
            let (terminal, chrome) = (terminal.to_string(), chrome.to_string());
            fingerprint(&[1, 2], &groups, &waiting, &move |hwnd| {
                if hwnd == 1 {
                    chrome.clone()
                } else {
                    terminal.clone()
                }
            })
        };
        assert_eq!(print("build 1%", "Inbox"), print("build 99%", "Inbox"));
        assert_ne!(print("build 1%", "Inbox"), print("build 1%", "Mail"));
    }

    #[test]
    fn a_window_that_comes_or_goes_counts_but_the_order_does_not() {
        let groups = groups();
        let waiting = BTreeSet::new();
        let title = |_: isize| String::new();
        let print = |visible: &[isize]| fingerprint(visible, &groups, &waiting, &title);
        assert_ne!(print(&[1, 2]), print(&[1, 2, 3]), "a new window");
        assert_ne!(print(&[1, 2]), print(&[1]), "a window went");
        assert_eq!(print(&[1, 2]), print(&[2, 1]), "another window in front");
        assert_eq!(print(&[1, 2]), print(&[1, 2]));
    }

    #[test]
    fn the_waiting_names_count() {
        let groups = groups();
        let title = |_: isize| "same".to_string();
        let none = BTreeSet::new();
        let one = BTreeSet::from(["paint"]);
        assert_ne!(
            fingerprint(&[1], &groups, &none, &title),
            fingerprint(&[1], &groups, &one, &title)
        );
    }

    #[test]
    fn matching_waits_for_a_change_or_a_full_round() {
        let mut lookout = Lookout::default();
        assert!(lookout.due(7), "the first look matches");
        for _ in 1..FULL_EVERY {
            assert!(!lookout.due(7));
        }
        assert!(lookout.due(7), "a full round without a change");
        assert!(!lookout.due(7));
        assert!(lookout.due(8), "a change");
        assert!(!lookout.due(8));
    }
}
