//! Catches the key pressed right after a hotkey. Windows only knows hotkeys
//! of one step, so a second step, such as Win+Alt+A and then a digit, needs a
//! look at the keyboard for a short time: a low-level keyboard hook that is
//! put in when the wait starts and taken out when it ends (Principle 5:
//! nothing runs in the background that the user did not switch on).
//!
//! The hook runs on the thread that started the wait, which must run a
//! message loop. Windows removes a low-level hook that answers slowly, so the
//! hook only asks a rule about the key and, for a key it keeps, posts a
//! message; the work happens when that message comes round.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KillTimer, PostMessageW, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// The timer on the owner window that ends a wait. The owner's window
/// procedure passes it to `on_timer`.
pub const TIMER_ID: usize = 0x5743_0010;

/// One key press: the key and the modifiers (`MOD_WIN`, `MOD_ALT`,
/// `MOD_CONTROL`, `MOD_SHIFT`) held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub vk: u32,
    pub modifiers: u32,
}

/// What the hook does with a key while it waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Kept for the caller; the app in front never sees it.
    Take,
    /// Left to Windows and the app in front.
    Pass,
    /// Left to Windows, and the wait ends.
    End,
}

/// Decides about each key press while a wait runs.
pub type Rule = fn(Stroke) -> Verdict;

/// What the hook does with `stroke` at `now`, for a wait that started at
/// `started` and lasts `wait`: after the wait every key passes.
pub fn verdict(
    stroke: Stroke,
    started: Instant,
    wait: Duration,
    now: Instant,
    rule: Rule,
) -> Verdict {
    if now.saturating_duration_since(started) > wait {
        Verdict::End
    } else {
        rule(stroke)
    }
}

/// Whether a key is one of the modifiers; they pass on their own, since a
/// stroke is the key pressed while they are held.
pub fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

struct Wait {
    hook: isize,
    owner: isize,
    message: u32,
    started: Instant,
    wait: Duration,
    rule: Rule,
    taken: Vec<Stroke>,
    /// Keys whose press was kept: their release is kept too.
    held: Vec<u32>,
}

thread_local! {
    static WAIT: RefCell<Option<Wait>> = const { RefCell::new(None) };
}

/// Starts a wait of `wait` on this thread: each key press goes to `rule`,
/// and a key it takes is kept and `message` is posted to `owner`. Starting
/// again while a wait runs starts that wait over. Returns false when Windows
/// refused the hook; keys then go to Windows as usual.
pub fn listen(owner: isize, message: u32, wait: Duration, rule: Rule) -> bool {
    stop();
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        log::warn!("could not start waiting for the next key: the keyboard hook was refused");
        return false;
    }
    let ms = wait.as_millis().min(u32::MAX as u128) as u32;
    unsafe { SetTimer(owner as HWND, TIMER_ID, ms, None) };
    WAIT.with(|cell| {
        *cell.borrow_mut() = Some(Wait {
            hook: hook as isize,
            owner,
            message,
            started: Instant::now(),
            wait,
            rule,
            taken: Vec::new(),
            held: Vec::new(),
        })
    });
    true
}

/// Ends the wait, if one runs, and takes the hook out.
pub fn stop() {
    let ended = WAIT.with(|cell| cell.borrow_mut().take());
    if let Some(wait) = ended {
        unsafe {
            UnhookWindowsHookEx(wait.hook as *mut core::ffi::c_void);
            KillTimer(wait.owner as HWND, TIMER_ID);
        }
    }
}

/// The owner's `TIMER_ID` came round: the wait is over.
pub fn on_timer() {
    stop();
}

/// The keys taken since the last call, oldest first.
pub fn take() -> Vec<Stroke> {
    WAIT.with(|cell| {
        cell.borrow_mut()
            .as_mut()
            .map(|wait| std::mem::take(&mut wait.taken))
            .unwrap_or_default()
    })
}

fn down(vk: u16) -> bool {
    (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
}

/// The modifiers held right now.
fn modifiers_now() -> u32 {
    let mut modifiers = 0;
    if down(VK_LWIN) || down(VK_RWIN) {
        modifiers |= MOD_WIN;
    }
    if down(VK_MENU) {
        modifiers |= MOD_ALT;
    }
    if down(VK_CONTROL) {
        modifiers |= MOD_CONTROL;
    }
    if down(VK_SHIFT) {
        modifiers |= MOD_SHIFT;
    }
    modifiers
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let event = wparam as u32;
    let swallow = WAIT.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            return false;
        };
        let Some(wait) = slot.as_mut() else {
            return false;
        };
        if event == WM_KEYUP || event == WM_SYSKEYUP {
            let held = wait.held.iter().position(|vk| *vk == info.vkCode);
            return held.map(|at| wait.held.swap_remove(at)).is_some();
        }
        if !(event == WM_KEYDOWN || event == WM_SYSKEYDOWN) || is_modifier(info.vkCode) {
            return false;
        }
        let stroke = Stroke {
            vk: info.vkCode,
            modifiers: modifiers_now(),
        };
        match verdict(stroke, wait.started, wait.wait, Instant::now(), wait.rule) {
            Verdict::Take => {
                wait.taken.push(stroke);
                if !wait.held.contains(&stroke.vk) {
                    wait.held.push(stroke.vk);
                }
                unsafe { PostMessageW(wait.owner as HWND, wait.message, 0, 0) };
                true
            }
            // The timer takes the hook out; doing it here, inside the
            // hook's own call, is not allowed.
            Verdict::Pass | Verdict::End => false,
        }
    });
    if swallow {
        return 1;
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take_all(_: Stroke) -> Verdict {
        Verdict::Take
    }

    #[test]
    fn keys_pass_once_the_wait_is_over() {
        let start = Instant::now();
        let wait = Duration::from_millis(1500);
        let key = Stroke {
            vk: 0x31,
            modifiers: MOD_WIN | MOD_ALT,
        };
        let at = |ms| start + Duration::from_millis(ms);
        assert_eq!(verdict(key, start, wait, at(0), take_all), Verdict::Take);
        assert_eq!(verdict(key, start, wait, at(1500), take_all), Verdict::Take);
        assert_eq!(verdict(key, start, wait, at(1501), take_all), Verdict::End);
    }

    #[test]
    fn modifiers_are_not_strokes_of_their_own() {
        for vk in [0x10, 0x11, 0x12, 0x5B, 0x5C, 0xA0, 0xA5] {
            assert!(is_modifier(vk), "{vk:#x}");
        }
        for vk in [0x30, 0x41, 0x1B, 0x70] {
            assert!(!is_modifier(vk), "{vk:#x}");
        }
    }

    #[test]
    fn nothing_is_taken_until_a_wait_starts() {
        assert!(take().is_empty());
        stop();
        assert!(take().is_empty());
    }
}
