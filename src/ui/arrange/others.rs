//! The other desktops in the Arrange strip: hidden by default, shown by the
//! link under this desktop's windows, the D key, a window moved to one of
//! them, or a card held over the link.

use std::time::{Duration, Instant};

use crate::core::ui_bridge::{ArrangeDesktop, ArrangeGroup};

/// How long a dragged card must rest on the link before the other desktops
/// open, so a quick pass over it does not.
pub const HOVER_DELAY: Duration = Duration::from_millis(350);

/// The link's line, with a small gap above it.
pub const LINK_HEIGHT: f32 = 24.0;

/// Whether the other desktops are open. A new strip always starts closed.
#[derive(Debug, Default)]
pub struct Others {
    open: bool,
    over_since: Option<Instant>,
}

impl Others {
    /// The other desktops are on screen: asked for, and there is one with
    /// windows of this app.
    pub fn shown(&self, count: usize) -> bool {
        self.open && count > 0
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.over_since = None;
    }

    pub fn expand(&mut self) {
        self.open = true;
        self.over_since = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Tells that a dragged card is, or is not, over the link at `now`.
    /// Opens the desktops once it has rested there for `HOVER_DELAY`;
    /// until then returns how much longer to wait.
    pub fn drag_over(&mut self, now: Instant, over: bool) -> Option<Duration> {
        if !over || self.open {
            self.over_since = None;
            return None;
        }
        let since = *self.over_since.get_or_insert(now);
        let waited = now.saturating_duration_since(since);
        if waited >= HOVER_DELAY {
            self.expand();
            None
        } else {
            Some(HOVER_DELAY - waited)
        }
    }
}

/// How many other desktops have windows of the group.
pub fn count(group: &ArrangeGroup, desktops: &[ArrangeDesktop]) -> usize {
    desktops
        .iter()
        .filter(|desktop| !desktop.current)
        .filter(|desktop| group.windows.iter().any(|w| w.desktop == desktop.id))
        .count()
}

/// The link's words; None when there is nothing to open.
pub fn label(count: usize, shown: bool) -> Option<String> {
    match (count, shown) {
        (0, _) => None,
        (_, true) => Some("Hide other desktops".to_string()),
        (1, false) => Some("+ 1 other desktop".to_string()),
        (n, false) => Some(format!("+ {n} other desktops")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::ArrangeWindow;

    fn at(ms: u64, start: Instant) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn the_label_counts_the_other_desktops() {
        assert_eq!(label(0, false), None);
        assert_eq!(label(0, true), None);
        assert_eq!(label(1, false).as_deref(), Some("+ 1 other desktop"));
        assert_eq!(label(3, false).as_deref(), Some("+ 3 other desktops"));
        assert_eq!(label(3, true).as_deref(), Some("Hide other desktops"));
    }

    #[test]
    fn it_starts_closed_and_d_toggles() {
        let mut others = Others::default();
        assert!(!others.shown(2));
        others.toggle();
        assert!(others.shown(2));
        others.toggle();
        assert!(!others.shown(2));
    }

    #[test]
    fn with_no_other_desktop_with_windows_nothing_is_shown() {
        let mut others = Others::default();
        others.expand();
        assert!(!others.shown(0));
    }

    #[test]
    fn a_move_to_a_hidden_desktop_opens_it_until_toggled_or_reopened() {
        let mut others = Others::default();
        others.expand();
        assert!(others.shown(1));
        others.reset();
        assert!(!others.shown(1));
    }

    #[test]
    fn a_card_opens_the_desktops_only_after_resting_on_the_link() {
        let start = Instant::now();
        let mut others = Others::default();
        assert!(others.drag_over(at(0, start), true).is_some());
        assert!(others.drag_over(at(200, start), true).is_some());
        assert!(!others.shown(1));
        assert_eq!(others.drag_over(at(350, start), true), None);
        assert!(others.shown(1));
    }

    #[test]
    fn a_quick_pass_over_the_link_starts_the_wait_again() {
        let start = Instant::now();
        let mut others = Others::default();
        others.drag_over(at(0, start), true);
        others.drag_over(at(200, start), false);
        assert!(others.drag_over(at(400, start), true).is_some());
        assert!(!others.shown(1));
    }

    #[test]
    fn counting_skips_this_desktop_and_desktops_without_windows() {
        let desktop = |id: &str, current: bool| ArrangeDesktop {
            id: id.to_string(),
            name: id.to_string(),
            current,
        };
        let window = |desktop: &str| ArrangeWindow {
            hwnd: 0,
            label: String::new(),
            desktop: desktop.to_string(),
            monitor: None,
            on_monitor: None,
            saved_monitor: None,
            renamed: false,
            same_title: false,
        };
        let group = ArrangeGroup {
            key: String::new(),
            label: String::new(),
            windows: vec![window("a"), window("b"), window("b"), window("d")],
        };
        let desktops = [
            desktop("a", true),
            desktop("b", false),
            desktop("c", false),
            desktop("d", false),
        ];
        assert_eq!(count(&group, &desktops), 2);
    }
}
