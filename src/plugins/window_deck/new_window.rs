//! Ctrl+N in WindowDeck: a new window of the app, as Shift+click on its
//! taskbar button gives. The app is started again from its Start menu entry
//! (its AppUserModelID in `shell:AppsFolder`) when it has one, else from
//! its program file with no arguments. The first window of the app that
//! was not there before, within `WAIT`, comes to the front. Some apps only
//! bring an old window forward; then nothing more happens. No other window
//! is moved or changed.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, SW_SHOWNORMAL};

use crate::core::{appid, live_windows, wide, window_actions, windows_list};

/// How long a new window may take to come.
pub const WAIT: Duration = Duration::from_secs(5);
/// How often the windows are looked at while waiting.
pub const LOOK_MS: u32 = 200;

/// What to start for a new window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// An app the Start menu knows by this AppUserModelID.
    App(String),
    /// A program file.
    Program(String),
}

impl Launch {
    fn file(&self) -> String {
        match self {
            Launch::App(id) => format!("shell:AppsFolder\\{id}"),
            Launch::Program(path) => path.clone(),
        }
    }
}

/// The app's Start menu entry wins over its program file: a packaged app
/// cannot be started from its file, and Chrome's ID names the profile.
/// `app_id` is only an ID the Start menu knows.
pub fn target(app_id: Option<&str>, exe_path: &str) -> Option<Launch> {
    match app_id {
        Some(id) if !id.is_empty() => Some(Launch::App(id.to_string())),
        _ => (!exe_path.is_empty()).then(|| Launch::Program(exe_path.to_string())),
    }
}

/// The first window in `now` that was not in `before`.
pub fn new_one(before: &[isize], now: &[isize]) -> Option<isize> {
    now.iter().copied().find(|hwnd| !before.contains(hwnd))
}

/// A new window that is awaited.
#[derive(Debug, Clone)]
pub struct Wait {
    group: String,
    before: Vec<isize>,
    until: Instant,
}

/// What a look at the windows found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Found(isize),
    Waiting,
    TooLate,
}

impl Wait {
    pub fn look(&self, now: &[isize], at: Instant) -> Look {
        match new_one(&self.before, now) {
            Some(hwnd) => Look::Found(hwnd),
            None if at >= self.until => Look::TooLate,
            None => Look::Waiting,
        }
    }

    pub fn group(&self) -> &str {
        &self.group
    }
}

/// The windows of taskbar group `group` now.
pub fn windows_of(group: &str) -> Vec<isize> {
    live_windows::enumerate(&|_| true, &|_| true)
        .into_iter()
        .filter(|window| window.group == group)
        .map(|window| window.hwnd as isize)
        .collect()
}

/// Starts the app of `hwnd`, whose taskbar group is `group`, again. None
/// when it has nothing to start from or Windows refused.
pub fn start(group: &str, hwnd: isize) -> Option<Wait> {
    let before = windows_of(group);
    let app_id = appid::read(hwnd as HWND)
        .id
        .filter(|id| appid::registered_name(id).is_some());
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd as HWND, &mut pid) };
    let exe = if pid == 0 {
        String::new()
    } else {
        windows_list::exe_path(pid)
    };
    let Some(launch) = target(app_id.as_deref(), &exe) else {
        log::info!("Ctrl+N: {group} has nothing to start a new window from");
        return None;
    };
    let file = wide(&launch.file());
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        log::info!("Ctrl+N: Windows could not start {launch:?} (code {result})");
        return None;
    }
    log::info!("Ctrl+N: started {launch:?} for a new window of {group}");
    Some(Wait {
        group: group.to_string(),
        before,
        until: Instant::now() + WAIT,
    })
}

/// The new window came: to the front with it.
pub fn bring(hwnd: isize) {
    window_actions::activate(hwnd as HWND);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_menu_entry_wins_over_the_program_file() {
        assert_eq!(
            target(Some("Claude_pzs8sxrjxfjjc!Claude"), "C:\\Apps\\claude.exe"),
            Some(Launch::App("Claude_pzs8sxrjxfjjc!Claude".to_string()))
        );
        assert_eq!(
            target(None, "C:\\Windows\\notepad.exe"),
            Some(Launch::Program("C:\\Windows\\notepad.exe".to_string()))
        );
        assert_eq!(
            target(Some(""), "C:\\Windows\\notepad.exe"),
            Some(Launch::Program("C:\\Windows\\notepad.exe".to_string()))
        );
        assert_eq!(target(None, ""), None, "nothing to start from");
        assert_eq!(
            Launch::App("A!b".to_string()).file(),
            "shell:AppsFolder\\A!b"
        );
    }

    #[test]
    fn only_a_window_not_seen_before_is_the_new_one() {
        assert_eq!(new_one(&[1, 2], &[2, 1]), None);
        assert_eq!(new_one(&[1, 2], &[2, 7, 1]), Some(7));
        assert_eq!(new_one(&[], &[5]), Some(5));
        assert_eq!(new_one(&[1], &[]), None, "the old window closed");
    }

    #[test]
    fn the_wait_ends_with_the_new_window_or_after_its_time() {
        let start = Instant::now();
        let wait = Wait {
            group: "app".to_string(),
            before: vec![1],
            until: start + WAIT,
        };
        assert_eq!(wait.look(&[1], start), Look::Waiting);
        assert_eq!(wait.look(&[1, 4], start), Look::Found(4));
        assert_eq!(wait.look(&[1], start + WAIT), Look::TooLate);
        assert_eq!(wait.group(), "app");
    }
}
