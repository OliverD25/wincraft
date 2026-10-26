//! "Open WindowDeck with Win+1…9": a low-level keyboard hook, put in only
//! while the switch is on (Principle 5), that keeps Win and a top-row digit
//! from Windows and asks the host for WindowDeck on that taskbar button.
//!
//! Windows registers Win+number for itself, so no hotkey can have it; a
//! hook sees the key first. Only Win with a digit and no other modifier is
//! kept: Win+Shift, Win+Ctrl and Win+Alt with a number go on to Windows, and
//! so does every other key. A kept digit's release is kept too, so Windows
//! never sees half a key, and the mask key follows it, so letting go of Win
//! does not open the Start menu. A button whose app has no window is handed
//! back to Windows by the host (`core::mask_key::replay_win_digit`), with a
//! tag this hook lets through.
//!
//! The hook runs on a thread of its own and only decides and posts; the
//! work happens on the host thread when the host's message comes round.

use std::cell::RefCell;
use std::sync::{mpsc, Mutex, PoisonError};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::core::{chord, host, mask_key, taskbar_number};

/// What the hook does with one key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Windows and the app in front get it as usual.
    Pass,
    /// Kept from Windows: the release of a kept digit, or the key repeat of
    /// one still held.
    Hide,
    /// Kept, with the mask key after it, and WindowDeck asked for button
    /// `digit`. `again` is the same digit once more while Win stayed down.
    Take { digit: u8, again: bool },
}

/// What the hook remembers between events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    /// Digits whose press was kept and whose release is still to come.
    held: Vec<u32>,
    /// The digit taken last while Win has been down.
    last: Option<u8>,
}

impl Rule {
    /// One key event: `vk` going `down` or up, with `modifiers` held
    /// (`MOD_WIN`, `MOD_ALT`, `MOD_CONTROL`, `MOD_SHIFT`). `ours` is a key
    /// WinCraft sent itself.
    pub fn step(&mut self, vk: u32, down: bool, modifiers: u32, ours: bool) -> Step {
        if ours || vk == mask_key::VK {
            return Step::Pass;
        }
        if vk == u32::from(VK_LWIN) || vk == u32::from(VK_RWIN) {
            if !down {
                self.last = None;
            }
            return Step::Pass;
        }
        if chord::is_modifier(vk) {
            return Step::Pass;
        }
        if !down {
            return match self.held.iter().position(|held| *held == vk) {
                Some(at) => {
                    self.held.swap_remove(at);
                    Step::Hide
                }
                None => Step::Pass,
            };
        }
        if self.held.contains(&vk) {
            return Step::Hide;
        }
        let Some(digit) = taskbar_number::digit_of(vk) else {
            return Step::Pass;
        };
        if modifiers & (MOD_WIN | MOD_ALT | MOD_CONTROL | MOD_SHIFT) != MOD_WIN {
            return Step::Pass;
        }
        self.held.push(vk);
        let again = self.last == Some(digit);
        self.last = Some(digit);
        Step::Take { digit, again }
    }
}

/// The thread the hook runs on, while it is in.
struct Worker {
    thread_id: u32,
    thread: std::thread::JoinHandle<()>,
}

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);

thread_local! {
    /// Only the hook's own thread uses it.
    static RULE: RefCell<Rule> = RefCell::new(Rule::default());
}

fn worker() -> std::sync::MutexGuard<'static, Option<Worker>> {
    WORKER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts the hook in, on a thread of its own that does nothing else. Windows
/// waits for a low-level hook before any key goes on, and passes the key to
/// Windows when the hook is late; on the busy host thread a repeated
/// Win+number could reach Windows that way, and Windows also removes a hook
/// that is late too often. False when Windows refused it; Win+number then
/// stays with Windows.
pub fn start() -> bool {
    let mut worker = worker();
    if worker.is_some() {
        return true;
    }
    let (ready, answer) = mpsc::channel();
    let Ok(thread) = std::thread::Builder::new()
        .name("win-number hook".to_string())
        .spawn(move || run(ready))
    else {
        return false;
    };
    match answer.recv() {
        Ok(Some(thread_id)) => {
            *worker = Some(Worker { thread_id, thread });
            true
        }
        _ => {
            let _ = thread.join();
            false
        }
    }
}

/// The hook's thread: puts the hook in, says so, and runs the message loop
/// Windows calls the hook from until `stop` asks it to end.
fn run(ready: mpsc::Sender<Option<u32>>) {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // Makes the thread's message queue, so `stop` can always reach it.
    unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        let _ = ready.send(None);
        return;
    }
    let _ = ready.send(Some(unsafe { GetCurrentThreadId() }));
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe { DispatchMessageW(&msg) };
    }
    unsafe { UnhookWindowsHookEx(hook) };
}

