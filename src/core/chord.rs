//! Catches the key pressed right after a hotkey. Windows only knows hotkeys
//! of one step, so a second step, such as Win+Alt+A and then a digit, needs a
//! look at the keyboard for a short time: a low-level keyboard hook that is
//! put in when the wait starts and taken out when it ends (Principle 5:
//! nothing runs in the background that the user did not switch on).
//!
//! The hook runs on a thread of its own (`core::ll_hook`): Windows skips,
//! and in the end removes, a low-level hook that answers slowly, and the
//! thread that starts a wait can be busy. The hook only asks a rule about
//! the key and, for a key it keeps, posts a message to the owner window;
//! the work happens when that message comes round.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KillTimer, PostMessageW, SetTimer, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
};

use crate::core::ll_hook::HookThread;

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
    /// Left to Windows, and the wait ends: the caller hears of it.
    End,
}

/// What came since the caller last looked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct News {
    /// The keys taken, oldest first.
    pub taken: Vec<Stroke>,
    /// A key the rule ends the wait on, or a mouse click, came: the caller
    /// stops the wait.
    pub ended: bool,
}

/// Whether a low-level mouse message is a button going down, which ends a
/// wait the way Esc does.
pub fn is_click(message: u32) -> bool {
    matches!(
        message,
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
    )
}

/// Decides about each key press while a wait runs. It is called on the
/// hook's own thread, so it carries what it needs to know.
pub type Rule = Box<dyn Fn(Stroke) -> Verdict + Send>;

/// What the hook does with `stroke` at `now`, for a wait that started at
/// `started` and lasts `wait`: after the wait every key passes.
pub fn verdict(
    stroke: Stroke,
    started: Instant,
    wait: Duration,
    now: Instant,
    rule: impl Fn(Stroke) -> Verdict,
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

/// What the hook does with one key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Pass,
    /// Kept from Windows: the release of a key that was taken.
    Hide,
    /// Kept for the owner, who hears of it.
    Take,
    /// Left to Windows, and the owner hears that the wait is over.
    End,
}

struct Wait {
    owner: isize,
    message: u32,
    started: Instant,
    wait: Duration,
    rule: Rule,
    news: News,
    /// Keys whose press was kept: their release is kept too.
    held: Vec<u32>,
}

impl Wait {
    fn new(owner: isize, message: u32, wait: Duration, rule: Rule) -> Self {
        Wait {
            owner,
            message,
            started: Instant::now(),
            wait,
            rule,
            news: News::default(),
            held: Vec::new(),
        }
    }

    /// One key going `down` or up, with `modifiers` held, at `now`.
    fn key(&mut self, vk: u32, down: bool, modifiers: u32, now: Instant) -> Step {
        if !down {
            return match self.held.iter().position(|held| *held == vk) {
                Some(at) => {
                    self.held.swap_remove(at);
                    Step::Hide
                }
                None => Step::Pass,
            };
        }
        if is_modifier(vk) {
            return Step::Pass;
        }
        let stroke = Stroke { vk, modifiers };
        match verdict(stroke, self.started, self.wait, now, &*self.rule) {
            Verdict::Take => {
                self.news.taken.push(stroke);
                if !self.held.contains(&vk) {
                    self.held.push(vk);
                }
                Step::Take
            }
            Verdict::Pass => Step::Pass,
            Verdict::End => self.end(),
        }
    }

    /// The wait is over; `End` the first time only, so the owner hears of
    /// it once.
    fn end(&mut self) -> Step {
        if self.news.ended {
            return Step::Pass;
        }
        self.news.ended = true;
        Step::End
    }

    fn tell_owner(&self) {
        unsafe { PostMessageW(self.owner as HWND, self.message, 0, 0) };
    }
}

/// The wait that runs, shared by the thread that started it and the hook's
/// own thread. Held only for a moment by either.
static WAIT: Mutex<Option<Wait>> = Mutex::new(None);
/// The hook's thread while a wait runs; see `core::ll_hook`.
static THREAD: Mutex<Option<HookThread>> = Mutex::new(None);

fn wait_slot() -> MutexGuard<'static, Option<Wait>> {
    WAIT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn thread_slot() -> MutexGuard<'static, Option<HookThread>> {
    THREAD.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts a wait of `wait`: each key press goes to `rule`, and `message` is
