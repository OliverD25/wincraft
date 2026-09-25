//! Which monitor each window is on, recorded with every snapshot so the
//! strip can later group windows by monitor. Three names for one monitor:
//! the GDI device name Windows uses internally ("\\.\DISPLAY1"), the name a
//! person knows it by ("DELL U2720Q"), and its place from the left.

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

pub struct Monitor {
    handle: HMONITOR,
    saved: SavedMonitor,
    left: i32,
    primary: bool,
}

struct Found {
    handle: HMONITOR,
    device: String,
    rect: RECT,
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
                left: monitor.rect.left,
                primary: monitor.primary,
            }
        })
        .collect()
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
    let lefts: Vec<i32> = monitors.iter().map(|monitor| monitor.left).collect();
    position_names(&lefts).swap_remove(index)
}

fn index_of(rect: Rect, monitors: &[Monitor]) -> Option<usize> {
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

    #[test]
    fn a_device_name_ends_at_its_nul() {
        let mut buffer = [0u16; 32];
        for (slot, unit) in buffer.iter_mut().zip(r"\\.\DISPLAY1".encode_utf16()) {
            *slot = unit;
        }
        assert_eq!(from_wide(&buffer), r"\\.\DISPLAY1");
    }
}
