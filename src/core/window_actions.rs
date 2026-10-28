//! What the strip asks to be done with one window that needs no saved
//! layout: switch to it, close it, move it to another monitor or desktop.
//! Each of these changes a real window, so a read-only test instance must
//! turn the request down before it calls one (Principle 1).

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, PostMessageW, ShowWindow, SW_RESTORE, WM_CLOSE,
};

use crate::core::monitor_list;
use crate::core::virtual_desktops::{self as desktops, Desktop, DesktopId, Reader};
use crate::core::window_identity::Rect;
use crate::core::{host, window_names};

/// Brings a window to the front, restoring it first when it is minimized.
pub fn activate(hwnd: HWND) {
    if unsafe { IsIconic(hwnd) } != 0 {
        let before = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        crate::core::detailed::call(
            "ShowWindow SW_RESTORE (was visible)",
            hwnd as isize,
            i64::from(before),
            false,
        );
    }
    host::bring_to_front(hwnd);
}

/// Asks a window to close, as its close button would.
pub fn close(hwnd: HWND) {
    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
}

/// Moves a window whose rectangle is `rect` now to the monitor with that
/// device name; nothing when it is there already, when that monitor is not
/// connected or when the window is gone (`rect` is None).
pub fn move_to_monitor(hwnd: HWND, device: &str, rect: Option<Rect>) {
    let monitors = monitor_list::list();
    let Some(to) = monitors.iter().position(|m| m.id().device == device) else {
        log::info!("monitor {device} is not connected; the window stays where it is");
        return;
    };
    let Some(from) = rect.and_then(|rect| monitor_list::index_of(rect, &monitors)) else {
        log::info!("the window to move to {device} is no longer open");
        return;
    };
    if from == to {
        return;
    }
    let label = window_names::display_title(hwnd);
    match monitor_list::move_window(hwnd, &monitors[from], &monitors[to], &monitors) {
        Ok(()) => log::info!("moved \"{label}\" to monitor {device}"),
        Err(err) => log::warn!("could not move \"{label}\" to monitor {device}: {err}"),
    }
}

/// Moves a window to another desktop. A window on every desktop, or a
/// desktop that no longer exists, is left alone.
pub fn move_to_desktop(
    mover: &desktops::Mover,
    reader: Option<&Reader>,
    hwnd: HWND,
    target: DesktopId,
    registry: &[Desktop],
    label: &str,
) {
    if target == DesktopId::ALL {
        return;
    }
    let Some(name) = desktops::name_of(registry, target) else {
        log::info!("\"{label}\" was on desktop {target}, which no longer exists");
        return;
    };
    if reader.and_then(|reader| reader.read(hwnd)) == Some(DesktopId::ALL) {
        return;
    }
    match mover.move_to(hwnd, target) {
        Ok(()) => log::info!("moved \"{label}\" to {name}"),
        Err(err) => log::warn!("could not move \"{label}\" to {name}: {err}"),
    }
}
