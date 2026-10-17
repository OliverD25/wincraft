use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Dwm::DWM_CLOAKED_SHELL;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsZoomed,
    SW_SHOWMAXIMIZED, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
};

use super::identity::WindowIdentity;
use crate::core::appid;
use crate::core::windows_list;

pub use crate::core::windows_list::{exe_path, window_text};

pub struct LiveWindow {
    pub hwnd: HWND,
    pub identity: WindowIdentity,
    /// The taskbar group the window's button is in (see `appid::group_key`).
    pub group: String,
    /// The app's own name, when the window gives one.
    pub app_name: Option<String>,
    pub exe_path: String,
}

/// The app windows (as Alt+Tab counts them) of the programs `watched`
/// accepts, top of the z-order first, across every virtual desktop.
/// `on_known_desktop` tells a window on another desktop from one the shell
/// hid.
pub fn enumerate(
    watched: &dyn Fn(&str) -> bool,
    on_known_desktop: &dyn Fn(HWND) -> bool,
) -> Vec<LiveWindow> {
    let mut path_of_pid: HashMap<u32, String> = HashMap::new();
    let mut windows = Vec::new();
    for hwnd in windows_list::app_windows(on_known_desktop) {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let exe_path = path_of_pid
            .entry(pid)
            .or_insert_with(|| exe_path(pid))
            .clone();
        let exe = file_name(&exe_path);
        if !watched(&exe) {
            continue;
        }
        let title = window_text(hwnd);
        let (rect, maximized) = placement(hwnd);
        let app = appid::read(hwnd);
        let group = appid::group_key(app.id.as_deref(), &exe_path);
        windows.push(LiveWindow {
            hwnd,
            identity: WindowIdentity::new(&exe, &title, rect, maximized).in_group(&group),
            group,
            app_name: app.name,
            exe_path,
        });
    }
    windows
}

/// The shell cloaks the windows of every desktop but the current one.
pub fn on_other_desktop(hwnd: HWND) -> bool {
    windows_list::cloak(hwnd) & DWM_CLOAKED_SHELL != 0
}

/// A window's program and taskbar group, for a window found some other way,
/// such as the one in front: (exe name, group key).
pub fn describe(hwnd: HWND) -> (String, String) {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let path = exe_path(pid);
    let app = appid::read(hwnd);
    (file_name(&path), appid::group_key(app.id.as_deref(), &path))
}

/// Lower-case file name of a program path, like "chrome.exe".
pub fn file_name(path: &str) -> String {
    path.rsplit('\\').next().unwrap_or("").to_lowercase()
}

/// The "FileDescription" in a program's version resource, which is the
/// name most programs give themselves: "Character Map", "Telegram Desktop".
pub fn file_description(path: &str) -> Option<String> {
    crate::core::version_info::string(path, "FileDescription")
}

/// A minimized window reports -32000 as its position, so its restored
/// rectangle stands in; "maximized" then means it will come back maximized.
fn placement(hwnd: HWND) -> ([i32; 4], bool) {
    let mut place: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    let placed = unsafe { GetWindowPlacement(hwnd, &mut place) } != 0;
    if unsafe { IsIconic(hwnd) } != 0 && placed {
        let r = place.rcNormalPosition;
        let maximized = place.flags & WPF_RESTORETOMAXIMIZED != 0;
        return ([r.left, r.top, r.right, r.bottom], maximized);
    }
    let mut r: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetWindowRect(hwnd, &mut r) };
    let maximized =
        unsafe { IsZoomed(hwnd) } != 0 || (placed && place.showCmd == SW_SHOWMAXIMIZED as u32);
    ([r.left, r.top, r.right, r.bottom], maximized)
}
