use std::path::Path;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, ShowWindow, SW_RESTORE, SW_SHOWNORMAL,
};

use crate::core::{host, wide};
use crate::search::Action;

/// Runs a row's action on the host thread. It is the thread the palette's
/// hotkey was delivered to, which is what lets it launch a program into the
/// foreground and bring another window forward; COM is also set up there,
/// which ShellExecute needs for shortcuts and shell extensions.
pub fn perform(action: &Action) {
    match action {
        Action::Open(path) => shell_open(path),
        Action::Activate(hwnd) => activate(*hwnd as HWND),
        // The palette handles these itself before anything reaches the host.
        Action::Command(_) | Action::OpenPlugin(_) => {}
    }
}

fn shell_open(path: &Path) {
    let file = wide(&path.to_string_lossy());
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as any value above 32.
    if result as usize <= 32 {
        log::warn!(
            "could not open {} (ShellExecute {})",
            path.display(),
            result as usize
        );
    }
}

fn activate(hwnd: HWND) {
    if unsafe { IsIconic(hwnd) } != 0 {
        unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    host::bring_to_front(hwnd);
}
