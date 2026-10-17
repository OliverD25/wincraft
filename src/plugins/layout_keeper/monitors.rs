//! Which monitor each window is on, recorded with every snapshot so the
//! strip can group windows by monitor. Three names for one monitor: the GDI
//! device name Windows uses internally ("\\.\DISPLAY1"), the name a person
//! knows it by ("DELL U2720Q"), and its place from the left. Neither the
//! device name nor the place is sure to survive a reboot or a monitor being
//! unplugged, so a saved monitor is found again by all three (`find`).

use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromRect, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

use super::identity::Rect;
use super::state::SavedMonitor;
use crate::core::monitors::position_names;
use crate::core::ui_bridge::ArrangeMonitor;

pub struct Monitor {
    handle: HMONITOR,
    saved: SavedMonitor,
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
                saved: SavedMonitor {
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

/// Every monitor as the saved layout names it, in the order of `list`.
pub fn saved_names(monitors: &[Monitor]) -> Vec<SavedMonitor> {
    monitors
        .iter()
        .map(|monitor| monitor.saved.clone())
        .collect()
}

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

/// The monitors for the strip: the connected ones in the order of `list`,
/// then `disconnected`, which are saved monitors not connected now.
pub fn for_strip(monitors: &[Monitor], disconnected: &[SavedMonitor]) -> Vec<ArrangeMonitor> {
    let lefts: Vec<i32> = monitors.iter().map(|monitor| monitor.rect[0]).collect();
    let places = position_names(&lefts);
    let mut strip: Vec<ArrangeMonitor> = monitors
        .iter()
        .zip(places)
        .map(|(monitor, place)| ArrangeMonitor {
            device: monitor.saved.device.clone(),
            label: place.unwrap_or_else(|| fallback_label(&monitor.saved)),
            name: monitor.saved.name.clone(),
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
fn fallback_label(saved: &SavedMonitor) -> String {
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

/// The monitor a window's rectangle is mostly on; for a minimized window
/// the rectangle is the one it will come back to.
pub fn of_rect(rect: Rect, monitors: &[Monitor]) -> Option<SavedMonitor> {
    index_of(rect, monitors).map(|index| monitors[index].saved.clone())
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

/// GDI device name to friendly name, for every active display path. The
/// friendly name comes from the monitor's EDID and can be empty, as on
/// many laptop panels.
fn friendly_names() -> Vec<(String, String)> {
    let (mut path_count, mut mode_count) = (0u32, 0u32);
    if unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
    } != ERROR_SUCCESS
    {
        return Vec::new();
    }
    let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> =
        vec![unsafe { std::mem::zeroed() }; path_count as usize];
    let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> =
        vec![unsafe { std::mem::zeroed() }; mode_count as usize];
    let status = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Vec::new();
    }
    paths.truncate(path_count as usize);
    paths
        .iter()
        .filter_map(|path| {
            let mut source: DISPLAYCONFIG_SOURCE_DEVICE_NAME = unsafe { std::mem::zeroed() };
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0 {
                return None;
            }
            let mut target: DISPLAYCONFIG_TARGET_DEVICE_NAME = unsafe { std::mem::zeroed() };
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0 {
                return None;
            }
            Some((
                from_wide(&source.viewGdiDeviceName),
                from_wide(&target.monitorFriendlyDeviceName),
            ))
        })
        .collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
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
    fn a_device_name_ends_at_its_nul() {
        let mut buffer = [0u16; 32];
        for (slot, unit) in buffer.iter_mut().zip(r"\\.\DISPLAY1".encode_utf16()) {
            *slot = unit;
        }
        assert_eq!(from_wide(&buffer), r"\\.\DISPLAY1");
    }
}
