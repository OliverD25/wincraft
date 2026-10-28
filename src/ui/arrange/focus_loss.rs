//! What the strip does when another window takes the keyboard from it.
//!
//! A real click into another app closes it, as before, but not at once:
//! the strip waits `WAIT` and looks again. While a Windows key is in use
//! nothing closes, because Win and a digit may follow and move the strip,
//! and Win let go alone closes it through the host. When the shell (the
//! taskbar, Start or search) took the keyboard right after a Windows key,
//! the strip takes it back once instead of closing. After Win and a digit,
//! Windows hands the keyboard back to the app that had it before the strip
//! opened as soon as Win is let go; the strip takes it back from any window
//! for a moment, so a second Win and digit still finds it open.
//!
//! The Windows key times come from WindowDeck's hook through
//! `ui_bridge::win_key_times`, so they are right even when the strip lost
//! the focus before it could look at the key itself.

use std::time::Duration;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LWIN, VK_RWIN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, GA_ROOT,
};

use crate::core::{ui_bridge, windows_list};

/// How long the strip waits after losing the keyboard before it closes.
pub const WAIT: Duration = Duration::from_millis(300);
/// Nothing closes this long after a Windows key was let go.
pub const AFTER_WIN: Duration = Duration::from_millis(300);
/// The shell taking the keyboard this soon after a Windows key was let go
/// took it because of that key.
pub const SHELL_AFTER_WIN: Duration = Duration::from_millis(1500);
/// A Windows key the hook saw go down longer ago than this, with no up
/// seen, no longer counts as held (the hook may have been taken out).
const HELD_AT_MOST: Duration = Duration::from_secs(10);

/// The Windows keys, as milliseconds ago.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WinKeys {
    /// Down now, by the key state.
    pub down_now: bool,
    pub since_down: Option<u64>,
    pub since_up: Option<u64>,
    /// The last press took a digit for WindowDeck.
    pub took_digit: bool,
}

