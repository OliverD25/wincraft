//! The running side of `wincraft.exe --waiting` / `--answered`: a Claude
//! Code session waits for the user, or was answered. Runs without the Host
//! borrow, like the shell hook, because the marks live outside Host.

use super::*;
use crate::core::attention;
use crate::core::claude_waits::{self, Ask, Kind};
use crate::core::windows_list;
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::UI::WindowsAndMessaging::{FlashWindowEx, FLASHWINFO, FLASHW_TRAY};

/// How often WinCraft flashes a waiting window's taskbar button. After the
/// last flash the button stays orange until the window is activated.
const FLASH_COUNT: u32 = 3;

/// Takes one `WM_COPYDATA`. Returns 0 for a copy that is not ours, else
/// `claude_waits::reply_for` the number of windows it touched.
pub(super) fn copy_data(lparam: LPARAM) -> LRESULT {
    let Some(data) = (unsafe { (lparam as *const COPYDATASTRUCT).as_ref() }) else {
        return 0;
    };
    let bytes = if data.lpData.is_null() || data.cbData == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize) }
    };
    let Some((kind, ask, title)) = claude_waits::decode_ask(data.dwData, bytes) else {
        return 0;
    };
    let matched = session_windows(&title);
    let touched = match kind {
        Kind::Waiting => mark_waiting(&title, &matched, ask),
        Kind::Answered => clear_answered(&title, &matched),
    };
    claude_waits::reply_for(touched)
}

/// The visible Claude windows whose app title is `title`, or the one named
/// by `hwnd:<number>`.
fn session_windows(title: &str) -> Vec<isize> {
    windows_list::app_window_records(&|_| true)
        .into_iter()
        .filter(|(hwnd, record)| {
            if let Some(wanted) = claude_waits::window_handle(title) {
                let mut pid = 0u32;
                unsafe { GetWindowThreadProcessId(*hwnd, &mut pid) };
                return *hwnd as isize == wanted
                    && claude_waits::is_claude_app(&windows_list::exe_path(pid));
            }
            if record.title.trim() != title {
                return false;
            }
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(*hwnd, &mut pid) };
            claude_waits::shows_session(&windows_list::exe_path(pid), &record.title, title)
        })
        .map(|(hwnd, _)| hwnd as isize)
        .collect()
}

fn mark_waiting(title: &str, windows: &[isize], ask: Ask) -> usize {
    let added = ATTENTION.with(|attention| attention.borrow_mut().wait_as(windows, title, ask));
    let foreground = unsafe { GetForegroundWindow() } as isize;
    let flashed = if instance::is_test() {
        0
    } else {
        windows
            .iter()
            .filter(|hwnd| **hwnd != foreground)
            .map(|hwnd| flash(*hwnd))
            .count()
    };
    log::info!(
        "attention: Claude session \"{title}\" waits: {} windows matched, {added} newly marked, {flashed} taskbar buttons flashed",
        windows.len()
    );
    if added > 0 {
        refresh_arrange();
    }
    windows.len()
}

fn clear_answered(title: &str, windows: &[isize]) -> usize {
    let cleared = ATTENTION.with(|attention| attention.borrow_mut().answered(title, windows));
    let stopped = attention::stop_flashing(&cleared, instance::is_test());
    log::info!(
        "attention: Claude session \"{title}\" answered: {} windows matched, {} marks cleared, {stopped} taskbar buttons told to stop",
        windows.len(),
        cleared.len()
    );
    if !cleared.is_empty() {
        refresh_arrange();
    }
    cleared.len()
}

fn flash(hwnd: isize) {
    let info = FLASHWINFO {
        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
        hwnd: hwnd as HWND,
        dwFlags: FLASHW_TRAY,
        uCount: FLASH_COUNT,
        dwTimeout: 0,
    };
    unsafe { FlashWindowEx(&info) };
}
