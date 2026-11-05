//! Puts a plugin window on the screen: in the middle of the monitor under
//! the mouse, or back where the user left it. Positions are physical pixels,
//! so winit corrects the size itself when the monitor has another scale.

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
};

/// Left, top, right, bottom.
pub type Area = (i32, i32, i32, i32);

/// The top-left corner that puts a `size` window in the middle of `work`.
pub fn centred(work: Area, size: (i32, i32)) -> (i32, i32) {
    let x = (work.0 + (work.2 - work.0 - size.0) / 2).max(work.0);
    let y = (work.1 + (work.3 - work.1 - size.1) / 2).max(work.1);
    (x, y)
}

/// `at` moved just enough to keep the whole `size` window inside `work`, so
/// a monitor that got smaller or went away never hides it.
pub fn kept_inside(work: Area, size: (i32, i32), at: (i32, i32)) -> (i32, i32) {
    let x = at.0.min(work.2 - size.0).max(work.0);
    let y = at.1.min(work.3 - size.1).max(work.1);
    (x, y)
}

/// The window's top-left corner on the screen.
pub fn top_left(hwnd: HWND) -> Option<(i32, i32)> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if hwnd.is_null() || unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    Some((rect.left, rect.top))
}

/// Puts the window in the middle of the monitor under the mouse.
pub fn centre_on_cursor_monitor(hwnd: HWND) {
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return;
    }
    if let (Some(work), Some(size)) = (work_area_at(point), size_of(hwnd)) {
        move_to(hwnd, centred(work, size));
    }
}

/// Puts the window's top-left corner at `at`, kept inside the monitor
/// nearest to that point.
pub fn place_at(hwnd: HWND, at: (i32, i32)) {
    let point = POINT { x: at.0, y: at.1 };
    if let (Some(work), Some(size)) = (work_area_at(point), size_of(hwnd)) {
        move_to(hwnd, kept_inside(work, size, at));
    }
}

fn size_of(hwnd: HWND) -> Option<(i32, i32)> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if hwnd.is_null() || unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    Some((rect.right - rect.left, rect.bottom - rect.top))
}

fn work_area_at(point: POINT) -> Option<Area> {
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let work = info.rcWork;
    Some((work.left, work.top, work.right, work.bottom))
}

fn move_to(hwnd: HWND, at: (i32, i32)) {
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            at.0,
            at.1,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = (0, 0, 1920, 1040);

    #[test]
    fn a_window_goes_in_the_middle_of_the_work_area() {
        assert_eq!(centred(WORK, (1000, 600)), (460, 220));
        assert_eq!(centred((1920, 0, 3840, 1040), (1000, 600)), (2380, 220));
    }

    #[test]
    fn a_window_bigger_than_the_monitor_starts_at_its_top_left() {
        assert_eq!(centred(WORK, (2500, 1200)), (0, 0));
    }

    #[test]
    fn a_spot_inside_the_monitor_stays_where_it_is() {
        assert_eq!(kept_inside(WORK, (800, 500), (100, 200)), (100, 200));
    }

    #[test]
    fn a_spot_past_an_edge_is_pulled_back_inside() {
        assert_eq!(kept_inside(WORK, (800, 500), (1500, 900)), (1120, 540));
        assert_eq!(kept_inside(WORK, (800, 500), (-300, -50)), (0, 0));
    }
}
