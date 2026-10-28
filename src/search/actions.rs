use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CONTROL, VK_V,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, IsWindow, PostMessageW, ShowWindow, SW_MINIMIZE, SW_RESTORE,
    SW_SHOWNORMAL, WM_CLOSE,
};

use crate::core::virtual_desktops::Pinner;
use crate::core::{clipboard, host, instance, monitors, wide};
use crate::search::Action;

/// The window in front just before the palette opened: where a paste goes.
static BEFORE_PALETTE: AtomicIsize = AtomicIsize::new(0);

/// Called as the palette opens, before it takes the foreground.
pub fn remember_front() {
    BEFORE_PALETTE.store(unsafe { GetForegroundWindow() } as isize, Ordering::Relaxed);
}

/// The window that was in front when the palette last opened, for a
/// command that acts on "the window you were in".
pub fn front_before_palette() -> HWND {
    BEFORE_PALETTE.load(Ordering::Relaxed) as HWND
}

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
        Action::Paste(text) => paste(owner, text),
        Action::Minimize(windows) => each_window("minimizing", windows, |hwnd| {
            unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
            Ok(())
        }),
        Action::Close(windows) => each_window("closing", windows, |hwnd| {
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            Ok(())
        }),
        Action::NextMonitor(windows) => each_window(
            "moving to the next monitor",
            windows,
            monitors::move_to_next,
        ),
        Action::AllDesktops { hwnd, on } => all_desktops(*hwnd as HWND, *on),
        // The palette and the host handle these before they reach here.
        Action::Command(_)
        | Action::OpenPlugin(_)
        | Action::SetPrefix(_)
        | Action::ChangeShortcut { .. }
        | Action::Arrange(_)
        | Action::Provider { .. } => {}
    }
}

/// True, after logging it, in a test instance, which never changes a real
/// window.
fn read_only(what: &str) -> bool {
    if instance::is_test() {
        log::info!("read-only test instance: {what} skipped");
        return true;
    }
    false
}

/// Runs `step` on every window that still exists, logging what failed.
fn each_window(what: &str, windows: &[isize], step: impl Fn(HWND) -> Result<(), String>) {
    if read_only(&format!("{what} {} windows", windows.len())) {
        return;
    }
    for hwnd in windows {
        let hwnd = *hwnd as HWND;
        if unsafe { IsWindow(hwnd) } == 0 {
            continue;
        }
        if let Err(err) = step(hwnd) {
            log::warn!("{what} a window failed: {err}");
        }
    }
    log::info!("{what} {} windows, as picked in the palette", windows.len());
}

fn all_desktops(hwnd: HWND, on: bool) {
    let what = if on {
        "showing a window on all desktops"
    } else {
        "showing a window on one desktop"
    };
    if read_only(what) {
        return;
    }
    crate::search::start_com();
    match Pinner::new().and_then(|pinner| pinner.set_pinned(hwnd, on)) {
        Ok(()) => log::info!("{what}, as picked in the palette"),
        Err(err) => log::warn!("{what} failed: {err}"),
    }
}

/// The answer goes on the clipboard, then Ctrl+V goes to the window that
/// was in front, as if the person had pressed it there.
fn paste(owner: HWND, text: &str) {
    let target = BEFORE_PALETTE.load(Ordering::Relaxed) as HWND;
    if !clipboard::put_text(owner, text) {
        log::warn!("could not put the answer on the clipboard");
        return;
    }
    if target.is_null() || unsafe { IsWindow(target) } == 0 {
        log::info!("no window to paste into; the answer is on the clipboard");
        return;
    }
    if read_only("pasting into the window in front") {
        return;
    }
    host::bring_to_front(target);
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != inputs.len() {
        log::warn!("Ctrl+V could not be sent; the answer is on the clipboard");
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
