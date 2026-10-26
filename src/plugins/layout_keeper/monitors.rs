//! What LayoutKeeper does with the monitors a window was saved on: finding a
//! saved monitor again, where a restore puts a window, and keeping the
//! monitor of a window whose monitor is unplugged. The monitors connected
//! now come from `core::monitor_list`.

use super::identity;
use super::state::{SavedMonitor, SavedWindow};
pub use crate::core::monitor_list::{
    for_strip, ids as saved_names, index_of, list, move_window, strip_label, Monitor,
};

/// The connected monitor that a saved one is, or None when it is not
/// connected. The friendly name decides; a monitor without one (many laptop
/// panels, or when Windows cannot say) is matched by its device name. Two
/// monitors of the same model are told apart by device name, then by place,
/// and left unmatched when neither settles it.
pub fn find(saved: &SavedMonitor, live: &[SavedMonitor]) -> Option<usize> {
    let candidates: Vec<usize> = (0..live.len())
        .filter(|index| {
            let monitor = &live[*index];
            monitor.name == saved.name
                || (monitor.name.is_empty() && monitor.device == saved.device)
        })
        .collect();
    match candidates.as_slice() {
        [] => None,
        [only] => Some(*only),
        several => several
            .iter()
            .find(|index| live[**index].device == saved.device)
            .or_else(|| {
                several
                    .iter()
                    .find(|index| live[**index].position == saved.position)
            })
            .copied(),
    }
}

/// What a restore does about a window's monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Return {
    /// It is on its saved monitor, or none was saved.
    Stay,
    /// Its saved monitor is not connected: it stays where it is and keeps
    /// the saved one, rather than being piled onto the primary monitor.
    Unplugged,
    /// Back to the connected monitor at this place in the list.
    To(usize),
}

/// Where a restore puts a window saved on `saved` that is on the monitor at
/// place `now` in the list, from `live`, the list's saved names.
pub fn restore_target(
    saved: Option<&SavedMonitor>,
    now: Option<usize>,
    live: &[SavedMonitor],
) -> Return {
    let Some(saved) = saved else {
        return Return::Stay;
    };
    match find(saved, live) {
        None => Return::Unplugged,
        Some(target) if now.is_some_and(|now| now != target) => Return::To(target),
        Some(_) => Return::Stay,
    }
}

