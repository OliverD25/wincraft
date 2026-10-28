//! The monitors connected now, as the strip shows them and as
//! LayoutKeeper records them with every snapshot. Three names for one
//! monitor: the GDI device name Windows uses internally ("\\.\DISPLAY1"),
//! the name a person knows it by ("DELL U2720Q"), and its place from the
//! left. Neither the
//! device name nor the place is sure to survive a reboot or a monitor being
//! unplugged, so LayoutKeeper finds a saved monitor again by all three.

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromRect, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

use serde::{Deserialize, Serialize};

use crate::core::monitors::{
    self as core_monitors, friendly_names, from_wide, position_names, Rect,
};
use crate::core::ui_bridge::ArrangeMonitor;

/// The three names of one monitor; LayoutKeeper saves it with each window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonitorId {
    /// The GDI device name, such as "\\.\DISPLAY1".
    pub device: String,
    /// The name from the monitor itself, such as "DELL U2720Q"; can be empty.
    pub name: String,
    /// Place from the left, 0 first.
    pub position: usize,
}

pub struct Monitor {
    handle: HMONITOR,
    id: MonitorId,
    rect: Rect,
    work: Rect,
    primary: bool,
}

struct Found {
    handle: HMONITOR,
    device: String,
    rect: RECT,
    work: RECT,
    primary: bool,
}

/// Every active monitor, numbered from the left.
pub fn list() -> Vec<Monitor> {
    let mut found: Vec<Found> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut found as *mut Vec<Found> as LPARAM,
        )
    };
    let friendly = friendly_names();
    let order = left_to_right(
        &found
            .iter()
            .map(|monitor| (monitor.rect.left, monitor.rect.top))
            .collect::<Vec<_>>(),
    );
    found
        .into_iter()
        .zip(order)
        .map(|(monitor, position)| {
            let name = friendly
                .iter()
                .find(|(gdi, _)| *gdi == monitor.device)
                .map(|(_, name)| name.clone())
                .unwrap_or_default();
            Monitor {
                handle: monitor.handle,
                id: MonitorId {
                    device: monitor.device,
                    name,
                    position,
                },
                rect: corners(monitor.rect),
                work: corners(monitor.work),
                primary: monitor.primary,
            }
        })
        .collect()
}

fn corners(rect: RECT) -> Rect {
    [rect.left, rect.top, rect.right, rect.bottom]
}

impl Monitor {
    pub fn id(&self) -> &MonitorId {
        &self.id
    }

    /// For the detailed log's session header: the handle, the whole area,
    /// the work area and whether it is the primary monitor.
    pub fn parts(&self) -> (HMONITOR, Rect, Rect, bool) {
        (self.handle, self.rect, self.work, self.primary)
    }
}

/// Every monitor's names, in the order of `list`.
pub fn ids(monitors: &[Monitor]) -> Vec<MonitorId> {
    monitors.iter().map(|monitor| monitor.id.clone()).collect()
}

/// The monitors for the strip: the connected ones in the order of `list`,
/// then `disconnected`, which are saved monitors not connected now.
pub fn for_strip(monitors: &[Monitor], disconnected: &[MonitorId]) -> Vec<ArrangeMonitor> {
    let lefts: Vec<i32> = monitors.iter().map(|monitor| monitor.rect[0]).collect();
    let places = position_names(&lefts);
    let mut strip: Vec<ArrangeMonitor> = monitors
        .iter()
        .zip(places)
        .map(|(monitor, place)| ArrangeMonitor {
            device: monitor.id.device.clone(),
            label: place.unwrap_or_else(|| fallback_label(&monitor.id)),
            name: monitor.id.name.clone(),
            rect: monitor.rect,
            work: monitor.work,
            primary: monitor.primary,
            connected: true,
        })
        .collect();
    strip.extend(disconnected.iter().map(|saved| ArrangeMonitor {
        device: saved.device.clone(),
        label: fallback_label(saved),
        name: saved.name.clone(),
        rect: [0; 4],
        work: [0; 4],
        primary: false,
        connected: false,
    }));
    strip
}

