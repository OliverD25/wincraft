//! Finding, reading and hiding Explorer's taskbar hover popup. Shared by
//! WindowDeck's hover strip and by LayoutKeeper's hide-popup test
//! (`--test-hide-popup`).
//!
//! Only plain hides are used, never a move off-screen: Explorer shows a
//! hidden popup again on the next hover by itself, so a WinCraft that
//! crashes or quits can never leave the taskbar without its popup.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_DOWN, VK_ESCAPE, VK_LBUTTON, VK_LEFT, VK_LWIN, VK_MBUTTON, VK_MENU,
    VK_RBUTTON, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, IsWindowVisible, SetWindowPos, ShowWindowAsync, SWP_ASYNCWINDOWPOS,
    SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
};

pub mod keys;

use crate::core::wide;
use keys::{KeyState, POPUP_CLASS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Method {
    ShowWindowAsync,
    SetWindowPosHide,
    MoveOffScreen,
}

impl Method {
    pub const ALL: [Method; 3] = [
        Method::ShowWindowAsync,
        Method::SetWindowPosHide,
        Method::MoveOffScreen,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Method::ShowWindowAsync => "(a) ShowWindowAsync SW_HIDE",
            Method::SetWindowPosHide => "(b) SetWindowPos SWP_HIDEWINDOW",
            Method::MoveOffScreen => "(c) SetWindowPos off-screen",
        }
    }
}

/// Hides the popup the way `method` says. `MoveOffScreen` does nothing:
/// it is only named so the test can report it was never used.
pub fn hide(hwnd: HWND, method: Method) {
    match method {
        Method::ShowWindowAsync => {
            unsafe { ShowWindowAsync(hwnd, SW_HIDE) };
        }
        Method::SetWindowPosHide => {
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_HIDEWINDOW
                        | SWP_NOACTIVATE
                        | SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOZORDER
                        | SWP_ASYNCWINDOWPOS,
                )
            };
        }
        Method::MoveOffScreen => {}
    }
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
