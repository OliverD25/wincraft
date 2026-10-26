//! The windows WindowDeck lists itself, for when no plugin that keeps a
//! window order (LayoutKeeper) answers the strip. Every app's windows but
//! WinCraft's and Windows' helpers, one group per taskbar button, in the
//! order this session has seen them: a group seen for the first time is
//! taken bottom of the z-order first, and a new window joins at the end, the
//! way Windows adds a new button. Nothing is saved, so a new order lasts
//! until WinCraft stops, and what needs a saved layout (the "Moved" chip, a
//! new order for other desktops at the next restore) is not there.
//!
//! Only looked at when the strip asks: no timer runs for it (Principle 5).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::core::live_windows::{self, handles_in, LiveWindow};
use crate::core::taskbar_order::{self, Handle, OrderModel, Taskbar};
use crate::core::traits::WindowGroups;
use crate::core::ui_bridge::{ArrangeAction, ArrangeGroup};
use crate::core::virtual_desktops::{self as desktops, DesktopId, LazyMover, Reader};
use crate::core::{app_names, host, monitor_list, strip_groups, window_actions, window_names};

/// A window missing this long (in seconds of strip use) is forgotten, so a
/// window that comes back soon takes its old place.
const DROP_AFTER_SECONDS: u64 = 30;

/// What the strip's app list says is shown when there are no windows.
const WATCHED: &str = "any app";

/// What an action asks of the caller once it was carried out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// The strip should get fresh data a moment later, once Windows has
    /// made the change.
    Refresh,
}

pub struct OwnWindows {
    groups: BTreeMap<String, OrderModel>,
    labels: BTreeMap<String, String>,
    /// Groups labelled before the apps index was ready; asked again later.
    unresolved: BTreeSet<String>,
    reader: Option<Reader>,
    reader_tried: bool,
    mover: LazyMover,
    taskbar: Option<Taskbar>,
    started: Instant,
    /// A test instance never changes a real window (Principle 1).
    read_only: bool,
    /// Requests a read-only instance turned down.
    pub refused: u32,
}

impl OwnWindows {
    pub fn new(read_only: bool) -> Self {
        Self {
            groups: BTreeMap::new(),
            labels: BTreeMap::new(),
            unresolved: BTreeSet::new(),
            reader: None,
            reader_tried: false,
            mover: LazyMover::Untried,
            taskbar: None,
            started: Instant::now(),
            read_only,
            refused: 0,
        }
    }

    /// Forgets everything, for a plugin switched off.
    pub fn clear(&mut self) {
        *self = Self::new(self.read_only);
    }

    fn ensure_reader(&mut self) {
        if self.reader_tried {
            return;
        }
        self.reader_tried = true;
        match Reader::new() {
            Ok(reader) => self.reader = Some(reader),
            Err(err) => log::warn!("WindowDeck cannot tell the desktops apart: {err}"),
        }
    }

    /// Looks at the windows open now and brings every group up to date.
    fn refresh(&mut self) -> Vec<LiveWindow> {
        self.ensure_reader();
        let live = live_windows::on_desktops(&live_windows::is_app, self.reader.as_ref());
        let tick = self.started.elapsed().as_secs();
        self.track(&live, tick);
        live
    }

    /// Brings every group's order up to date with these windows.
    fn track(&mut self, live: &[LiveWindow], tick: u64) {
        let mut keys: Vec<String> = Vec::new();
        for window in live {
            if !keys.contains(&window.group) {
                keys.push(window.group.clone());
            }
            self.remember_label(window);
        }
        for key in self.groups.keys() {
            if !keys.contains(key) {
                keys.push(key.clone());
            }
        }
        for key in keys {
            let mine = handles_in(live, &key);
            self.groups
                .entry(key)
                .or_default()
                .refresh(&mine, tick, DROP_AFTER_SECONDS);
        }
        self.groups.retain(|_, group| !group.is_empty());
    }

    fn remember_label(&mut self, window: &LiveWindow) {
        if self.labels.contains_key(&window.group) && !self.unresolved.contains(&window.group) {
            return;
        }
        let (label, resolved) = app_names::window_label(window);
        if resolved {
            self.unresolved.remove(&window.group);
        } else {
            self.unresolved.insert(window.group.clone());
        }
        self.labels.insert(window.group.clone(), label);
    }