/// posted to `owner` for a key it takes and when the wait ends early
/// (`take_for` says which). A mouse click ends the wait too; the click
/// itself goes through. Starting again while a wait runs starts that wait
/// over. A wait of another owner ends, and that owner gets its message so it
/// can take its own hint off the screen. Returns false when Windows refused
/// the hook; keys then go to Windows as usual.
///
/// The hook runs on a thread of its own, so a busy caller never makes
/// Windows skip it; the caller's thread gets the timer that ends the wait,
/// and must run a message loop.
pub fn listen(
    owner: isize,
    message: u32,
    wait: Duration,
    rule: impl Fn(Stroke) -> Verdict + Send + 'static,
) -> bool {
    let before = wait_slot().as_ref().map(|wait| (wait.owner, wait.message));
    stop();
    if let Some((other, other_message)) = before.filter(|(other, _)| *other != owner) {
        unsafe { PostMessageW(other as HWND, other_message, 0, 0) };
    }
    *wait_slot() = Some(Wait::new(owner, message, wait, Box::new(rule)));
    // Without the mouse hook a click only fails to end the wait; the keys
    // still work.
    let thread = HookThread::start(
        "chord hook",
        vec![
            (WH_KEYBOARD_LL, Some(hook_proc)),
            (WH_MOUSE_LL, Some(mouse_proc)),
        ],
    );
    let Some(thread) = thread else {
        wait_slot().take();
        log::warn!("could not start waiting for the next key: the keyboard hook was refused");
        return false;
    };
    *thread_slot() = Some(thread);
    let ms = wait.as_millis().min(u32::MAX as u128) as u32;
    unsafe { SetTimer(owner as HWND, TIMER_ID, ms, None) };
    true
}

/// Ends the wait, if one runs, and takes the hook out.
fn stop() {
    let ended = wait_slot().take();
    // Neither lock is held here: the hook may be waiting for `WAIT`.
    let thread = thread_slot().take();
    if let Some(thread) = thread {
        thread.stop();
    }
    if let Some(wait) = ended {
        unsafe { KillTimer(wait.owner as HWND, TIMER_ID) };
    }
}

/// Ends `owner`'s wait, if it runs; a wait of another owner goes on.
pub fn stop_for(owner: isize) {
    if waiting_for(owner) {
        stop();
    }
}

/// `owner`'s `TIMER_ID` came round: its wait is over.
pub fn on_timer(owner: isize) {
    stop_for(owner);
}

/// Whether a wait of `owner` runs.
pub fn waiting_for(owner: isize) -> bool {
    wait_slot().as_ref().is_some_and(|wait| wait.owner == owner)
}

