//! Building the strip's data from windows in taskbar order: one group per
//! app, its windows with their desktop and monitor, the apps with the most
//! windows first. LayoutKeeper builds it from its saved order; WindowDeck
//! builds it from the windows open now when LayoutKeeper is off.

use windows_sys::Win32::Foundation::HWND;

use crate::core::monitor_list::{self, Monitor};
use crate::core::taskbar_order::Handle;
use crate::core::traits::WindowGroups;
use crate::core::ui_bridge::{ArrangeDesktop, ArrangeGroup, ArrangeMonitor, ArrangeWindow};
use crate::core::virtual_desktops::{Desktop, DesktopId, Reader};
use crate::core::window_identity::{self as identity, Rect, WindowIdentity};
use crate::core::window_names;
use crate::core::window_snapshot;

/// What the strip shows of where a window is: its monitor's place word
/// (None on the primary monitor) and its place in the monitor list.
pub type Place<'a> = dyn Fn(Rect) -> (Option<String>, Option<usize>) + 'a;

/// `Place` for the monitors connected now.
pub fn place_on(monitors: &[Monitor]) -> impl Fn(Rect) -> (Option<String>, Option<usize>) + '_ {
    move |rect| {
        (
            monitor_list::strip_label(rect, monitors),
            monitor_list::index_of(rect, monitors),
        )
    }
}

/// The id of the desktop a window is on, as the strip spells it: empty when
/// it cannot be read or the window is on every desktop.
/// A window snapshot in use (see `core::window_snapshot`) answers first.
pub fn desktop_of(reader: Option<&Reader>, hwnd: Handle) -> String {
    let read = |reader: &Reader| match window_snapshot::desktop_of(hwnd) {
        Some(known) => known,
        None => reader.read(hwnd as HWND),
    };
    reader
        .and_then(read)
        .filter(|id| *id != DesktopId::ALL)
        .map(|id| id.to_string())
        .unwrap_or_default()
}

/// One app's group for the strip, its windows in `windows`' order (the
/// taskbar order), or None when it has none. `desktop_of` gives a window's
/// desktop id ("" when unknown or on every desktop); `saved_monitor` the
/// monitor it was saved on, for LayoutKeeper's "Moved" chip.
pub fn group<'a>(
    key: &str,
    label: String,
    windows: impl Iterator<Item = (Handle, &'a WindowIdentity)>,
    desktop_of: &dyn Fn(Handle) -> String,
    place: &Place<'_>,
    saved_monitor: &dyn Fn(Handle) -> Option<usize>,
) -> Option<ArrangeGroup> {
    let mut windows: Vec<ArrangeWindow> = windows
        .map(|(hwnd, identity)| {
            let name = window_names::custom(hwnd);
            let (monitor, on_monitor) = place(identity.rect);
            ArrangeWindow {
                hwnd,
                renamed: name.is_some(),
                label: name.unwrap_or_else(|| identity.label().to_string()),
                desktop: desktop_of(hwnd),
                monitor,
                on_monitor,
                saved_monitor: saved_monitor(hwnd),
                same_title: false,
                attention: false,
            }
        })
        .collect();
    let titles: Vec<&str> = windows.iter().map(|w| w.label.as_str()).collect();
    let same = identity::same_titles(&titles);
    for (window, same) in windows.iter_mut().zip(same) {
        window.same_title = same;
    }
    (!windows.is_empty()).then(|| ArrangeGroup {
        key: key.to_string(),
        label,
        windows,
    })
}

