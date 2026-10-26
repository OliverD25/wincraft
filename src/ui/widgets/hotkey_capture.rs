//! Recording a hotkey: a low-level keyboard hook that takes the keys before
//! Windows acts on them, and a guard on the recording window that stops
//! Windows from acting on a key the hook missed.
//!
//! Three things in here look removable and are not:
//!
//! - The mask key. When the hook hides a key while Alt or Win is held,
//!   Windows still sees the modifier go down and up with nothing in between.
//!   A lone Win tap opens the Start menu, and a lone Alt tap opens the menu of
//!   the window in front. So the hook sends an unassigned key
//!   (`core::mask_key`) in place of the hidden one, the same trick AutoHotkey
//!   and PowerToys use.
//! - Hiding the release of a hidden key. Windows must never see half a key.
//! - The window guard. winit hands every `WM_SYSKEYDOWN` to `DefWindowProc`,
//!   which opens the system menu on Alt+Space and closes the window on
//!   Alt+F4. Windows silently drops a low-level hook whose thread is too slow
//!   to answer once, and nothing tells the program. The UI thread also paints
//!   every WinCraft window, so a slow frame is enough. Without the guard the
//!   box would keep saying "Press keys…" while every key went to the window.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VIRTUAL_KEY, VK_CONTROL,
    VK_ESCAPE, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
    VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetWindowThreadProcessId, SetWindowsHookExW,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, SC_KEYMENU, WH_KEYBOARD_LL, WM_CHAR, WM_DEADCHAR,
    WM_KEYDOWN, WM_NCDESTROY, WM_SYSCHAR, WM_SYSCOMMAND, WM_SYSDEADCHAR, WM_SYSKEYDOWN,
};

use crate::core::mask_key::{self, VK as MASK_VK};
use crate::core::traits::Hotkey;

const GUARD_ID: usize = 0x5743_4B47;

#[derive(Default)]
struct CaptureState {
    hook: isize,
    /// The window the guard is on, or 0.
    window: isize,
    modifiers: u32,
    live: u32,
    /// Keys whose press was hidden, so their release is hidden too.
    hidden: Vec<u32>,
    result: Option<Hotkey>,
    cancelled: bool,
}

thread_local! {
    static CAPTURE: RefCell<CaptureState> = RefCell::new(CaptureState::default());
}

/// What happens to one key event while a capture runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Windows gets the event as usual.
    Pass,
    /// Windows does not get the event: the release of a hidden key.
    Hide,
    /// Hidden and kept as the result. `mask` is true when a mask key must
    /// follow, because Alt or Win is held.
    Record { hotkey: Hotkey, mask: bool },
    /// Esc alone: hidden, and the capture ends.
    Cancel,
}

fn step(vk: u32, down: bool, modifiers: u32, press_was_hidden: bool) -> Step {
    if vk == MASK_VK || modifier_bit(vk).is_some() {
        return Step::Pass;
    }
    if !down {
        return if press_was_hidden {
            Step::Hide
        } else {
            Step::Pass
        };
    }
    if vk == u32::from(VK_ESCAPE) && modifiers == 0 {
        return Step::Cancel;
    }
    Step::Record {
        hotkey: Hotkey {
            modifiers: modifiers | MOD_NOREPEAT,
            vk,
        },
        mask: modifiers & (MOD_ALT | MOD_WIN) != 0,
    }
}

impl CaptureState {
    /// Keeps what `step` decided and says whether the event is hidden.
    fn apply(&mut self, vk: u32, step: Step) -> bool {
        match step {
            Step::Pass => false,
            Step::Hide => {
                self.hidden.retain(|key| *key != vk);
                true
            }
            Step::Record { hotkey, .. } => {
                self.result = Some(hotkey);
                self.hide_release(vk);
                true
            }
            Step::Cancel => {
                self.cancelled = true;
                self.hide_release(vk);
                true
            }
        }
    }

    fn hide_release(&mut self, vk: u32) {
        if !self.hidden.contains(&vk) {
            self.hidden.push(vk);
        }
    }
}