/// What came for `owner` since its last call. When another owner's wait
/// took over, or none runs, `owner`'s wait is over.
pub fn take_for(owner: isize) -> News {
    wait_slot()
        .as_mut()
        .filter(|wait| wait.owner == owner)
        .map(|wait| News {
            taken: std::mem::take(&mut wait.news.taken),
            ended: wait.news.ended,
        })
        .unwrap_or(News {
            taken: Vec::new(),
            ended: true,
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
    let began = Instant::now();
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let event = wparam as u32;
    let is_down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;
    let is_up = event == WM_KEYUP || event == WM_SYSKEYUP;
    let step = if is_down || is_up {
        let mut slot = wait_slot();
        match slot.as_mut() {
            Some(wait) => {
                let step = wait.key(info.vkCode, is_down, modifiers_now(), Instant::now());
                if matches!(step, Step::Take | Step::End) {
                    wait.tell_owner();
                }
                step
            }
            None => Step::Pass,
        }
    } else {
        Step::Pass
    };
    if crate::core::detailed::on() {
        crate::core::detailed::KeyEvent {
            hook: "chord",
            vk: info.vkCode,
            message: event,
            flags: info.flags,
            extra: info.dwExtraInfo,
            lag_ms: crate::core::detailed::lag_ms(info.time),
            took_ms: began.elapsed().as_secs_f64() * 1000.0,
        }
        .write(&step);
    }
    // The owner takes the hook out after `End`; doing it here, inside the
    // hook's own call, is not allowed.
    match step {
        Step::Take | Step::Hide => 1,
        Step::Pass | Step::End => unsafe {
            CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
        },
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && is_click(wparam as u32) {
        if let Some(wait) = wait_slot().as_mut() {
            if wait.end() == Step::End {
                wait.tell_owner();
            }
        }
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

    /// Digits are taken, Esc ends the wait, every other key passes.
    fn digits(stroke: Stroke) -> Verdict {
        match stroke.vk {
            0x30..=0x39 => Verdict::Take,
            0x1B => Verdict::End,
            _ => Verdict::Pass,
        }
    }

    #[test]
    fn a_taken_key_is_kept_with_its_release_and_others_pass() {
        let mut wait = Wait::new(1, 2, Duration::from_secs(3), Box::new(digits));
        let now = Instant::now();
        assert_eq!(wait.key(0x12, true, MOD_ALT, now), Step::Pass, "Alt");
        assert_eq!(wait.key(0x33, true, MOD_WIN | MOD_ALT, now), Step::Take);
        assert_eq!(
            wait.key(0x33, true, MOD_WIN | MOD_ALT, now),
            Step::Take,
            "repeat"
        );
        assert_eq!(wait.key(0x33, false, MOD_WIN | MOD_ALT, now), Step::Hide);
        assert_eq!(wait.key(0x33, false, 0, now), Step::Pass, "released once");
        assert_eq!(wait.key(0x41, true, 0, now), Step::Pass, "A");
        assert_eq!(wait.key(0x41, false, 0, now), Step::Pass);
        let news = std::mem::take(&mut wait.news);
        assert_eq!(news.taken.len(), 2);
        assert!(!news.ended);
    }

    #[test]
    fn the_end_is_told_once_and_late_keys_pass() {
        let mut wait = Wait::new(1, 2, Duration::from_secs(3), Box::new(digits));
        let now = Instant::now();
        assert_eq!(wait.key(0x1B, true, 0, now), Step::End, "Esc");
        assert_eq!(wait.key(0x1B, true, 0, now), Step::Pass, "told once");
        assert!(wait.news.ended);

        let mut late = Wait::new(1, 2, Duration::from_millis(10), Box::new(digits));
        let after = Instant::now() + Duration::from_millis(50);
        assert_eq!(late.key(0x31, true, 0, after), Step::End);
        assert_eq!(late.end(), Step::Pass, "a click after it");
    }

    #[test]
    fn with_no_wait_running_every_owner_hears_that_it_is_over() {
        assert!(!waiting_for(1));
        let over = News {
            taken: Vec::new(),
            ended: true,
        };
        assert_eq!(take_for(1), over);
        stop_for(1);
        on_timer(2);
        assert!(!waiting_for(1));
        assert!(!waiting_for(2));
    }

    /// Lets every key through, so the test keeps nothing from the user.
    fn pass_all(_: Stroke) -> Verdict {
        Verdict::Pass
    }

    #[test]
    fn a_second_listen_replaces_the_first_and_a_stop_ends_the_thread() {
        // Window handles no window has, so the timer and posts go nowhere.
        let (first, second) = (0x7fff_ff10, 0x7fff_ff20);
        if !listen(first, 0x8001, Duration::from_secs(3), pass_all) {
            return; // No keyboard hook on this machine: nothing to check.
        }
        assert!(waiting_for(first));
        assert!(listen(second, 0x8001, Duration::from_secs(3), pass_all));
        assert!(!waiting_for(first), "the first wait ended");
        assert!(take_for(first).ended);
        assert!(waiting_for(second));
        assert!(thread_slot().is_some());
        stop_for(second);
        assert!(!waiting_for(second));
        assert!(thread_slot().is_none(), "the hook's thread ended");
        stop_for(second);
        assert!(take_for(second).ended);
    }

    #[test]
    fn a_button_going_down_is_a_click_and_moving_is_not() {
        for message in [
            WM_LBUTTONDOWN,
            WM_RBUTTONDOWN,
            WM_MBUTTONDOWN,
            WM_XBUTTONDOWN,
        ] {
            assert!(is_click(message));
        }
        // WM_MOUSEMOVE, WM_LBUTTONUP and WM_MOUSEWHEEL.
        for message in [0x0200, 0x0202, 0x020A] {
            assert!(!is_click(message));
        }
    }
}
