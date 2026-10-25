//! Keeps a change the user just made on screen until the plugin's own data
//! shows the same change.
//!
//! A drop sends up to three steps to the host: move to a desktop, move to a
//! monitor, then reorder the taskbar buttons. The plugin sends fresh data
//! while those steps are still running, so a half-done order would flash by
//! before the final one. The strip holds its own result back from that data
//! until the data agrees with it, or until `HOLD` has passed.

use std::time::{Duration, Instant};

use crate::core::ui_bridge::{ArrangeAction, ArrangeGroup, ArrangeWindow};

/// How long the plugin's data may disagree with a local change. A reorder
/// rebuilds the taskbar buttons, which can take a good part of a second;
/// after this long the plugin's data wins even if it still disagrees (for
/// example when the move failed).
pub const HOLD: Duration = Duration::from_secs(2);

/// What one group should look like once the host has done the steps.
pub struct Hold {
    group: String,
    until: Instant,
    /// The whole group's new order, when the change reordered it.
    order: Option<Vec<isize>>,
    desktops: Vec<(isize, String)>,
    monitors: Vec<(isize, Option<usize>)>,
}

impl Hold {
    /// The hold for `actions` that were just applied to `local`, or None
    /// when none of them moves or reorders a window.
    pub fn new(
        local: &ArrangeGroup,
        actions: &[ArrangeAction],
        monitor_place: &dyn Fn(&str) -> Option<usize>,
        now: Instant,
    ) -> Option<Self> {
        let mut hold = Self {
            group: local.key.clone(),
            until: now + HOLD,
            order: None,
            desktops: Vec::new(),
            monitors: Vec::new(),
        };
        for action in actions {
            match action {
                ArrangeAction::MoveToDesktop { hwnd, desktop } => {
                    hold.desktops.push((*hwnd, desktop.clone()));
                }
                ArrangeAction::MoveToMonitor { hwnd, monitor } => {
                    hold.monitors.push((*hwnd, monitor_place(monitor)));
                }
                ArrangeAction::Reorder { .. } => {
                    hold.order = Some(local.windows.iter().map(|w| w.hwnd).collect());
                }
                _ => {}
            }
        }
        (hold.order.is_some() || !hold.desktops.is_empty() || !hold.monitors.is_empty())
            .then_some(hold)
    }

    pub fn group(&self) -> &str {
        &self.group
    }

    pub fn expired(&self, now: Instant) -> bool {
        now >= self.until
    }

    /// Whether `incoming` already shows the change. A window that is not in
    /// `incoming` (it closed) is left out of the comparison.
    pub fn agrees(&self, incoming: &ArrangeGroup) -> bool {
        let window = |hwnd: isize| incoming.windows.iter().find(|w| w.hwnd == hwnd);
        if let Some(order) = &self.order {
            let wanted: Vec<isize> = order
                .iter()
                .copied()
                .filter(|hwnd| window(*hwnd).is_some())
                .collect();
            let shown: Vec<isize> = incoming
                .windows
                .iter()
                .map(|w| w.hwnd)
                .filter(|hwnd| order.contains(hwnd))
                .collect();
            if wanted != shown {
                return false;
            }
        }
        self.desktops
            .iter()
            .all(|(hwnd, desktop)| window(*hwnd).is_none_or(|w| &w.desktop == desktop))
            && self
                .monitors
                .iter()
                .all(|(hwnd, place)| window(*hwnd).is_none_or(|w| w.on_monitor == *place))
    }

    /// `incoming` with the change laid over it: the held order, desktops and
    /// monitors, and everything else (titles, windows that opened or closed)
    /// as the plugin sent it. A window that is new goes last.
    pub fn overlay(&self, mut incoming: ArrangeGroup) -> ArrangeGroup {
        if let Some(order) = &self.order {
            incoming.windows.sort_by_key(|window| {
                order
                    .iter()
                    .position(|hwnd| *hwnd == window.hwnd)
                    .unwrap_or(usize::MAX)
            });
        }
        let find = |windows: &mut [ArrangeWindow], hwnd: isize| -> Option<usize> {
            windows.iter().position(|w| w.hwnd == hwnd)
        };
        for (hwnd, desktop) in &self.desktops {
            if let Some(at) = find(&mut incoming.windows, *hwnd) {
                incoming.windows[at].desktop = desktop.clone();
            }
        }
        for (hwnd, place) in &self.monitors {
            if let Some(at) = find(&mut incoming.windows, *hwnd) {
                incoming.windows[at].on_monitor = *place;
                incoming.windows[at].saved_monitor = *place;
            }
        }
        incoming
    }
}