    fn label_of(&self, key: &str) -> String {
        self.labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| app_names::friendly_key(key))
    }

    /// The groups for the strip, from the windows open now.
    pub fn window_groups(&mut self) -> WindowGroups {
        self.refresh();
        let registry = desktops::list();
        let current = desktops::current();
        let monitors = monitor_list::list();
        let reader = self.reader.as_ref();
        let desktop_of = |hwnd: Handle| strip_groups::desktop_of(reader, hwnd);
        let place = strip_groups::place_on(&monitors);
        let front = unsafe { GetForegroundWindow() };
        let (_, front_key) = live_windows::describe(front);
        let groups = self.groups_with(&desktop_of, &place);
        strip_groups::window_groups(
            groups,
            &front_key,
            strip_groups::desktops(&registry, current),
            WATCHED.to_string(),
            monitor_list::for_strip(&monitors, &[]),
        )
    }

    /// Every group with a window, in taskbar order, unsorted.
    fn groups_with(
        &self,
        desktop_of: &dyn Fn(Handle) -> String,
        place: &strip_groups::Place<'_>,
    ) -> Vec<ArrangeGroup> {
        self.groups
            .iter()
            .filter_map(|(key, model)| {
                strip_groups::group(
                    key,
                    self.label_of(key),
                    model.windows(),
                    desktop_of,
                    place,
                    &|_| None,
                )
            })
            .collect()
    }

    /// True, after logging it, when this is a read-only test instance; the
    /// caller then changes nothing.
    fn refuses(&mut self, what: &str) -> bool {
        if !self.read_only {
            return false;
        }
        self.refused += 1;
        log::info!("read-only test instance: {what} skipped");
        true
    }

    /// Does what the strip asked for one window.
    pub fn act(&mut self, action: &ArrangeAction) -> After {
        let what = match action {
            ArrangeAction::Activate(_) => "switching to a window",
            ArrangeAction::Reorder { .. } => "reordering from the strip",
            ArrangeAction::MoveToDesktop { .. } => "moving a window to another desktop",
            ArrangeAction::MoveToMonitor { .. } => "moving a window to another monitor",
            ArrangeAction::Close(_) => "closing a window",
            // The host sends names to WindowNamer; they never come here.
            ArrangeAction::Rename { .. } | ArrangeAction::AskName(_) => return After::Nothing,
        };
        if self.refuses(what) {
            return After::Nothing;
        }
        match action {
            ArrangeAction::Activate(hwnd) => {
                window_actions::activate(*hwnd as HWND);
                After::Nothing
            }
            ArrangeAction::Reorder { group, order } => {
                self.refresh();
                let elsewhere = |hwnd: Handle| live_windows::on_other_desktop(hwnd as HWND);
                if let Some(send) = self.reorder(group, order, &elsewhere) {
                    self.send_order(group, &send);
                }
                After::Refresh
            }
            ArrangeAction::MoveToDesktop { hwnd, desktop } => {
                self.move_to_desktop(*hwnd, desktop);
                After::Refresh
            }
            ArrangeAction::MoveToMonitor { hwnd, monitor } => {
                let rect = self
                    .refresh()
                    .iter()
                    .find(|window| window.hwnd as Handle == *hwnd)
                    .map(|window| window.identity.rect);
                window_actions::move_to_monitor(*hwnd as HWND, monitor, rect);
                After::Refresh
            }
            ArrangeAction::Close(hwnd) => {
                window_actions::close(*hwnd as HWND);
                After::Refresh
            }
            ArrangeAction::Rename { .. } | ArrangeAction::AskName(_) => After::Nothing,
        }
    }

    /// Puts a group in the strip's order and returns the buttons to send to
    /// the taskbar: only this desktop's, since re-adding a button pulls its
    /// window over from another desktop (`elsewhere`). None when the taskbar
    /// has nothing to change or the group is not known.
    fn reorder(
        &mut self,
        group: &str,
        order: &[Handle],
        elsewhere: &dyn Fn(Handle) -> bool,
    ) -> Option<Vec<Handle>> {
        let model = self.groups.get_mut(group)?;
        model.set_order(order);
        let pending = model.pending()?;
        let send: Vec<Handle> = pending
            .iter()
            .copied()
            .filter(|hwnd| !elsewhere(*hwnd))
            .collect();
        if send.is_empty() {
            model.mark_applied(pending);
            return None;
        }
        Some(send)
    }

    fn send_order(&mut self, group: &str, send: &[Handle]) {
        let Some(took) = taskbar_order::apply_logged(&mut self.taskbar, send) else {
            return;
        };
        log::info!(
            "applied the order of {} {} windows in {} ms",
            send.len(),
            self.label_of(group),
            took.as_millis()
        );
        if let Some(model) = self.groups.get_mut(group) {
            if let Some(pending) = model.pending() {
                model.mark_applied(pending);
            }
        }
    }

    fn move_to_desktop(&mut self, hwnd: Handle, desktop: &str) {
        let Some(target) = DesktopId::parse(desktop) else {
            log::info!("the strip named desktop \"{desktop}\", which cannot be read");
            return;
        };
        let registry = desktops::list();
        if !self.mover.ensure(&registry) {
            host::notify(
                "WindowDeck",
                &format!(
                    "Windows cannot be moved to another desktop: {}",
                    self.mover.why_not()
                ),
            );
            return;
        }
        self.ensure_reader();
        let LazyMover::Ready(mover) = &self.mover else {
            return;
        };
        let label = window_names::display_title(hwnd as HWND);
        window_actions::move_to_desktop(
            mover,
            self.reader.as_ref(),
            hwnd as HWND,
            target,
            &registry,
            &label,
        );
        // The taskbar order of the group is sent again at the next reorder.
        for model in self.groups.values_mut() {
            if model.position_of(hwnd).is_some() {
                model.invalidate();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::window_identity::WindowIdentity;

    fn live_window(hwnd: isize, title: &str, group: &str, left: i32) -> LiveWindow {
        LiveWindow {
            hwnd: hwnd as HWND,
            identity: WindowIdentity::new("app.exe", title, [left, 0, left + 800, 600], false)
                .in_group(group),
            group: group.to_string(),
            app_name: Some(group.to_string()),
            exe_path: format!("c:\\apps\\{group}.exe"),
        }
    }

    /// Made-up handles that no real window has.
    fn sample() -> Vec<LiveWindow> {
        // Top of the z-order first, as Windows lists them.
        vec![
            live_window(0x7a01, "Inbox", "Mail.App", 0),
            live_window(0x7a02, "Docs", "Browser.App", 0),
            live_window(0x7a03, "News", "Browser.App", 2000),
            live_window(0x7a04, "Code", "Browser.App", 0),
        ]
    }

    fn no_place(_: [i32; 4]) -> (Option<String>, Option<usize>) {
        (None, None)
    }

    fn hwnds(group: &ArrangeGroup) -> Vec<isize> {
        group.windows.iter().map(|window| window.hwnd).collect()
    }

    #[test]
    fn groups_are_built_from_the_open_windows_alone() {
        let mut own = OwnWindows::new(true);
        own.track(&sample(), 0);
        let desktop_of = |hwnd: Handle| {
            if hwnd == 0x7a03 {
                "{D2}".to_string()
            } else {
                String::new()
            }
        };
        let built = strip_groups::window_groups(
            own.groups_with(&desktop_of, &no_place),
            "Mail.App",
            Vec::new(),
            WATCHED.to_string(),
            Vec::new(),
        );
        let keys: Vec<&str> = built.groups.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, ["Browser.App", "Mail.App"], "most windows first");
        assert_eq!(built.focus, 1, "the front window's app");
        assert_eq!(built.groups[0].label, "Browser.App", "the app's own name");
        // A group seen the first time is in Windows' order of adding:
        // bottom of the z-order first.
        assert_eq!(hwnds(&built.groups[0]), [0x7a04, 0x7a03, 0x7a02]);
        assert_eq!(built.groups[0].windows[1].desktop, "{D2}");
        assert!(built.groups[0]
            .windows
            .iter()
            .all(|window| window.saved_monitor.is_none()));
        assert_eq!(built.watched, ["any app"]);
    }

    #[test]
    fn a_new_window_joins_its_group_at_the_end_and_a_closed_one_goes() {
        let mut own = OwnWindows::new(true);
        own.track(&sample(), 0);
        let mut later = sample();
        later.insert(0, live_window(0x7a05, "New", "Browser.App", 0));
        own.track(&later, 1);
        let groups = own.groups_with(&|_| String::new(), &no_place);
        let browser = groups.iter().find(|g| g.key == "Browser.App").unwrap();
        assert_eq!(hwnds(browser), [0x7a04, 0x7a03, 0x7a02, 0x7a05]);

        later.retain(|window| window.hwnd as Handle != 0x7a03);
        own.track(&later, 2);
        let groups = own.groups_with(&|_| String::new(), &no_place);
        let browser = groups.iter().find(|g| g.key == "Browser.App").unwrap();
        assert_eq!(hwnds(browser), [0x7a04, 0x7a02, 0x7a05]);
        // A window of the same program opened soon after takes the closed
        // one's place, as a window that comes back does.
        later.insert(0, live_window(0x7a06, "News", "Browser.App", 2000));
        own.track(&later, 3);
        let groups = own.groups_with(&|_| String::new(), &no_place);
        let browser = groups.iter().find(|g| g.key == "Browser.App").unwrap();
        assert_eq!(hwnds(browser), [0x7a04, 0x7a06, 0x7a02, 0x7a05]);
        // An app whose last window closed is gone after the wait.
        let only_browser: Vec<LiveWindow> = later
            .into_iter()
            .filter(|window| window.group == "Browser.App")
            .collect();
        own.track(&only_browser, 4);
        assert!(own
            .groups_with(&|_| String::new(), &no_place)
            .iter()
            .all(|g| g.key != "Mail.App"));
        own.track(&only_browser, 4 + DROP_AFTER_SECONDS);
        assert!(!own.groups.contains_key("Mail.App"));
    }

    #[test]
    fn with_no_windows_there_are_no_groups() {
        let mut own = OwnWindows::new(true);
        own.track(&[], 0);
        assert!(own.groups_with(&|_| String::new(), &no_place).is_empty());
        let built = strip_groups::window_groups(
            Vec::new(),
            "",
            Vec::new(),
            WATCHED.to_string(),
            Vec::new(),
        );
        assert_eq!(built.focus, 0);
    }

    #[test]
    fn an_unlabelled_group_gets_a_readable_name() {
        let own = OwnWindows::new(true);
        assert_eq!(own.label_of("Some.Vendor.Tool"), "Tool");
    }

    #[test]
    fn a_reorder_sends_only_this_desktops_buttons_in_the_new_order() {
        let mut own = OwnWindows::new(false);
        own.track(&sample(), 0);
        let elsewhere = |hwnd: Handle| hwnd == 0x7a03;
        let send = own.reorder("Browser.App", &[0x7a02, 0x7a03, 0x7a04], &elsewhere);
        assert_eq!(send, Some(vec![0x7a02, 0x7a04]));
        let groups = own.groups_with(&|_| String::new(), &no_place);
        let browser = groups.iter().find(|g| g.key == "Browser.App").unwrap();
        assert_eq!(hwnds(browser), [0x7a02, 0x7a03, 0x7a04]);
    }

    #[test]
    fn a_reorder_of_an_unknown_group_or_only_other_desktops_sends_nothing() {
        let mut own = OwnWindows::new(false);
        own.track(&sample(), 0);
        assert_eq!(own.reorder("Nope.App", &[0x7a01], &|_| false), None);
        assert_eq!(own.reorder("Mail.App", &[0x7a01], &|_| true), None);
        // Nothing left to send: the same order again changes nothing.
        assert_eq!(own.reorder("Mail.App", &[0x7a01], &|_| false), None);
    }

    #[test]
    fn a_read_only_instance_turns_every_window_change_down() {
        let mut own = OwnWindows::new(true);
        own.track(&sample(), 0);
        let actions = [
            ArrangeAction::Activate(0x7a01),
            ArrangeAction::Close(0x7a01),
            ArrangeAction::Reorder {
                group: "Browser.App".to_string(),
                order: vec![0x7a02, 0x7a03, 0x7a04],
            },
            ArrangeAction::MoveToDesktop {
                hwnd: 0x7a01,
                desktop: "{00000000-0000-0000-0000-000000000001}".to_string(),
            },
            ArrangeAction::MoveToMonitor {
                hwnd: 0x7a01,
                monitor: "\\\\.\\DISPLAY2".to_string(),
            },
        ];
        for action in &actions {
            assert_eq!(own.act(action), After::Nothing, "{action:?}");
        }
        assert_eq!(own.refused, actions.len() as u32);
        let groups = own.groups_with(&|_| String::new(), &no_place);
        let browser = groups.iter().find(|g| g.key == "Browser.App").unwrap();
        assert_eq!(hwnds(browser), [0x7a04, 0x7a03, 0x7a02], "order untouched");
    }

    #[test]
    fn names_are_not_this_plugins_business() {
        let mut own = OwnWindows::new(false);
        let rename = ArrangeAction::Rename {
            hwnd: 0x7a01,
            name: Some("x".to_string()),
        };
        assert_eq!(own.act(&rename), After::Nothing);
        assert_eq!(own.act(&ArrangeAction::AskName(0x7a01)), After::Nothing);
        assert_eq!(own.refused, 0);
    }

    #[test]
    fn a_desktop_that_cannot_be_read_moves_nothing() {
        let mut own = OwnWindows::new(false);
        // Returns before any desktop or window is touched.
        own.move_to_desktop(0x7a01, "not a desktop id");
        assert!(matches!(own.mover, LazyMover::Untried));
    }

    #[test]
    fn clearing_forgets_the_groups_but_stays_read_only() {
        let mut own = OwnWindows::new(true);
        own.track(&sample(), 0);
        own.clear();
        assert!(own.groups.is_empty() && own.labels.is_empty());
        assert!(own.read_only);
    }
}
