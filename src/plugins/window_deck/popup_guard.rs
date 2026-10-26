//! While WindowDeck is open, Windows' taskbar thumbnail popup is hidden the
//! moment it appears, so it never covers the strip; for example when
//! Win+number comes again and Windows shows its popup on top.
//!
//! Nothing runs until the host asks WindowDeck how the strip looks, which it
//! does each time it opens the strip. Then two WinEvent hooks on Explorer and
//! a 100 ms timer run, and both stop once the strip has closed (or never came
//! within `WATCH_MS`) (Principle 5). Hiding is the hover strip's plain
//! `ShowWindowAsync(SW_HIDE)`, never a move. A test instance never hides
//! Windows' popup unless it was started with `--allow-hide-popup`.

use std::cell::{Cell, RefCell};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use crate::core::taskbar_popup::{self as popup, keys::WIN_RELEASE_MS, Method};
use crate::core::ui_bridge;

/// "WD" and 4, clear of the plugin's other timers.
pub const TIMER_ID: usize = 0x5744_0004;
const TICK_MS: u32 = 100;
/// A strip asked for that is not on screen after this long never came
/// (Win+Alt+A waits up to 3 s for a digit).
const WATCH_MS: u64 = 4000;

/// What the keyboard was doing and whether the strip is open, when a popup
/// appeared. Times are milliseconds ago.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inputs {
    pub strip_open: bool,
    /// False in a test instance without `--allow-hide-popup`.
    pub may_hide: bool,
    /// "Open WindowDeck with Win+1…9" is on.
    pub win_number: bool,
    pub win_down: bool,
    /// Since a Windows key was last seen down; None if not lately.
    pub since_win: Option<u64>,
    pub alt_down: bool,
    pub ctrl_down: bool,
    pub shift_down: bool,
}

/// Whether Windows' popup is hidden now. Only while the strip is open, and
/// never for a key combination that belongs to Windows: Alt (Alt+Tab,
/// Win+Alt+number), Win+Ctrl or Win+Shift with a number, and Win+number
/// itself while the Win+1…9 switch is off.
pub fn decide(inputs: &Inputs) -> bool {
    if !inputs.strip_open || !inputs.may_hide || inputs.alt_down {
        return false;
    }
    if inputs.win_down && (inputs.ctrl_down || inputs.shift_down) {
        return false;
    }
    let win_lately = inputs.win_down || inputs.since_win.is_some_and(|ms| ms <= WIN_RELEASE_MS);
    !(win_lately && !inputs.win_number)
}

/// What the guard knows, for the WinEvent callback, which may run while a
/// plugin call is on the stack and so never touches the plugin.
#[derive(Debug, Clone, Copy, Default)]
struct Settings {
    may_hide: bool,
    win_number: bool,
}

#[derive(Default)]
struct Watch {
    host: Option<HWND>,
    hooks: Vec<HWINEVENTHOOK>,
    since: u64,
    seen_open: bool,
    refusal_noted: bool,
}

thread_local! {
    static SETTINGS: Cell<Settings> = const { Cell::new(Settings { may_hide: false, win_number: false }) };
    static LAST_WIN: Cell<Option<u64>> = const { Cell::new(None) };
    static HIDDEN: Cell<u32> = const { Cell::new(0) };
    static WATCH: RefCell<Watch> = RefCell::new(Watch::default());
}

/// The plugin's host window and switches; `None` for teardown, which also
/// stops a running watch.
pub fn configure(host: Option<HWND>, may_hide: bool, win_number: bool) {
    SETTINGS.with(|cell| {
        cell.set(Settings {
            may_hide,
            win_number,
        })
    });
    if host.is_none() {
        stop();
    }
    WATCH.with(|cell| cell.borrow_mut().host = host);
}

/// The strip is opening or open: watch for Windows' popup until it closes.
pub fn watch() {
    let settings = SETTINGS.with(|cell| cell.get());
    WATCH.with(|cell| {
        let Ok(mut watch) = cell.try_borrow_mut() else {
            return;
        };
        let Some(host) = watch.host else {
            return;
        };
        if !settings.may_hide {
            if !watch.refusal_noted {
                watch.refusal_noted = true;
                log::info!(
                    "read-only test instance: hiding Windows' taskbar preview over WindowDeck skipped"
                );
            }
            return;
        }
        watch.since = now();
        if watch.hooks.is_empty() {
            watch.seen_open = false;
            watch.hooks = popup::hook_explorer(Some(on_event));
            unsafe { SetTimer(host, TIMER_ID, TICK_MS, None) };
        }
    });
}

/// The guard's timer came round.
pub fn tick() {
    let now = now();
    let keys = keys(now);
    let open = ui_bridge::arrange_window().is_some();
    let over = WATCH.with(|cell| {
        let Ok(mut watch) = cell.try_borrow_mut() else {
            return false;
        };
        if watch.hooks.is_empty() {
            return true;
        }
        if open {
            watch.seen_open = true;
        }
        (watch.seen_open && !open)
            || (!watch.seen_open && now.saturating_sub(watch.since) > WATCH_MS)
    });
    if over {
        stop();
        return;
    }
    // In case a WinEvent came late or not at all.
    let inputs = inputs(open, keys);
    if decide(&inputs) {
        for hwnd in popup::find_popups() {
            if popup::is_shown(hwnd) {
                hide(hwnd);
            }
        }
    }
}

