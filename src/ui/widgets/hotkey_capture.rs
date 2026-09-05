use std::cell::RefCell;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::core::hotkeys;
use crate::core::traits::Hotkey;

#[derive(Default)]
struct CaptureState {
    hook: isize,
    modifiers: u32,
    live: u32,
    result: Option<Hotkey>,
    cancelled: bool,
}

thread_local! {
    static CAPTURE: RefCell<CaptureState> = RefCell::new(CaptureState::default());
}

/// Windows gives the shell every Win+<key> before the focused window sees it,
/// so a normal key event would never show `Win+E`. The hook also swallows the
/// key it captured, which is what stops the shortcut from firing while the user
/// is only trying to name it.
pub fn start() {
    CAPTURE.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.hook != 0 {
            return;
        }
        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(hook_proc),
                GetModuleHandleW(std::ptr::null()),
                0,
            )
        };
        if hook.is_null() {
            log::warn!("could not install the keyboard hook for the capture box");
            return;
        }
        *state = CaptureState {
            hook: hook as isize,
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

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let event = wparam as u32;
    let down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;

    let swallow = CAPTURE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return false;
        };
        if state.hook == 0 {
            return false;
        }
        if let Some(bit) = modifier_bit(info.vkCode) {
            if down {
                state.modifiers |= bit;
            } else {
                state.modifiers &= !bit;
            }
            state.live = state.modifiers;
            return false;
        }
        if !down {
            return false;
        }
        if info.vkCode as u16 == 0x1B && state.modifiers == 0 {
            state.cancelled = true;
            return true;
        }
        state.result = Some(Hotkey {
            modifiers: state.modifiers | MOD_NOREPEAT,
            vk: info.vkCode,
        });
        true
    });

    if swallow {
        return 1;
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

pub fn describe(modifiers: u32) -> String {
    let text = hotkeys::format_modifiers(modifiers);
    if text.is_empty() {
        "Press a combination\u{2026}".to_string()
    } else {
        format!("{text}+\u{2026}")
    }
}