/// What the guard does with one message to the recording window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Guard {
    Pass,
    /// The window procedure never sees it.
    Drop,
    /// A key press the hook should have hidden: the hook is gone.
    Missed(u32),
}

fn guard(msg: u32, wparam: WPARAM) -> Guard {
    match msg {
        // DefWindowProc turns Alt+Space's character into the system menu.
        WM_CHAR | WM_SYSCHAR | WM_DEADCHAR | WM_SYSDEADCHAR => Guard::Drop,
        WM_SYSCOMMAND if (wparam as u32 & 0xFFF0) == SC_KEYMENU => Guard::Drop,
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wparam as u32;
            if vk == MASK_VK || modifier_bit(vk).is_some() {
                Guard::Pass
            } else {
                Guard::Missed(vk)
            }
        }
        _ => Guard::Pass,
    }
}

/// Windows gives the shell every Win+<key> before the focused window sees it,
/// so a normal key event would never show `Win+E`. The hook also hides the
/// key it captured, which is what stops the shortcut from firing while the
/// user is only trying to name it. The guard goes on the window in front when
/// it belongs to this thread: the window where the user clicked Change.
pub fn start() {
    CAPTURE.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.hook != 0 {
            return;
        }
        let hook = install_hook();
        if hook == 0 {
            log::warn!("could not install the keyboard hook for the capture box");
            return;
        }
        *state = CaptureState {
            hook,
            window: guard_window(),
            ..Default::default()
        };
    });
}

pub fn stop() {
    CAPTURE.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.hook != 0 {
            unsafe { UnhookWindowsHookEx(state.hook as *mut core::ffi::c_void) };
        }
        if state.window != 0 {
            unsafe { RemoveWindowSubclass(state.window as HWND, Some(guard_proc), GUARD_ID) };
        }
        *state = CaptureState::default();
    });
}

/// The modifiers held right now, so the box can show `Win+Alt+…` while the user
/// is still reaching for the last key.
pub fn live_modifiers() -> u32 {
    CAPTURE.with(|cell| cell.borrow().live)
}

pub fn take_result() -> Option<Hotkey> {
    CAPTURE.with(|cell| cell.borrow_mut().result.take())
}

pub fn take_cancelled() -> bool {
    CAPTURE.with(|cell| std::mem::take(&mut cell.borrow_mut().cancelled))
}

fn install_hook() -> isize {
    unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        ) as isize
    }
}

fn guard_window() -> isize {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null()
        || unsafe { GetWindowThreadProcessId(window, std::ptr::null_mut()) }
            != unsafe { GetCurrentThreadId() }
    {
        log::debug!("capture box: the window in front is not ours, so it has no guard");
        return 0;
    }
    if unsafe { SetWindowSubclass(window, Some(guard_proc), GUARD_ID, 0) } == 0 {
        log::warn!("could not guard the capture window; the keyboard hook still works");
        return 0;
    }
    window as isize
}

fn modifier_bit(vk: u32) -> Option<u32> {
    match vk as u16 {
        v if v == VK_SHIFT || v == VK_LSHIFT || v == VK_RSHIFT => Some(MOD_SHIFT),
        v if v == VK_MENU || v == VK_LMENU || v == VK_RMENU => Some(MOD_ALT),
        v if v == VK_LWIN || v == VK_RWIN => Some(MOD_WIN),
        v if v == VK_RCONTROL => Some(MOD_CONTROL),
        0x11 | 0xA2 => Some(MOD_CONTROL),
        _ => None,
    }
}

