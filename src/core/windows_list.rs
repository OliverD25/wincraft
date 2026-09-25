//! Which top-level windows are app windows: the ones Alt+Tab shows. A pure
//! rule over a few facts about each window, plus the Win32 calls that read
//! those facts, so the rule can be tested on made-up windows.

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWM_CLOAKED_APP, DWM_CLOAKED_SHELL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GetClassNameW, GetWindow, GetWindowLongW, GetWindowTextW,
    IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

use crate::core::wide;

/// Explorer's desktop and taskbars: visible, titled, and never app windows.
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

/// Store apps draw inside a frame window owned by ApplicationFrameHost. A
/// frame without its app's CoreWindow is a leftover of a suspended or closed
/// app that Alt+Tab does not show.
const FRAME_CLASS: &str = "ApplicationFrameWindow";
const CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

/// What the rule needs to know about one top-level window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowRecord {
    pub visible: bool,
    pub ex_style: u32,
    /// The window has an owner, and that owner is visible.
    pub owner_visible: bool,
    pub title: String,
    pub class: String,
    /// DWMWA_CLOAKED: 0, or DWM_CLOAKED_APP / _SHELL / _INHERITED bits.
    pub cloak: u32,
    /// Only asked for a window the shell cloaked: whether it sits on a
    /// virtual desktop that exists. The shell cloaks the windows of every
    /// other desktop, and those count; it also cloaks windows it hides
    /// entirely, and those do not.
    pub on_known_desktop: bool,
    /// Only asked for an ApplicationFrameWindow: whether it holds a visible
    /// CoreWindow that its app has not cloaked.
    pub frame_has_content: bool,
}

/// The Alt+Tab rule.
pub fn is_app_window(record: &WindowRecord) -> bool {
    if !record.visible || record.title.is_empty() {
        return false;
    }
    if SHELL_CLASSES.contains(&record.class.as_str()) {
        return false;
    }
    let tool = record.ex_style & WS_EX_TOOLWINDOW != 0;
    let app = record.ex_style & WS_EX_APPWINDOW != 0;
    if tool && !app {
        return false;
    }
    if record.owner_visible {
        return false;
    }
    if record.cloak & DWM_CLOAKED_APP != 0 {
        return false;
    }
    if record.cloak & DWM_CLOAKED_SHELL != 0 && !record.on_known_desktop {
        return false;
    }
    if record.class == FRAME_CLASS && !record.frame_has_content {
        return false;
    }
    true
}

pub fn cloak(hwnd: HWND) -> u32 {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    cloaked
}

pub fn window_text(hwnd: HWND) -> String {
    let mut buffer = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// Reads the facts about one window. `on_known_desktop` is only called for
/// a window the shell has cloaked, because asking the desktop manager costs
/// a COM call.
pub fn read(hwnd: HWND, on_known_desktop: &dyn Fn(HWND) -> bool) -> WindowRecord {
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    if !visible {
        return WindowRecord::default();
    }
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
    let class = class_name(hwnd);
    let cloak = cloak(hwnd);
    WindowRecord {
        visible,
        ex_style: unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32,
        owner_visible: !owner.is_null() && unsafe { IsWindowVisible(owner) } != 0,
        title: window_text(hwnd),
        on_known_desktop: cloak & DWM_CLOAKED_SHELL != 0 && on_known_desktop(hwnd),
        frame_has_content: class == FRAME_CLASS && frame_has_content(hwnd),
        class,
        cloak,
    }
}

/// On another desktop the shell cloaks the frame and its content alike, so
/// only a cloak by the app itself means the content is gone.
fn frame_has_content(frame: HWND) -> bool {
    let class = wide(CORE_WINDOW_CLASS);
    let mut child: HWND = std::ptr::null_mut();
    loop {
        child = unsafe { FindWindowExW(frame, child, class.as_ptr(), std::ptr::null()) };
        if child.is_null() {
            return false;
        }
        if unsafe { IsWindowVisible(child) } != 0 && cloak(child) & DWM_CLOAKED_APP == 0 {
            return true;
        }
    }
}

/// Every app window, top of the z-order first, across all virtual desktops.
pub fn app_windows(on_known_desktop: &dyn Fn(HWND) -> bool) -> Vec<HWND> {
    let mut candidates: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        let found = unsafe { &mut *(found as *mut Vec<HWND>) };
        found.push(hwnd);
        1
    }
    unsafe { EnumWindows(Some(collect), &mut candidates as *mut Vec<HWND> as LPARAM) };
    candidates
        .into_iter()
        .filter(|hwnd| is_app_window(&read(*hwnd, on_known_desktop)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(title: &str) -> WindowRecord {
        WindowRecord {
            visible: true,
            title: title.to_string(),
            class: "Chrome_WidgetWin_1".to_string(),
            ..WindowRecord::default()
        }
    }

    #[test]
    fn a_plain_visible_titled_window_counts() {
        assert!(is_app_window(&app("Inbox - Gmail")));
    }

    #[test]
    fn hidden_untitled_and_shell_windows_do_not() {
        assert!(!is_app_window(&WindowRecord {
            visible: false,
            ..app("Hidden")
        }));
        assert!(!is_app_window(&app("")));
        assert!(!is_app_window(&WindowRecord {
            class: "Progman".to_string(),
            ..app("Program Manager")
        }));
    }

    #[test]
    fn tool_windows_count_only_when_they_ask_for_a_button() {
        let tool = WindowRecord {
            ex_style: WS_EX_TOOLWINDOW,
            ..app("Palette")
        };
        assert!(!is_app_window(&tool));
        assert!(is_app_window(&WindowRecord {
            ex_style: WS_EX_TOOLWINDOW | WS_EX_APPWINDOW,
            ..tool
        }));
    }

    #[test]
    fn a_window_with_a_visible_owner_is_a_dialog() {
        assert!(!is_app_window(&WindowRecord {
            owner_visible: true,
            ..app("Save As")
        }));
        // A hidden owner, as WinForms and many frameworks use, is fine.
        assert!(is_app_window(&WindowRecord {
            owner_visible: false,
            ..app("Main window")
        }));
    }

    #[test]
    fn a_window_on_another_desktop_counts_and_a_hidden_one_does_not() {
        let other_desktop = WindowRecord {
            cloak: DWM_CLOAKED_SHELL,
            on_known_desktop: true,
            ..app("Research")
        };
        assert!(is_app_window(&other_desktop));
        assert!(!is_app_window(&WindowRecord {
            on_known_desktop: false,
            ..other_desktop.clone()
        }));
        assert!(!is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_APP,
            ..app("Background")
        }));
        assert!(!is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_APP | DWM_CLOAKED_SHELL,
            ..other_desktop
        }));
    }

    #[test]
    fn a_store_app_frame_needs_its_content() {
        let frame = WindowRecord {
            class: FRAME_CLASS.to_string(),
            ..app("Settings")
        };
        assert!(!is_app_window(&frame));
        assert!(is_app_window(&WindowRecord {
            frame_has_content: true,
            ..frame
        }));
    }
}
