//! Finding, reading and hiding Explorer's taskbar hover popup, for
//! WindowDeck's hover strip.
//!
//! Only plain hides are used, never a move off-screen: Explorer shows a
//! hidden popup again on the next hover by itself, so a WinCraft that
//! crashes or quits can never leave the taskbar without its popup.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::UI::Accessibility::{
    SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK, WINEVENTPROC,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_DOWN, VK_ESCAPE, VK_LBUTTON, VK_LEFT, VK_LWIN, VK_MBUTTON, VK_MENU,
    VK_RBUTTON, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetWindowThreadProcessId, IsWindowVisible, ShowWindowAsync,
    CHILDID_SELF, EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED, OBJID_WINDOW, SW_HIDE,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

pub mod keys;

use crate::core::wide;
use crate::core::windows_list::class_name;
use keys::{KeyState, POPUP_CLASS};

/// Hides the popup. Explorer shows it again by itself on the next hover.
pub fn hide(hwnd: HWND) {
    unsafe { ShowWindowAsync(hwnd, SW_HIDE) };
}

pub fn find_popups() -> Vec<HWND> {
    // By class only: Explorer gives the window its title only while shown.
    let class = wide(POPUP_CLASS);
    let mut found = Vec::new();
    let mut after: HWND = std::ptr::null_mut();
    loop {
        let hwnd = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                after,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() || found.len() >= 16 {
            break;
        }
        found.push(hwnd);
        after = hwnd;
    }
    found
}

/// Whether the window is visible, and its DWM cloak flags.
pub fn visible_and_cloak(hwnd: HWND) -> (bool, u32) {
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    let mut cloak = 0u32;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloak as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    (visible, cloak)
}

pub fn is_shown(hwnd: HWND) -> bool {
    let (visible, cloak) = visible_and_cloak(hwnd);
    visible && cloak == 0
}

pub fn key_down(key: u16) -> bool {
    (unsafe { GetAsyncKeyState(key as i32) } as u16) & 0x8000 != 0
}

pub fn digit_down() -> bool {
    (b'0'..=b'9').any(|key| key_down(u16::from(key)))
}

/// The keys and mouse buttons as they are now.
pub fn key_state() -> KeyState {
    let win_down = key_down(VK_LWIN) || key_down(VK_RWIN);
    KeyState {
        win_down,
        // Only asked while a Windows key is down: Win+digit is all it is for.
        digit_down: win_down && digit_down(),
        alt_down: key_down(VK_MENU),
        nav_down: [
            VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_TAB, VK_RETURN, VK_SPACE, VK_ESCAPE,
        ]
        .into_iter()
        .any(key_down),
        escape: key_down(VK_ESCAPE),
        mouse_button_down: key_down(VK_LBUTTON) || key_down(VK_RBUTTON) || key_down(VK_MBUTTON),
        middle_down: key_down(VK_MBUTTON),
    }
}

/// Two WinEvent hooks on Explorer that call `on_event` the moment it shows
/// or uncloaks a window, so its popup can be hidden before it is drawn.
/// Out of context: the events come through the calling thread's message
/// loop and Explorer never waits for them. Empty when Explorer is not
/// running.
pub fn hook_explorer(on_event: WINEVENTPROC) -> Vec<HWINEVENTHOOK> {
    let tray = unsafe { FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null()) };
    let mut explorer = 0u32;
    unsafe { GetWindowThreadProcessId(tray, &mut explorer) };
    if explorer == 0 {
        return Vec::new();
    }
    [EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED]
        .into_iter()
        .map(|event| unsafe {
            SetWinEventHook(
                event,
                event,
                std::ptr::null_mut(),
                on_event,
                explorer,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        })
        .filter(|hook| !hook.is_null())
        .collect()
}

/// Takes out the hooks `hook_explorer` put in.
pub fn unhook_explorer(hooks: &mut Vec<HWINEVENTHOOK>) {
    for hook in hooks.drain(..) {
        unsafe { UnhookWinEvent(hook) };
    }
}

/// Whether a WinEvent from `hook_explorer` is about the popup's own window.
pub fn is_popup_event(hwnd: HWND, id_object: i32, id_child: i32) -> bool {
    id_object == OBJID_WINDOW
        && id_child == CHILDID_SELF as i32
        && !hwnd.is_null()
        && class_name(hwnd) == POPUP_CLASS
}