/// Takes the hook out, if it is in, and ends its thread.
pub fn stop() {
    let Some(worker) = worker().take() else {
        return;
    };
    unsafe { PostThreadMessageW(worker.thread_id, WM_QUIT, 0, 0) };
    let _ = worker.thread.join();
}

pub fn is_running() -> bool {
    worker().is_some()
}

fn down(vk: u16) -> bool {
    (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000) != 0
}

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
    let is_down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;
    let is_up = event == WM_KEYUP || event == WM_SYSKEYUP;
    let step = if is_down || is_up {
        let ours = mask_key::is_ours(info.dwExtraInfo);
        RULE.with(|cell| match cell.try_borrow_mut() {
            Ok(mut rule) => rule.step(info.vkCode, is_down, modifiers_now(), ours),
            Err(_) => Step::Pass,
        })
    } else {
        Step::Pass
    };
    match step {
        Step::Pass => unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) },
        Step::Hide => 1,
        Step::Take { digit, again } => {
            // After the borrow: the mask comes back through this hook.
            mask_key::send("WindowDeck");
            host::post_taskbar_button(digit, again);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: u32 = 0x31;
    const THREE: u32 = 0x33;
    const WIN: u32 = 0x5B;

    #[test]
    fn win_and_a_digit_alone_is_kept_with_its_release() {
        let mut rule = Rule::default();
        assert_eq!(rule.step(WIN, true, MOD_WIN, false), Step::Pass);
        assert_eq!(
            rule.step(THREE, true, MOD_WIN, false),
            Step::Take {
                digit: 3,
                again: false
            }
        );
        assert_eq!(
            rule.step(THREE, true, MOD_WIN, false),
            Step::Hide,
            "key repeat"
        );
        assert_eq!(
            rule.step(THREE, false, MOD_WIN, false),
            Step::Hide,
            "its release"
        );
        assert_eq!(rule.step(WIN, false, 0, false), Step::Pass);
        let zero = rule.step(0x30, true, MOD_WIN, false);
        assert_eq!(
            zero,
            Step::Take {
                digit: 0,
                again: false
            },
            "0 is button 10"
        );
    }

    #[test]
    fn the_same_digit_while_win_stays_down_is_again() {
        let mut rule = Rule::default();
        let press = |rule: &mut Rule, vk| {
            let step = rule.step(vk, true, MOD_WIN, false);
            rule.step(vk, false, MOD_WIN, false);
            step
        };
        assert_eq!(
            press(&mut rule, ONE),
            Step::Take {
                digit: 1,
                again: false
            }
        );
        assert_eq!(
            press(&mut rule, ONE),
            Step::Take {
                digit: 1,
                again: true
            }
        );
        assert_eq!(
            press(&mut rule, THREE),
            Step::Take {
                digit: 3,
                again: false
            }
        );
        rule.step(WIN, false, 0, false);
        assert_eq!(
            press(&mut rule, THREE),
            Step::Take {
                digit: 3,
                again: false
            },
            "Win was let go in between"
        );
    }

    #[test]
    fn win_with_shift_ctrl_or_alt_and_a_number_stays_with_windows() {
        let mut rule = Rule::default();
        for modifiers in [
            MOD_WIN | MOD_SHIFT,
            MOD_WIN | MOD_CONTROL,
            MOD_WIN | MOD_ALT,
            MOD_ALT,
            0,
        ] {
            assert_eq!(
                rule.step(ONE, true, modifiers, false),
                Step::Pass,
                "{modifiers:#x}"
            );
            assert_eq!(rule.step(ONE, false, modifiers, false), Step::Pass);
        }
        // Other keys with Win, the keypad and the modifiers themselves pass.
        assert_eq!(rule.step(0x45, true, MOD_WIN, false), Step::Pass, "Win+E");
        assert_eq!(
            rule.step(0x61, true, MOD_WIN, false),
            Step::Pass,
            "keypad 1"
        );
        assert_eq!(rule.step(0xA0, true, MOD_WIN, false), Step::Pass, "Shift");
    }

    #[test]
    fn keys_wincraft_sent_itself_pass() {
        let mut rule = Rule::default();
        // The replay of Win+number for a pinned app with no windows, and
        // the mask key.
        assert_eq!(rule.step(ONE, true, MOD_WIN, true), Step::Pass);
        assert_eq!(rule.step(ONE, false, MOD_WIN, true), Step::Pass);
        assert_eq!(rule.step(mask_key::VK, true, MOD_WIN, false), Step::Pass);
        assert_eq!(rule.step(mask_key::VK, false, MOD_WIN, false), Step::Pass);
    }

    #[test]
    fn with_the_switch_off_there_is_no_hook() {
        stop();
        assert!(!is_running());
    }
}