/// The modifiers Windows thinks are held for the message being handled.
fn held_for_message() -> u32 {
    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(vk)) } < 0;
    let mut modifiers = 0;
    if down(VK_SHIFT) {
        modifiers |= MOD_SHIFT;
    }
    if down(VK_CONTROL) {
        modifiers |= MOD_CONTROL;
    }
    if down(VK_MENU) {
        modifiers |= MOD_ALT;
    }
    if down(VK_LWIN) || down(VK_RWIN) {
        modifiers |= MOD_WIN;
    }
    modifiers
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let event = wparam as u32;
    let down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;
    let vk = info.vkCode;

    let (hide, mask) = CAPTURE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return (false, false);
        };
        if state.hook == 0 {
            return (false, false);
        }
        if let Some(bit) = modifier_bit(vk) {
            if down {
                state.modifiers |= bit;
            } else {
                state.modifiers &= !bit;
            }
            state.live = state.modifiers;
        }
        let press_was_hidden = state.hidden.contains(&vk);
        let step = step(vk, down, state.modifiers, press_was_hidden);
        let mask = matches!(step, Step::Record { mask: true, .. });
        (state.apply(vk, step), mask)
    });

    // Sent after the borrow ends: the mask comes back through this hook.
    if mask {
        mask_key::send("capture box");
    }
    if hide {
        return 1;
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Takes a key press the hook missed as if the hook had seen it, and puts a
/// new hook in place of the one Windows dropped.
fn take_missed(vk: u32) -> bool {
    CAPTURE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return false;
        };
        if state.hook == 0 {
            return false;
        }
        log::warn!(
            "capture box: the keyboard hook missed a key, so Windows probably dropped it; \
             the window took the key and the hook is installed again"
        );
        unsafe { UnhookWindowsHookEx(state.hook as *mut core::ffi::c_void) };
        let hook = install_hook();
        state.hook = hook;
        state.modifiers = held_for_message();
        state.live = state.modifiers;
        let step = step(vk, true, state.modifiers, false);
        let hidden = state.apply(vk, step);
        if hook == 0 {
            log::warn!("could not install the keyboard hook for the capture box again");
        }
        hidden
    })
}