/// A monitor's name when its place gives none: alone, or no longer there.
fn fallback_label(saved: &MonitorId) -> String {
    if saved.name.is_empty() {
        format!("Monitor {}", saved.position + 1)
    } else {
        saved.name.clone()
    }
}

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> i32 {
    let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if unsafe {
        GetMonitorInfoW(
            monitor,
            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
        )
    } != 0
    {
        let found = unsafe { &mut *(data as *mut Vec<Found>) };
        found.push(Found {
            handle: monitor,
            device: from_wide(&info.szDevice),
            rect: info.monitorInfo.rcMonitor,
            work: info.monitorInfo.rcWork,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    1
}

/// What the Arrange strip says about a window's monitor: its place word,
/// or nothing when the window is on the primary monitor.
pub fn strip_label(rect: Rect, monitors: &[Monitor]) -> Option<String> {
    let index = index_of(rect, monitors)?;
    if monitors[index].primary {
        return None;
    }
    let lefts: Vec<i32> = monitors.iter().map(|monitor| monitor.rect[0]).collect();
    position_names(&lefts).swap_remove(index)
}

/// The place in `monitors` of the monitor a rectangle is mostly on.
pub fn index_of(rect: Rect, monitors: &[Monitor]) -> Option<usize> {
    let rect = RECT {
        left: rect[0],
        top: rect[1],
        right: rect[2],
        bottom: rect[3],
    };
    let handle = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    monitors.iter().position(|monitor| monitor.handle == handle)
}

/// Moves a window from one monitor to another without activating it; see
/// `core::monitors::move_window`.
pub fn move_window(
    hwnd: HWND,
    from: &Monitor,
    to: &Monitor,
    monitors: &[Monitor],
) -> Result<(), String> {
    let primary = monitors
        .iter()
        .find(|monitor| monitor.primary)
        .map_or([0, 0, 0, 0], |primary| primary.work);
    core_monitors::move_window(hwnd, from.work, to.work, primary)
}

/// Each monitor's place from the left, 0 first; monitors stacked above one
/// another are counted from the top.
fn left_to_right(corners: &[(i32, i32)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..corners.len()).collect();
    order.sort_by_key(|index| corners[*index]);
    let mut position = vec![0; corners.len()];
    for (place, index) in order.into_iter().enumerate() {
        position[index] = place;
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitors_are_numbered_from_the_left() {
        // Primary in the middle at 0, one to the left, one to the right.
        assert_eq!(left_to_right(&[(0, 0), (-1920, 0), (2560, 200)]), [1, 0, 2]);
        // Two stacked at the same left edge: the upper one first.
        assert_eq!(left_to_right(&[(0, 1080), (0, 0)]), [1, 0]);
        assert!(left_to_right(&[]).is_empty());
    }

    fn monitor(device: &str, name: &str, position: usize, left: i32, primary: bool) -> Monitor {
        Monitor {
            handle: std::ptr::null_mut(),
            id: MonitorId {
                device: device.to_string(),
                name: name.to_string(),
                position,
            },
            rect: [left, 0, left + 1920, 1080],
            work: [left, 0, left + 1920, 1040],
            primary,
        }
    }

    #[test]
    fn the_strip_names_monitors_by_place_and_adds_the_unplugged_ones_last() {
        let monitors = [
            monitor("D1", "DELL", 1, 0, true),
            monitor("D2", "LG", 0, -1920, false),
        ];
        let unplugged = [MonitorId {
            device: "D3".to_string(),
            name: String::new(),
            position: 2,
        }];
        let strip = for_strip(&monitors, &unplugged);
        let labels: Vec<&str> = strip.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["Right", "Left", "Monitor 3"]);
        assert!(strip[0].primary && strip[0].connected);
        assert_eq!(strip[0].work, [0, 0, 1920, 1040]);
        assert!(!strip[2].connected);
        assert_eq!(strip[2].rect, [0; 4]);
        assert_eq!(ids(&monitors)[1].name, "LG");
        assert_eq!(monitors[0].id().device, "D1");
    }

    #[test]
    fn a_lone_monitor_is_called_by_its_own_name_or_its_number() {
        let strip = for_strip(&[monitor("D1", "DELL", 0, 0, true)], &[]);
        assert_eq!(strip.len(), 1);
        assert_eq!(strip[0].label, "DELL");
        let strip = for_strip(&[monitor("D1", "", 0, 0, true)], &[]);
        assert_eq!(strip[0].label, "Monitor 1");
        assert!(for_strip(&[], &[]).is_empty());
    }
}
