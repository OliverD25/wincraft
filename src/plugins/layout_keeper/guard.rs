//! Keeps a browser crash or restart during the day from overwriting the
//! saved layout. After a restart the windows come back in a scrambled order,
//! and the next timer save would make that scrambled order the one restored
//! after a reboot.
//!
//! A window's identity here is its handle: titles change with every tab
//! switch, but a handle only changes when the window is closed and opened
//! again, which is exactly what a restart does to every window at once.

use std::collections::{BTreeMap, BTreeSet};

use super::order::Handle;

/// Why a group's timer saves are held.
#[derive(Clone, Debug, PartialEq)]
pub enum Why {
    /// The window count fell by more than half, from `before` to `now`.
    Dropped { before: usize, now: usize },
    /// `replaced` of the `before` windows were swapped for new ones between
    /// two looks.
    Replaced { replaced: usize, before: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Held {
        group: String,
        why: Why,
    },
    /// The windows came back after a drop, so it was a restart; the hold now
    /// waits for the count to stay put.
    Refilled {
        group: String,
        now: usize,
    },
    /// The count stayed the same long enough; timer saves resume.
    Released {
        group: String,
        stable_seconds: u64,
    },
    /// The windows did not come back in time, so they were closed on
    /// purpose; timer saves resume.
    Closed {
        group: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// How long the count must stay the same before a hold ends.
    pub stable_seconds: u64,
    /// How long dropped windows have to come back to count as a restart.
    pub refill_seconds: u64,
}

#[derive(Clone, Debug)]
enum Hold {
    /// Waiting until `deadline` for the windows to come back.
    Dropped { before: usize, deadline: u64 },
    /// The group went through a restart; waiting for it to settle.
    Settling,
}

#[derive(Clone, Debug)]
struct Watch {
    handles: Vec<Handle>,
    /// When the window count last changed.
    changed_at: u64,
    hold: Option<Hold>,
}

#[derive(Default)]
pub struct Guard {
    watches: BTreeMap<String, Watch>,
    /// Groups that were held and whose new windows have not been written
    /// yet: the write that first includes them keeps the old file first.
    backup_due: BTreeSet<String>,
}

impl Guard {
    /// One look at every group's windows, `now` in seconds. Returns what
    /// changed, for the log.
    pub fn observe(
        &mut self,
        now: u64,
        live: &BTreeMap<String, Vec<Handle>>,
        timing: Timing,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        let keys: BTreeSet<String> = self.watches.keys().chain(live.keys()).cloned().collect();
        for key in keys {
            let handles = live.get(&key).cloned().unwrap_or_default();
            let Some(watch) = self.watches.get_mut(&key) else {
                self.watches.insert(
                    key,
                    Watch {
                        handles,
                        changed_at: now,
                        hold: None,
                    },
                );
                continue;
            };
            let before = watch.handles.len();
            let count = handles.len();
            if count != before {
                watch.changed_at = now;
            }
            match watch.hold {
                None => {
                    let gone = watch
                        .handles
                        .iter()
                        .filter(|handle| !handles.contains(handle))
                        .count();
                    let new = handles
                        .iter()
                        .filter(|handle| !watch.handles.contains(handle))
                        .count();
                    let replaced = gone.min(new);
                    let why = if count * 2 < before {
                        watch.hold = Some(Hold::Dropped {
                            before,
                            deadline: now + timing.refill_seconds,
                        });
                        Some(Why::Dropped { before, now: count })
                    } else if before > 0 && replaced * 3 > before {
                        watch.hold = Some(Hold::Settling);
                        Some(Why::Replaced { replaced, before })
                    } else {
                        None
                    };
                    if let Some(why) = why {
                        // Steady means steady since the burst: after a fast
                        // restart the count never changed at all.
                        watch.changed_at = now;
                        self.backup_due.insert(key.clone());
                        events.push(Event::Held {
                            group: key.clone(),
                            why,
                        });
                    }
                }
                Some(Hold::Dropped { before, deadline }) => {
                    if count * 2 > before {
                        watch.hold = Some(Hold::Settling);
                        events.push(Event::Refilled {
                            group: key.clone(),
                            now: count,
                        });
                    } else if now >= deadline {
                        watch.hold = None;
                        events.push(Event::Closed { group: key.clone() });
                    }
                }
                Some(Hold::Settling) => {
                    let stable = now.saturating_sub(watch.changed_at);
                    if stable >= timing.stable_seconds {
                        watch.hold = None;
                        events.push(Event::Released {
                            group: key.clone(),
                            stable_seconds: stable,
                        });
                    }
                }
            }
            watch.handles = handles;
        }
        self.watches
            .retain(|_, watch| !watch.handles.is_empty() || watch.hold.is_some());
        events
    }

    /// The groups whose timer saves keep the windows saved before.
    pub fn held(&self) -> BTreeSet<String> {
        self.watches
            .iter()
            .filter(|(_, watch)| watch.hold.is_some())
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Whether the next write puts a held group's new windows into the file:
    /// any write that is not a timer save, or a timer save once the hold is
    /// over.
    pub fn backup_needed(&self, timer: bool) -> bool {
        let held = self.held();
        self.backup_due
            .iter()
            .any(|group| !timer || !held.contains(group))
    }

    /// A write that `backup_needed` asked a copy for has gone through, or
    /// found the layout unchanged, which leaves nothing to protect.
    pub fn wrote(&mut self, timer: bool) {
        let held = self.held();
        self.backup_due
            .retain(|group| timer && held.contains(group));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMING: Timing = Timing {
        stable_seconds: 30,
        refill_seconds: 180,
    };

    fn look(groups: &[(&str, &[Handle])]) -> BTreeMap<String, Vec<Handle>> {
        groups
            .iter()
            .map(|(key, handles)| (key.to_string(), handles.to_vec()))
            .collect()
    }

    fn held(guard: &Guard) -> Vec<String> {
        guard.held().into_iter().collect()
    }

    #[test]
    fn a_crash_and_restart_holds_until_the_count_settles() {
        let mut guard = Guard::default();
        guard.observe(0, &look(&[("Chrome", &[1, 2, 3, 4, 5, 6])]), TIMING);

        let events = guard.observe(30, &look(&[("Chrome", &[])]), TIMING);
        assert_eq!(
            events,
            [Event::Held {
                group: "Chrome".into(),
                why: Why::Dropped { before: 6, now: 0 }
            }]
        );
        assert_eq!(held(&guard), ["Chrome"]);

        let events = guard.observe(60, &look(&[("Chrome", &[11, 12, 13, 14])]), TIMING);
        assert_eq!(
            events,
            [Event::Refilled {
                group: "Chrome".into(),
                now: 4
            }]
        );
        // The last window arrives; the count must then stay put for 30 s.
        assert!(guard
            .observe(90, &look(&[("Chrome", &[11, 12, 13, 14, 15, 16])]), TIMING)
            .is_empty());
        assert_eq!(held(&guard), ["Chrome"]);
        let events = guard.observe(120, &look(&[("Chrome", &[11, 12, 13, 14, 15, 16])]), TIMING);
        assert_eq!(
            events,
            [Event::Released {
                group: "Chrome".into(),
                stable_seconds: 30
            }]
        );
        assert!(held(&guard).is_empty());
    }

    #[test]
    fn windows_closed_on_purpose_release_the_hold_after_the_refill_time() {
        let mut guard = Guard::default();
        guard.observe(0, &look(&[("Chrome", &[1, 2, 3, 4])]), TIMING);
        guard.observe(30, &look(&[("Chrome", &[1])]), TIMING);
        assert_eq!(held(&guard), ["Chrome"]);
        assert!(guard
            .observe(120, &look(&[("Chrome", &[1])]), TIMING)
            .is_empty());
        let events = guard.observe(210, &look(&[("Chrome", &[1])]), TIMING);
        assert_eq!(
            events,
            [Event::Closed {
                group: "Chrome".into()
            }]
        );
        assert!(held(&guard).is_empty());
    }

    #[test]
    fn a_fast_restart_shows_as_replaced_windows() {
        let mut guard = Guard::default();
        guard.observe(0, &look(&[("Chrome", &[1, 2, 3, 4, 5, 6])]), TIMING);
        let events = guard.observe(30, &look(&[("Chrome", &[1, 2, 3, 24, 25, 26])]), TIMING);
        assert_eq!(
            events,
            [Event::Held {
                group: "Chrome".into(),
                why: Why::Replaced {
                    replaced: 3,
                    before: 6
                }
            }]
        );
        // The count was the same all along, yet the hold still waits the
        // full steady time from the moment of the restart.
        let same = look(&[("Chrome", &[1, 2, 3, 24, 25, 26])]);
        assert!(guard.observe(45, &same, TIMING).is_empty());
        assert_eq!(
            guard.observe(60, &same, TIMING),
            [Event::Released {
                group: "Chrome".into(),
                stable_seconds: 30
            }]
        );
    }

    #[test]
    fn ordinary_use_never_holds() {
        let mut guard = Guard::default();
        guard.observe(0, &look(&[("Chrome", &[1, 2, 3, 4, 5, 6])]), TIMING);
        // Opening and closing a window or two, and a second group appearing.
        for (now, handles) in [
            (30, &[1, 2, 3, 4, 5, 6, 7][..]),
            (60, &[1, 2, 3, 4, 5, 7][..]),
            (90, &[1, 2, 3, 4, 7, 8][..]),
            (120, &[1, 2, 3, 4][..]),
        ] {
            let events = guard.observe(
                now,
                &look(&[("Chrome", handles), ("Chrome._crx_app", &[40])]),
                TIMING,
            );
            assert!(events.is_empty(), "at {now}: {events:?}");
        }
        assert!(!guard.backup_needed(true));
        assert!(!guard.backup_needed(false));
    }

    #[test]
    fn the_first_write_with_the_new_windows_keeps_a_copy_first() {
        let mut guard = Guard::default();
        guard.observe(0, &look(&[("Chrome", &[1, 2, 3, 4])]), TIMING);
        guard.observe(30, &look(&[("Chrome", &[])]), TIMING);

        // Timer saves during the hold keep the old windows: no copy needed.
        assert!(!guard.backup_needed(true));
        // A manual or shutdown save writes the new windows: copy first.
        assert!(guard.backup_needed(false));

        guard.observe(60, &look(&[("Chrome", &[5, 6, 7, 8])]), TIMING);
        guard.observe(90, &look(&[("Chrome", &[5, 6, 7, 8])]), TIMING);
        assert!(held(&guard).is_empty());
        assert!(guard.backup_needed(true));
        guard.wrote(true);
        assert!(!guard.backup_needed(true));
        assert!(!guard.backup_needed(false));
    }

    #[test]
    fn a_write_during_the_hold_keeps_the_copy_due_for_timer_saves_only() {
        let mut guard = Guard::default();
        guard.observe(0, &look(&[("A", &[1, 2]), ("B", &[3, 4])]), TIMING);
        guard.observe(30, &look(&[("A", &[]), ("B", &[3, 4])]), TIMING);
        assert!(guard.backup_needed(false));
        guard.wrote(false);
        assert!(!guard.backup_needed(false));
        assert!(!guard.backup_needed(true));
    }
}