/// Settles every hold against fresh data: a hold whose group is gone, that
/// agrees or that has run out ends, and the groups of the others come back
/// with the change laid over them.
pub fn filter(holds: &mut Vec<Hold>, groups: &mut [ArrangeGroup], now: Instant) {
    holds.retain(|hold| {
        let Some(group) = groups.iter_mut().find(|g| g.key == hold.group) else {
            return false;
        };
        if hold.expired(now) || hold.agrees(group) {
            return false;
        }
        let incoming = ArrangeGroup {
            key: group.key.clone(),
            label: group.label.clone(),
            windows: std::mem::take(&mut group.windows),
        };
        *group = hold.overlay(incoming);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(hwnd: isize, desktop: &str, on_monitor: Option<usize>) -> ArrangeWindow {
        ArrangeWindow {
            hwnd,
            label: format!("w{hwnd}"),
            desktop: desktop.to_string(),
            monitor: None,
            on_monitor,
            saved_monitor: on_monitor,
            renamed: false,
            same_title: false,
            attention: false,
        }
    }

    fn group(key: &str, windows: Vec<ArrangeWindow>) -> ArrangeGroup {
        ArrangeGroup {
            key: key.to_string(),
            label: key.to_string(),
            windows,
        }
    }

    fn order(group: &ArrangeGroup) -> Vec<isize> {
        group.windows.iter().map(|w| w.hwnd).collect()
    }

    fn place(device: &str) -> Option<usize> {
        (device == "B").then_some(1)
    }

    /// Window 1 dragged to desktop "d2", monitor B, and placed first.
    fn drop_of_one() -> (Hold, Instant) {
        let now = Instant::now();
        let local = group(
            "a",
            vec![
                window(1, "d2", Some(1)),
                window(2, "d1", Some(0)),
                window(3, "d1", Some(1)),
            ],
        );
        let actions = [
            ArrangeAction::MoveToDesktop {
                hwnd: 1,
                desktop: "d2".into(),
            },
            ArrangeAction::MoveToMonitor {
                hwnd: 1,
                monitor: "B".into(),
            },
            ArrangeAction::Reorder {
                group: "a".into(),
                order: vec![1, 2, 3],
            },
        ];
        (Hold::new(&local, &actions, &place, now).unwrap(), now)
    }

    #[test]
    fn actions_that_move_nothing_make_no_hold() {
        let local = group("a", vec![window(1, "d1", None)]);
        let actions = [ArrangeAction::Close(1), ArrangeAction::AskName(1)];
        assert!(Hold::new(&local, &actions, &place, Instant::now()).is_none());
    }

    #[test]
    fn an_intermediate_snapshot_is_held_back() {
        let (hold, now) = drop_of_one();
        // The move is done but the taskbar order is still the old one.
        let mut groups = vec![group(
            "a",
            vec![
                window(2, "d1", Some(0)),
                window(1, "d2", Some(1)),
                window(3, "d1", Some(1)),
            ],
        )];
        let mut holds = vec![hold];
        filter(&mut holds, &mut groups, now);
        assert_eq!(holds.len(), 1);
        assert_eq!(order(&groups[0]), [1, 2, 3]);
    }

    #[test]
    fn a_snapshot_with_the_old_monitor_is_held_back() {
        let (hold, now) = drop_of_one();
        let mut groups = vec![group(
            "a",
            vec![
                window(1, "d2", Some(0)),
                window(2, "d1", Some(0)),
                window(3, "d1", Some(1)),
            ],
        )];
        let mut holds = vec![hold];
        filter(&mut holds, &mut groups, now);
        assert_eq!(holds.len(), 1);
        assert_eq!(groups[0].windows[0].on_monitor, Some(1));
    }

    #[test]
    fn an_agreeing_snapshot_is_taken_and_ends_the_hold() {
        let (hold, now) = drop_of_one();
        let mut groups = vec![group(
            "a",
            vec![
                window(1, "d2", Some(1)),
                window(2, "d1", Some(0)),
                window(3, "d1", Some(1)),
            ],
        )];
        let sent = groups.clone();
        let mut holds = vec![hold];
        filter(&mut holds, &mut groups, now);
        assert!(holds.is_empty());
        assert_eq!(groups, sent);
    }

    #[test]
    fn the_plugin_wins_after_the_timeout() {
        let (hold, now) = drop_of_one();
        let sent = vec![group(
            "a",
            vec![
                window(2, "d1", Some(0)),
                window(1, "d1", Some(0)),
                window(3, "d1", Some(1)),
            ],
        )];
        let mut groups = sent.clone();
        let mut holds = vec![hold];
        filter(
            &mut holds,
            &mut groups,
            now + HOLD - Duration::from_millis(1),
        );
        assert_eq!(holds.len(), 1);
        groups = sent.clone();
        filter(&mut holds, &mut groups, now + HOLD);
        assert!(holds.is_empty());
        assert_eq!(groups, sent);
    }

    #[test]
    fn a_window_that_closes_during_the_hold_disappears() {
        let (hold, now) = drop_of_one();
        let mut groups = vec![group(
            "a",
            vec![window(2, "d1", Some(0)), window(1, "d2", Some(1))],
        )];
        let mut holds = vec![hold];
        filter(&mut holds, &mut groups, now);
        // Window 3 is gone from the data, so it is gone from the screen too.
        assert_eq!(order(&groups[0]), [1, 2]);
        assert_eq!(holds.len(), 1);
        // Data that now agrees, minus the closed window, ends the hold.
        let mut groups = vec![group(
            "a",
            vec![window(1, "d2", Some(1)), window(2, "d1", Some(0))],
        )];
        filter(&mut holds, &mut groups, now);
        assert!(holds.is_empty());
    }

    #[test]
    fn the_moved_window_closing_ends_the_hold() {
        let (hold, now) = drop_of_one();
        let mut groups = vec![group(
            "a",
            vec![window(2, "d1", Some(0)), window(3, "d1", Some(1))],
        )];
        let mut holds = vec![hold];
        filter(&mut holds, &mut groups, now);
        assert!(holds.is_empty());
        assert_eq!(order(&groups[0]), [2, 3]);
    }

    #[test]
    fn new_windows_and_fresh_titles_come_through_while_held() {
        let (hold, now) = drop_of_one();
        let mut renamed = window(3, "d1", Some(1));
        renamed.label = "fresh title".into();
        let mut groups = vec![group(
            "a",
            vec![
                window(2, "d1", Some(0)),
                window(1, "d2", Some(1)),
                window(9, "d1", None),
                renamed,
            ],
        )];
        let mut holds = vec![hold];
        filter(&mut holds, &mut groups, now);
        assert_eq!(holds.len(), 1);
        assert_eq!(order(&groups[0]), [1, 2, 3, 9]);
        assert_eq!(groups[0].windows[2].label, "fresh title");
    }

    #[test]
    fn other_groups_are_untouched_and_a_missing_group_drops_the_hold() {
        let (hold, now) = drop_of_one();
        let other = group("b", vec![window(7, "d1", None), window(5, "d1", None)]);
        let mut groups = vec![
            other.clone(),
            group(
                "a",
                vec![window(2, "d1", Some(0)), window(1, "d2", Some(1))],
            ),
        ];
        let mut holds = vec![hold];
        filter(&mut holds, &mut groups, now);
        assert_eq!(groups[0], other);
        assert_eq!(holds[0].group(), "a");
        let mut groups = vec![other];
        filter(&mut holds, &mut groups, now);
        assert!(holds.is_empty());
    }
}