/// Takes the hooks out and stops the timer.
pub fn stop() {
    WATCH.with(|cell| {
        let Ok(mut watch) = cell.try_borrow_mut() else {
            return;
        };
        if watch.hooks.is_empty() {
            return;
        }
        popup::unhook_explorer(&mut watch.hooks);
        if let Some(host) = watch.host {
            unsafe { KillTimer(host, TIMER_ID) };
        }
        let hidden = HIDDEN.with(|cell| cell.replace(0));
        if hidden > 0 {
            log::info!("WindowDeck closed; Windows' preview over it was hidden {hidden} times");
        }
        watch.seen_open = false;
    });
}

#[cfg(test)]
fn is_watching() -> bool {
    WATCH.with(|cell| cell.try_borrow().is_ok_and(|watch| !watch.hooks.is_empty()))
}

fn now() -> u64 {
    unsafe { GetTickCount64() }
}

#[derive(Clone, Copy)]
struct Keys {
    win: bool,
    since_win: Option<u64>,
    alt: bool,
    ctrl: bool,
    shift: bool,
}

fn keys(now: u64) -> Keys {
    let win = popup::key_down(VK_LWIN) || popup::key_down(VK_RWIN);
    if win {
        LAST_WIN.with(|cell| cell.set(Some(now)));
    }
    Keys {
        win,
        since_win: LAST_WIN
            .with(|cell| cell.get())
            .map(|at| now.saturating_sub(at)),
        alt: popup::key_down(VK_MENU),
        ctrl: popup::key_down(VK_CONTROL),
        shift: popup::key_down(VK_SHIFT),
    }
}

fn inputs(strip_open: bool, keys: Keys) -> Inputs {
    let settings = SETTINGS.with(|cell| cell.get());
    Inputs {
        strip_open,
        may_hide: settings.may_hide,
        win_number: settings.win_number,
        win_down: keys.win,
        since_win: keys.since_win,
        alt_down: keys.alt,
        ctrl_down: keys.ctrl,
        shift_down: keys.shift,
    }
}

fn hide(hwnd: HWND) {
    popup::hide(hwnd, Method::ShowWindowAsync);
    HIDDEN.with(|cell| cell.set(cell.get() + 1));
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if !popup::is_popup_event(hwnd, id_object, id_child) {
        return;
    }
    let open = ui_bridge::arrange_window().is_some();
    if decide(&inputs(open, keys(now()))) {
        hide(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Inputs {
        Inputs {
            strip_open: true,
            may_hide: true,
            win_number: true,
            ..Inputs::default()
        }
    }

    #[test]
    fn the_popup_is_hidden_only_while_the_strip_is_open() {
        assert!(decide(&open()));
        let closed = Inputs {
            strip_open: false,
            ..open()
        };
        assert!(!decide(&closed));
    }

    #[test]
    fn a_test_instance_hides_it_only_when_allowed() {
        // `may_hide` is false in a test instance without --allow-hide-popup.
        let test = Inputs {
            may_hide: false,
            ..open()
        };
        assert!(!decide(&test));
        let closed_test = Inputs {
            strip_open: false,
            ..test
        };
        assert!(!decide(&closed_test));
    }

    #[test]
    fn win_number_again_is_hidden_while_the_switch_is_on() {
        let again = Inputs {
            win_down: true,
            since_win: Some(0),
            ..open()
        };
        assert!(decide(&again));
        let just_released = Inputs {
            since_win: Some(100),
            ..open()
        };
        assert!(decide(&just_released));
    }

    #[test]
    fn win_number_stays_with_windows_while_the_switch_is_off() {
        let off = Inputs {
            win_number: false,
            ..open()
        };
        assert!(decide(&off), "a popup the mouse opened over the strip");
        let held = Inputs {
            win_down: true,
            since_win: Some(0),
            ..off
        };
        assert!(!decide(&held));
        let released = Inputs {
            since_win: Some(300),
            ..off
        };
        assert!(!decide(&released));
        let long_ago = Inputs {
            since_win: Some(5000),
            ..off
        };
        assert!(decide(&long_ago));
    }

    #[test]
    fn alt_and_win_with_shift_or_ctrl_stay_with_windows() {
        let alt = Inputs {
            alt_down: true,
            ..open()
        };
        assert!(!decide(&alt), "Alt+Tab");
        let win_alt = Inputs {
            win_down: true,
            ..alt
        };
        assert!(!decide(&win_alt), "Win+Alt+number");
        for (ctrl_down, shift_down) in [(true, false), (false, true), (true, true)] {
            let combo = Inputs {
                win_down: true,
                ctrl_down,
                shift_down,
                ..open()
            };
            assert!(!decide(&combo), "ctrl {ctrl_down} shift {shift_down}");
        }
        let shift_alone = Inputs {
            shift_down: true,
            ..open()
        };
        assert!(decide(&shift_alone), "Shift without Win is not Win+Shift");
    }

    #[test]
    fn nothing_is_watched_without_a_host_window() {
        configure(None, true, true);
        watch();
        assert!(!is_watching());
    }
}
