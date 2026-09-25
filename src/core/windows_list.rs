//! Which top-level windows are app windows: the ones Alt+Tab shows. A pure
//! rule over a few facts about each window, plus the Win32 calls that read
//! those facts, so the rule can be tested on made-up windows. LayoutKeeper
//! and the palette's windows list both use it.

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

/// The Alt+Tab rule. WS_EX_APPWINDOW asks for a taskbar button and an
/// Alt+Tab entry, so it outweighs both WS_EX_TOOLWINDOW and a visible owner.
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
    if record.owner_visible && !app {
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

/// An app window the shell has cloaked because it sits on another virtual
/// desktop.
pub fn on_other_desktop(record: &WindowRecord) -> bool {
    record.cloak & DWM_CLOAKED_SHELL != 0
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

/// A Store app's content window inside its frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CoreWindow {
    visible: bool,
    cloak: u32,
}

/// On another desktop the shell cloaks the frame and its content alike, so
/// only a cloak by the app itself means the content is gone. Every
/// CoreWindow child is looked at, not just the first.
fn any_shown(children: &[CoreWindow]) -> bool {
    children
        .iter()
        .any(|child| child.visible && child.cloak & DWM_CLOAKED_APP == 0)
}

fn frame_has_content(frame: HWND) -> bool {
    let class = wide(CORE_WINDOW_CLASS);
    let mut children = Vec::new();
    let mut child: HWND = std::ptr::null_mut();
    loop {
        child = unsafe { FindWindowExW(frame, child, class.as_ptr(), std::ptr::null()) };
        if child.is_null() {
            return any_shown(&children);
        }
        children.push(CoreWindow {
            visible: unsafe { IsWindowVisible(child) } != 0,
            cloak: cloak(child),
        });
    }
}

/// Every app window, top of the z-order first, across all virtual desktops.
pub fn app_windows(on_known_desktop: &dyn Fn(HWND) -> bool) -> Vec<HWND> {
    app_window_records(on_known_desktop)
        .into_iter()
        .map(|(hwnd, _)| hwnd)
        .collect()
}

/// The same windows with what was read about each, for a caller that
/// shows their titles or where they are.
pub fn app_window_records(on_known_desktop: &dyn Fn(HWND) -> bool) -> Vec<(HWND, WindowRecord)> {
    let mut candidates: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        let found = unsafe { &mut *(found as *mut Vec<HWND>) };
        found.push(hwnd);
        1
    }
    unsafe { EnumWindows(Some(collect), &mut candidates as *mut Vec<HWND> as LPARAM) };
    candidates
        .into_iter()
        .map(|hwnd| (hwnd, read(hwnd, on_known_desktop)))
        .filter(|(_, record)| is_app_window(record))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Dwm::DWM_CLOAKED_INHERITED;

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
        assert!(!on_other_desktop(&app("Inbox - Gmail")));
    }

    // The windows list's own cases from the palette search, kept as they
    // were written there.

    #[test]
    fn a_window_on_another_desktop_is_listed_and_marked() {
        let window = WindowRecord {
            cloak: DWM_CLOAKED_SHELL,
            on_known_desktop: true,
            ..app("Notes")
        };
        assert!(is_app_window(&window));
        assert!(on_other_desktop(&window));
    }

    #[test]
    fn hidden_system_windows_are_left_out() {
        // Windows Input Experience: cloaked by the shell, on no desktop.
        assert!(!is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_SHELL,
            on_known_desktop: false,
            ..app("Windows Input Experience")
        }));
        // A suspended Settings window, cloaked by its app.
        assert!(!is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_APP,
            ..app("Settings")
        }));
        assert!(!is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_SHELL | DWM_CLOAKED_APP,
            on_known_desktop: true,
            ..app("Settings")
        }));
    }

    #[test]
    fn tool_windows_owned_windows_and_untitled_ones_are_left_out() {
        let tool = WindowRecord {
            ex_style: WS_EX_TOOLWINDOW,
            ..app("Palette")
        };
        assert!(!is_app_window(&tool));
        assert!(is_app_window(&WindowRecord {
            ex_style: WS_EX_TOOLWINDOW | WS_EX_APPWINDOW,
            ..tool
        }));
        let dialog = WindowRecord {
            owner_visible: true,
            ..app("Save as")
        };
        assert!(!is_app_window(&dialog));
        assert!(is_app_window(&WindowRecord {
            owner_visible: false,
            ..dialog
        }));
        assert!(!is_app_window(&app("")));
        assert!(!is_app_window(&WindowRecord {
            visible: false,
            ..app("Hidden")
        }));
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
    fn an_owned_window_that_asks_for_a_button_counts() {
        // Alt+Tab lists an owned window with WS_EX_APPWINDOW, as apps use
        // for a second top-level window of their own.
        assert!(is_app_window(&WindowRecord {
            owner_visible: true,
            ex_style: WS_EX_APPWINDOW,
            ..app("Picture in picture")
        }));
        assert!(is_app_window(&WindowRecord {
            owner_visible: true,
            ex_style: WS_EX_APPWINDOW | WS_EX_TOOLWINDOW,
            ..app("Floating player")
        }));
        assert!(!is_app_window(&WindowRecord {
            owner_visible: true,
            ex_style: WS_EX_TOOLWINDOW,
            ..app("Find")
        }));
    }

    #[test]
    fn a_title_of_spaces_still_counts_but_an_empty_one_does_not() {
        assert!(!is_app_window(&app("")));
        assert!(is_app_window(&app(" ")));
    }

    #[test]
    fn the_taskbars_and_the_desktop_are_never_app_windows() {
        for class in SHELL_CLASSES {
            assert!(!is_app_window(&WindowRecord {
                class: class.to_string(),
                ..app("Shell")
            }));
        }
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
            ..other_desktop.clone()
        }));
        assert!(on_other_desktop(&other_desktop));
        assert!(!on_other_desktop(&app("Here")));
    }

    #[test]
    fn a_cloak_inherited_from_the_owner_changes_nothing_by_itself() {
        assert!(is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_INHERITED,
            ..app("Child of a cloaked owner")
        }));
        assert!(is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_SHELL | DWM_CLOAKED_INHERITED,
            on_known_desktop: true,
            ..app("On another desktop")
        }));
        assert!(!is_app_window(&WindowRecord {
            cloak: DWM_CLOAKED_APP | DWM_CLOAKED_INHERITED,
            ..app("Suspended")
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
            ..frame.clone()
        }));
        // A frame on another desktop is cloaked by the shell like any window.
        assert!(is_app_window(&WindowRecord {
            frame_has_content: true,
            cloak: DWM_CLOAKED_SHELL,
            on_known_desktop: true,
            ..frame
        }));
    }

    #[test]
    fn a_frame_counts_when_any_of_its_app_windows_is_shown() {
        let shown = CoreWindow {
            visible: true,
            cloak: 0,
        };
        let cloaked_by_app = CoreWindow {
            visible: true,
            cloak: DWM_CLOAKED_APP,
        };
        let hidden = CoreWindow {
            visible: false,
            cloak: 0,
        };
        let other_desktop = CoreWindow {
            visible: true,
            cloak: DWM_CLOAKED_SHELL,
        };
        assert!(!any_shown(&[]));
        assert!(any_shown(&[shown]));
        assert!(!any_shown(&[cloaked_by_app]));
        assert!(!any_shown(&[hidden]));
        assert!(any_shown(&[other_desktop]));
        // A frame can hold an old cloaked CoreWindow before the live one.
        assert!(any_shown(&[cloaked_by_app, shown]));
        assert!(!any_shown(&[hidden, cloaked_by_app]));
    }
}