/// A fresh look at one program's windows keeps the monitor of a window
/// saved on one that is unplugged now, so a restore once it is plugged in
/// again still knows where the window goes. Windows are paired with the
/// previous save the way a restore pairs them. `released[i]` is true for a
/// fresh window the user has since put on a monitor in the strip.
pub fn keep_unplugged(
    exe: &str,
    previous: &[SavedWindow],
    fresh: &mut [SavedWindow],
    released: &[bool],
    live: &[SavedMonitor],
) {
    let before: Vec<identity::WindowIdentity> =
        previous.iter().map(|window| window.identity(exe)).collect();
    let now: Vec<identity::WindowIdentity> =
        fresh.iter().map(|window| window.identity(exe)).collect();
    for (s, l) in identity::match_windows(&before, &now).pairs {
        let Some(monitor) = &previous[s].monitor else {
            continue;
        };
        if find(monitor, live).is_none() && !released.get(l).copied().unwrap_or(false) {
            fresh[l].monitor = Some(monitor.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(device: &str, name: &str, position: usize) -> SavedMonitor {
        SavedMonitor {
            device: format!(r"\\.\{device}"),
            name: name.to_string(),
            position,
        }
    }

    #[test]
    fn a_saved_monitor_is_found_by_its_name_whatever_its_device_is_now() {
        let live = [
            saved("DISPLAY2", "LG HDR 4K", 1),
            saved("DISPLAY1", "DELL P2419H", 0),
            saved("DISPLAY3", "LG SDQHD", 2),
        ];
        // After a reboot the right monitor came up as DISPLAY3, not 2.
        assert_eq!(find(&saved("DISPLAY2", "LG SDQHD", 2), &live), Some(2));
        assert_eq!(find(&saved("DISPLAY9", "DELL P2419H", 0), &live), Some(1));
        // Unplugged: nothing else has its name, so it is not connected.
        assert_eq!(find(&saved("DISPLAY4", "Samsung", 3), &live), None);
    }

    #[test]
    fn two_monitors_of_one_model_are_told_apart_by_device_then_place() {
        let live = [saved("DISPLAY1", "DELL", 0), saved("DISPLAY2", "DELL", 1)];
        assert_eq!(find(&saved("DISPLAY2", "DELL", 0), &live), Some(1));
        assert_eq!(find(&saved("DISPLAY7", "DELL", 1), &live), Some(1));
        assert_eq!(find(&saved("DISPLAY7", "DELL", 5), &live), None);
    }

    #[test]
    fn a_monitor_without_a_name_is_found_by_its_device() {
        // A laptop panel, or Windows could not read the names this time.
        let live = [saved("DISPLAY1", "", 0), saved("DISPLAY2", "", 1)];
        assert_eq!(find(&saved("DISPLAY2", "LG SDQHD", 1), &live), Some(1));
        assert_eq!(find(&saved("DISPLAY1", "", 0), &live), Some(0));
        assert_eq!(find(&saved("DISPLAY3", "LG SDQHD", 2), &live), None);
    }

    #[test]
    fn a_restore_moves_a_window_only_to_a_connected_saved_monitor() {
        let live = [
            saved("DISPLAY1", "LG HDR 4K", 1),
            saved("DISPLAY2", "DELL", 0),
        ];
        let dell = saved("DISPLAY2", "DELL", 0);
        assert_eq!(restore_target(Some(&dell), Some(0), &live), Return::To(1));
        assert_eq!(restore_target(Some(&dell), Some(1), &live), Return::Stay);
        assert_eq!(restore_target(None, Some(0), &live), Return::Stay);
        // Unplugged: left where it is, not piled onto the primary monitor.
        let gone = saved("DISPLAY3", "LG SDQHD", 2);
        assert_eq!(
            restore_target(Some(&gone), Some(0), &live),
            Return::Unplugged
        );
    }

    fn window(title: &str, monitor: Option<SavedMonitor>) -> SavedWindow {
        SavedWindow {
            name: Some(title.to_string()),
            title: title.to_string(),
            rect: [0, 0, 1200, 800],
            maximized: false,
            desktop: None,
            desktop_name: None,
            group: "Chrome".to_string(),
            taskbar_index: 0,
            z_index: 0,
            monitor,
            seen: None,
        }
    }

    #[test]
    fn a_save_keeps_the_monitor_of_a_window_whose_monitor_is_unplugged() {
        let live = [saved("DISPLAY1", "LG HDR 4K", 0)];
        let primary = Some(live[0].clone());
        let gone = Some(saved("DISPLAY3", "LG SDQHD", 1));
        let previous = [
            window("Mail", gone.clone()),
            window("Docs", primary.clone()),
        ];
        // Both are on the primary monitor now; Docs was there already.
        let mut fresh = [
            window("Docs", primary.clone()),
            window("Mail", primary.clone()),
        ];
        keep_unplugged("chrome.exe", &previous, &mut fresh, &[false, false], &live);
        assert_eq!(fresh[0].monitor, primary);
        assert_eq!(fresh[1].monitor, gone);

        // Put on the primary monitor in the strip: that is its place now.
        let mut fresh = [
            window("Docs", primary.clone()),
            window("Mail", primary.clone()),
        ];
        keep_unplugged("chrome.exe", &previous, &mut fresh, &[false, true], &live);
        assert_eq!(fresh[1].monitor, primary);

        // Plugged in again, nothing is kept: the window is wherever it is.
        let both = [live[0].clone(), saved("DISPLAY3", "LG SDQHD", 1)];
        let mut fresh = [window("Mail", primary.clone())];
        keep_unplugged("chrome.exe", &previous, &mut fresh, &[false], &both);
        assert_eq!(fresh[0].monitor, primary);
    }
}
