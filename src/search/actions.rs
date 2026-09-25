use std::path::Path;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, ShowWindow, SW_RESTORE, SW_SHOWNORMAL,
};

use crate::core::{clipboard, host, wide};
use crate::search::Action;

/// Runs a row's action on the host thread. It is the thread the palette's
/// hotkey was delivered to, which is what lets it launch a program into the
/// foreground and bring another window forward; COM is also set up there,
/// which ShellExecute needs for shortcuts and shell extensions. `owner` is
/// the host window, which the clipboard needs as its owner.
pub fn perform(action: &Action, owner: HWND) {
    match action {
        Action::Open(path) => shell_execute("open", &path.to_string_lossy(), None),
        Action::Reveal(path) => reveal(path),
        Action::Activate(hwnd) => activate(*hwnd as HWND),
        Action::Copy(text) => {
            if !clipboard::put_text(owner, text) {
                log::warn!("could not put the answer on the clipboard");
            }
        }
        Action::OpenUrl(url) => {
            if url.starts_with("https://") || url.starts_with("http://") {
                shell_execute("open", url, None);
            } else {
                log::warn!("refused to open {url:?}: not an http address");
            }
        }
        // The palette handles these itself before anything reaches the host.
        Action::Command(_)
        | Action::OpenPlugin(_)
        | Action::SetPrefix(_)
        | Action::Provider { .. } => {}
    }
}

fn shell_execute(verb: &str, file: &str, parameters: Option<&str>) {
    let parameters = parameters.map(wide);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide(verb).as_ptr(),
            wide(file).as_ptr(),
            parameters
                .as_ref()
                .map_or(std::ptr::null(), |text| text.as_ptr()),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as any value above 32.
    if result as usize <= 32 {
        log::warn!("could not open {file} (ShellExecute {})", result as usize);
    }
}

/// Explorer opens the parent folder with the item selected. A drive has no
/// parent, so it is simply opened.
fn reveal(path: &Path) {
    if path.parent().is_none() {
        shell_execute("open", &path.to_string_lossy(), None);
        return;
    }
    let target = path.to_string_lossy();
    let target = target.trim_end_matches('\\');
    shell_execute(
        "open",
        "explorer.exe",
        Some(&format!("/select,\"{target}\"")),
    );
}

fn activate(hwnd: HWND) {
    if unsafe { IsIconic(hwnd) } != 0 {
        unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    host::bring_to_front(hwnd);
}