unsafe extern "system" fn guard_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match guard(msg, wparam) {
        Guard::Drop => return 0,
        Guard::Missed(vk) if take_missed(vk) => return 0,
        _ => {}
    }
    if msg == WM_NCDESTROY {
        unsafe { RemoveWindowSubclass(hwnd, Some(guard_proc), GUARD_ID) };
        CAPTURE.with(|cell| {
            if let Ok(mut state) = cell.try_borrow_mut() {
                if state.window == hwnd as isize {
                    state.window = 0;
                }
            }
        });
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_E, VK_F10, VK_F4, VK_SPACE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SC_CLOSE, WM_KEYUP, WM_SYSKEYUP};

    fn key(vk: u16) -> u32 {
        u32::from(vk)
    }

    fn recorded(modifiers: u32, vk: u16, mask: bool) -> Step {
        Step::Record {
            hotkey: Hotkey {
                modifiers: modifiers | MOD_NOREPEAT,
                vk: key(vk),
            },
            mask,
        }
    }

    #[test]
    fn alt_space_is_hidden_and_masked_so_windows_opens_no_menu() {
        assert_eq!(
            step(key(VK_SPACE), true, MOD_ALT, false),
            recorded(MOD_ALT, VK_SPACE, true)
        );
        assert_eq!(step(key(VK_SPACE), false, MOD_ALT, true), Step::Hide);
    }

    #[test]
    fn alt_or_win_combinations_are_masked() {
        assert_eq!(
            step(key(VK_E), true, MOD_WIN, false),
            recorded(MOD_WIN, VK_E, true)
        );
        assert_eq!(
            step(key(VK_F4), true, MOD_ALT, false),
            recorded(MOD_ALT, VK_F4, true)
        );
        let ctrl_alt = MOD_CONTROL | MOD_ALT;
        assert_eq!(
            step(key(VK_E), true, ctrl_alt, false),
            recorded(ctrl_alt, VK_E, true)
        );
    }

    #[test]
    fn combinations_without_alt_or_win_need_no_mask() {
        assert_eq!(
            step(key(VK_F10), true, 0, false),
            recorded(0, VK_F10, false)
        );
        let ctrl_shift = MOD_CONTROL | MOD_SHIFT;
        assert_eq!(
            step(key(VK_E), true, ctrl_shift, false),
            recorded(ctrl_shift, VK_E, false)
        );
    }

    #[test]
    fn esc_alone_cancels_and_esc_with_a_modifier_is_a_hotkey() {
        assert_eq!(step(key(VK_ESCAPE), true, 0, false), Step::Cancel);
        assert_eq!(
            step(key(VK_ESCAPE), true, MOD_SHIFT, false),
            recorded(MOD_SHIFT, VK_ESCAPE, false)
        );
    }

    #[test]
    fn modifiers_the_mask_key_and_unrelated_releases_pass() {
        for vk in [VK_MENU, VK_LMENU, VK_LWIN, VK_RWIN, VK_SHIFT, VK_CONTROL] {
            assert_eq!(step(key(vk), true, 0, false), Step::Pass);
            assert_eq!(step(key(vk), false, 0, false), Step::Pass);
        }
        assert_eq!(step(MASK_VK, true, MOD_WIN, false), Step::Pass);
        assert_eq!(step(MASK_VK, false, MOD_WIN, false), Step::Pass);
        assert_eq!(step(key(VK_E), false, 0, false), Step::Pass);
    }

    #[test]
    fn a_hidden_press_hides_its_release_once() {
        let mut state = CaptureState::default();
        let vk = key(VK_SPACE);
        assert!(state.apply(vk, step(vk, true, MOD_ALT, false)));
        assert_eq!(
            state.result,
            Some(Hotkey {
                modifiers: MOD_ALT | MOD_NOREPEAT,
                vk
            })
        );
        let release = step(vk, false, MOD_ALT, state.hidden.contains(&vk));
        assert!(state.apply(vk, release));
        assert!(state.hidden.is_empty());
        assert!(!state.apply(vk, step(vk, false, 0, state.hidden.contains(&vk))));
    }

    #[test]
    fn cancel_is_kept_and_hides_the_esc_release() {
        let mut state = CaptureState::default();
        let vk = key(VK_ESCAPE);
        assert!(state.apply(vk, step(vk, true, 0, false)));
        assert!(state.cancelled);
        assert_eq!(state.hidden, vec![vk]);
    }

    #[test]
    fn the_guard_drops_what_turns_into_the_system_menu() {
        assert_eq!(guard(WM_SYSCHAR, WPARAM::from(b' ')), Guard::Drop);
        assert_eq!(guard(WM_CHAR, WPARAM::from(b'a')), Guard::Drop);
        assert_eq!(guard(WM_SYSCOMMAND, SC_KEYMENU as WPARAM), Guard::Drop);
        assert_eq!(
            guard(WM_SYSCOMMAND, (SC_KEYMENU | 3) as WPARAM),
            Guard::Drop
        );
    }

    #[test]
    fn the_guard_lets_the_close_button_and_modifiers_through() {
        assert_eq!(guard(WM_SYSCOMMAND, SC_CLOSE as WPARAM), Guard::Pass);
        assert_eq!(guard(WM_SYSKEYDOWN, VK_MENU as WPARAM), Guard::Pass);
        assert_eq!(guard(WM_KEYDOWN, VK_LWIN as WPARAM), Guard::Pass);
        assert_eq!(guard(WM_SYSKEYDOWN, MASK_VK as WPARAM), Guard::Pass);
        assert_eq!(guard(WM_SYSKEYUP, VK_SPACE as WPARAM), Guard::Pass);
        assert_eq!(guard(WM_KEYUP, VK_E as WPARAM), Guard::Pass);
    }

    #[test]
    fn a_key_press_reaching_the_window_means_the_hook_missed_it() {
        assert_eq!(
            guard(WM_SYSKEYDOWN, VK_SPACE as WPARAM),
            Guard::Missed(key(VK_SPACE))
        );
        assert_eq!(
            guard(WM_SYSKEYDOWN, VK_F4 as WPARAM),
            Guard::Missed(key(VK_F4))
        );
        assert_eq!(
            guard(WM_KEYDOWN, VK_F10 as WPARAM),
            Guard::Missed(key(VK_F10))
        );
    }
}