/// The switcher lists the apps with the most windows first, then by name.
pub fn sort(groups: &mut [ArrangeGroup]) {
    groups.sort_by(|a, b| {
        b.windows
            .len()
            .cmp(&a.windows.len())
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
}

/// The strip opens on the front window's taskbar group when it is listed,
/// else on the first.
pub fn focus(groups: &[ArrangeGroup], front_key: &str) -> usize {
    groups
        .iter()
        .position(|group| group.key == front_key)
        .unwrap_or(0)
}

/// The desktops in Task View order, the current one marked.
pub fn desktops(registry: &[Desktop], current: Option<DesktopId>) -> Vec<ArrangeDesktop> {
    registry
        .iter()
        .map(|desktop| ArrangeDesktop {
            id: desktop.id.to_string(),
            name: desktop.name.clone(),
            current: Some(desktop.id) == current,
        })
        .collect()
}

/// Everything the strip needs, from groups already built.
pub fn window_groups(
    mut groups: Vec<ArrangeGroup>,
    front_key: &str,
    desktops: Vec<ArrangeDesktop>,
    watched: String,
    monitors: Vec<ArrangeMonitor>,
) -> WindowGroups {
    sort(&mut groups);
    let focus = focus(&groups, front_key);
    WindowGroups {
        groups,
        desktops,
        focus,
        watched: vec![watched],
        monitors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(title: &str, left: i32) -> WindowIdentity {
        WindowIdentity::new("chrome.exe", title, [left, 0, left + 800, 600], false)
    }

    fn no_place(_: Rect) -> (Option<String>, Option<usize>) {
        (None, None)
    }

    #[test]
    fn a_group_keeps_the_order_it_is_given_and_marks_shared_titles() {
        let windows = [
            (0x7001, identity("Mail", 0)),
            (0x7002, identity("Docs", 0)),
            (0x7003, identity("Mail", 2000)),
        ];
        let desktop_of = |hwnd: Handle| {
            if hwnd == 0x7002 {
                "{OTHER}".to_string()
            } else {
                String::new()
            }
        };
        let place = |rect: Rect| {
            if rect[0] >= 1920 {
                (Some("Right".to_string()), Some(1))
            } else {
                (None, Some(0))
            }
        };
        let saved = |hwnd: Handle| (hwnd == 0x7003).then_some(0);
        let group = group(
            "Chrome",
            "Chrome".to_string(),
            windows.iter().map(|(hwnd, identity)| (*hwnd, identity)),
            &desktop_of,
            &place,
            &saved,
        )
        .unwrap();
        let hwnds: Vec<isize> = group.windows.iter().map(|w| w.hwnd).collect();
        assert_eq!(hwnds, [0x7001, 0x7002, 0x7003]);
        let same: Vec<bool> = group.windows.iter().map(|w| w.same_title).collect();
        assert_eq!(same, [true, false, true]);
        assert_eq!(group.windows[1].desktop, "{OTHER}");
        assert_eq!(group.windows[2].monitor.as_deref(), Some("Right"));
        assert_eq!(group.windows[2].on_monitor, Some(1));
        assert_eq!(group.windows[2].saved_monitor, Some(0));
        assert_eq!(group.windows[0].saved_monitor, None);
        assert!(group.windows.iter().all(|w| !w.attention && !w.renamed));
    }

    #[test]
    fn a_group_without_windows_is_left_out() {
        let none: [(Handle, WindowIdentity); 0] = [];
        let built = group(
            "Empty",
            "Empty".to_string(),
            none.iter().map(|(hwnd, identity)| (*hwnd, identity)),
            &|_| String::new(),
            &no_place,
            &|_| None,
        );
        assert!(built.is_none());
    }

    fn named(key: &str, label: &str, count: usize) -> ArrangeGroup {
        let windows = (0..count)
            .map(|index| (index as Handle + 1, identity("w", 0)))
            .collect::<Vec<_>>();
        group(
            key,
            label.to_string(),
            windows.iter().map(|(hwnd, identity)| (*hwnd, identity)),
            &|_| String::new(),
            &no_place,
            &|_| None,
        )
        .unwrap()
    }

    #[test]
    fn the_busiest_app_comes_first_and_the_front_one_is_focused() {
        let built = window_groups(
            vec![
                named("b", "beta", 1),
                named("a", "Alpha", 1),
                named("c", "Code", 3),
            ],
            "a",
            Vec::new(),
            "any app".to_string(),
            Vec::new(),
        );
        let keys: Vec<&str> = built.groups.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, ["c", "a", "b"], "ties go by name, in any case");
        assert_eq!(built.focus, 1);
        assert_eq!(built.watched, ["any app"]);
        assert_eq!(focus(&built.groups, "not listed"), 0);
        assert_eq!(focus(&[], "a"), 0);
    }

    #[test]
    fn the_current_desktop_is_marked() {
        let registry = [
            Desktop {
                id: DesktopId(1),
                name: "Work".to_string(),
            },
            Desktop {
                id: DesktopId(2),
                name: "Home".to_string(),
            },
        ];
        let listed = desktops(&registry, Some(DesktopId(2)));
        let current: Vec<bool> = listed.iter().map(|d| d.current).collect();
        assert_eq!(current, [false, true]);
        assert_eq!(listed[0].name, "Work");
        assert!(desktops(&registry, None).iter().all(|d| !d.current));
    }
}