impl WinKeys {
    /// Held now: down by the key state, or the hook saw it go down and
    /// not come up since.
    pub fn held(&self) -> bool {
        self.down_now
            || self.since_down.is_some_and(|down| {
                down < HELD_AT_MOST.as_millis() as u64 && self.since_up.is_none_or(|up| down < up)
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Stay open and look again after this long.
    Keep(Duration),
    /// Stay open and ask for the keyboard back.
    TakeBack,
    Close,
}

/// The rule. `lost_for` is how long the strip has been without the
/// keyboard; `shell_in_front` whether the window that has it is the
/// taskbar, Start or search; `took_back` whether the strip already asked
/// for the keyboard back during this loss.
pub fn verdict(lost_for: Duration, win: WinKeys, shell_in_front: bool, took_back: bool) -> Verdict {
    if win.held() {
        return Verdict::Keep(AFTER_WIN);
    }
    let since_up = win.since_up.map(Duration::from_millis);
    if let Some(left) = since_up.and_then(|up| AFTER_WIN.checked_sub(up)) {
        if !left.is_zero() {
            return Verdict::Keep(left);
        }
    }
    let just_after = since_up.is_some_and(|up| up < SHELL_AFTER_WIN);
    if (win.took_digit || shell_in_front) && !took_back && just_after {
        return Verdict::TakeBack;
    }
    match WAIT.checked_sub(lost_for) {
        Some(left) if !left.is_zero() => Verdict::Keep(left),
        _ => Verdict::Close,
    }
}

/// Whether a window of this class and program is part of the shell that a
/// Windows key brings up: the taskbars, Start and search.
pub fn is_shell(class: &str, exe: &str) -> bool {
    let program = exe
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(class, "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
        || matches!(
            program.as_str(),
            "startmenuexperiencehost.exe"
                | "searchhost.exe"
                | "searchapp.exe"
                | "shellexperiencehost.exe"
        )
}

/// The Windows keys now.
pub fn win_keys() -> WinKeys {
    let down = |vk: u16| (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000) != 0;
    let now = unsafe { GetTickCount64() };
    let (down_at, up_at) = ui_bridge::win_key_times();
    WinKeys {
        down_now: down(VK_LWIN) || down(VK_RWIN),
        since_down: down_at.map(|at| now.saturating_sub(at)),
        since_up: up_at.map(|at| now.saturating_sub(at)),
        took_digit: ui_bridge::win_took_digit(),
    }
}

/// The window that has the keyboard now: its class and its program's path.
pub fn front_window() -> (String, String) {
    let front = unsafe { GetForegroundWindow() };
    if front.is_null() {
        return (String::new(), String::new());
    }
    let root: HWND = unsafe { GetAncestor(front, GA_ROOT) };
    let window = if root.is_null() { front } else { root };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    (
        windows_list::class_name(window),
        windows_list::exe_path(pid),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOST: Duration = Duration::from_millis(0);

    fn idle() -> WinKeys {
        WinKeys {
            down_now: false,
            since_down: Some(60_000),
            since_up: Some(59_000),
            took_digit: false,
        }
    }

    #[test]
    fn a_click_into_another_app_closes_after_the_wait() {
        assert_eq!(verdict(LOST, idle(), false, false), Verdict::Keep(WAIT));
        assert_eq!(verdict(WAIT, idle(), false, false), Verdict::Close);
        let never = WinKeys::default();
        assert_eq!(verdict(WAIT * 2, never, false, false), Verdict::Close);
        // The taskbar clicked long after any Windows key closes it too.
        assert_eq!(verdict(WAIT, idle(), true, false), Verdict::Close);
    }

    #[test]
    fn nothing_closes_while_win_is_held_or_just_pressed() {
        let held = WinKeys {
            down_now: true,
            ..idle()
        };
        assert_eq!(
            verdict(WAIT * 5, held, false, false),
            Verdict::Keep(AFTER_WIN)
        );
        // The hook saw Win go down, before the key state shows it.
        let just_pressed = WinKeys {
            down_now: false,
            since_down: Some(5),
            since_up: Some(2000),
            took_digit: false,
        };
        assert!(just_pressed.held());
        assert_eq!(
            verdict(WAIT * 5, just_pressed, true, false),
            Verdict::Keep(AFTER_WIN)
        );
        // Down seen long ago with no up: the hook may be gone.
        let stale = WinKeys {
            down_now: false,
            since_down: Some(60_000),
            since_up: None,
            took_digit: false,
        };
        assert!(!stale.held());
    }

    #[test]
    fn a_delayed_close_is_called_off_when_win_comes() {
        // Lost the keyboard a moment ago; the close would come soon.
        let lost = Duration::from_millis(200);
        assert_eq!(
            verdict(lost, idle(), false, false),
            Verdict::Keep(WAIT - lost)
        );
        // Win goes down before it does (Win and a digit on the way).
        let win = WinKeys {
            down_now: true,
            since_down: Some(0),
            ..idle()
        };
        assert_eq!(verdict(WAIT, win, false, false), Verdict::Keep(AFTER_WIN));
        // Just let go: still a moment more.
        let let_go = WinKeys {
            down_now: false,
            since_down: Some(150),
            since_up: Some(100),
            took_digit: false,
        };
        assert_eq!(
            verdict(WAIT * 2, let_go, false, false),
            Verdict::Keep(AFTER_WIN - Duration::from_millis(100))
        );
    }

    #[test]
    fn the_shell_taking_the_keyboard_after_win_gives_it_back_once() {
        let after = WinKeys {
            down_now: false,
            since_down: Some(600),
            since_up: Some(400),
            took_digit: false,
        };
        assert_eq!(verdict(WAIT, after, true, false), Verdict::TakeBack);
        assert_eq!(
            verdict(WAIT, after, true, true),
            Verdict::Close,
            "only once"
        );
        assert_eq!(verdict(WAIT, after, false, false), Verdict::Close, "an app");
    }

    #[test]
    fn after_win_and_a_digit_any_window_gives_the_keyboard_back() {
        // Reported 2026-10-28: Win+1 opened WindowDeck; letting go of Win,
        // Windows gave the keyboard back to Chrome and WindowDeck closed, so
        // Win+2 opened a new one instead of moving it.
        let after = WinKeys {
            down_now: false,
            since_down: Some(600),
            since_up: Some(400),
            took_digit: true,
        };
        assert_eq!(verdict(WAIT, after, false, false), Verdict::TakeBack);
        // Once only: a second loss means the keyboard is wanted elsewhere,
        // and taking it back again and again fought with Windows (seen in
        // the 0.20.50 log, once a second).
        assert_eq!(verdict(WAIT, after, false, true), Verdict::Close);
        let later = WinKeys {
            since_down: Some(SHELL_AFTER_WIN.as_millis() as u64 + 200),
            since_up: Some(SHELL_AFTER_WIN.as_millis() as u64),
            ..after
        };
        assert_eq!(verdict(WAIT, later, false, false), Verdict::Close);
        let other_key = WinKeys {
            took_digit: false,
            ..after
        };
        assert_eq!(verdict(WAIT, other_key, false, false), Verdict::Close);
    }

    #[test]
    fn the_shell_is_the_taskbars_start_and_search() {
        assert!(is_shell("Shell_TrayWnd", r"C:\Windows\explorer.exe"));
        assert!(is_shell(
            "Shell_SecondaryTrayWnd",
            r"C:\Windows\explorer.exe"
        ));
        assert!(is_shell(
            "Windows.UI.Core.CoreWindow",
            r"C:\Windows\SystemApps\StartMenuExperienceHost.exe"
        ));
        assert!(is_shell(
            "Windows.UI.Core.CoreWindow",
            r"C:\x\SearchHost.exe"
        ));
        assert!(
            !is_shell("CabinetWClass", r"C:\Windows\explorer.exe"),
            "a folder"
        );
        assert!(!is_shell("Chrome_WidgetWin_1", r"C:\x\chrome.exe"));
        assert!(!is_shell("", ""));
    }
}
