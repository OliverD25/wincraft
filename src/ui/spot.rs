//! Where the palette sits on its monitor: which monitor a window or a work
//! area belongs to, and the spot to open at.

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromRect, MonitorFromWindow, HMONITOR, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

use crate::core::config::{centred_spot, PaletteSpot, WorkArea};
use crate::core::ui_bridge::MonitorRect;

/// A monitor: its device name, such as `\\.\DISPLAY1`, and its work area.
pub struct Monitor {
    pub name: String,
    pub work: WorkArea,
}

fn describe(monitor: HMONITOR) -> Option<Monitor> {
    let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    let ok = unsafe {
        GetMonitorInfoW(
            monitor,
            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
        )
    };
    if ok == 0 {
        return None;
    }
    let end = info
        .szDevice
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(info.szDevice.len());
    let work = info.monitorInfo.rcWork;
    Some(Monitor {
        name: String::from_utf16_lossy(&info.szDevice[..end]),
        work: (work.left, work.top, work.right, work.bottom),
    })
}

/// The monitor whose work area the host reported.
pub fn of_work_area(area: MonitorRect) -> Option<Monitor> {
    let rect = RECT {
        left: area.left,
        top: area.top,
        right: area.right,
        bottom: area.bottom,
    };
    describe(unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) })
}

/// The monitor a window is mostly on, and the window's top-left corner.
pub fn of_window(hwnd: isize) -> Option<(Monitor, (i32, i32))> {
    if hwnd == 0 {
        return None;
    }
    let hwnd = hwnd as HWND;
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    let monitor = describe(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) })?;
    Some((monitor, (rect.left, rect.top)))
}

/// Where the panel's top-left corner goes on this work area: the spot the
/// user dragged it to, kept inside the work area, or the default one.
pub fn panel_corner(saved: Option<PaletteSpot>, work: WorkArea, size: (i32, i32)) -> (i32, i32) {
    match saved {
        Some(spot) => spot.to_screen(work, size),
        None => centred_spot(work, size.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: WorkArea = (0, 0, 1920, 1040);

    #[test]
    fn no_saved_spot_means_the_centre() {
        assert_eq!(panel_corner(None, WORK, (880, 520)), (520, 187));
    }

    #[test]
    fn a_saved_spot_is_clamped_into_the_work_area() {
        let spot = Some(PaletteSpot { dx: 3000, dy: 10 });
        assert_eq!(panel_corner(spot, WORK, (880, 520)), (1040, 10));
    }
}
