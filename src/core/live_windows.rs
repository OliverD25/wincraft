//! The app windows open now, each with what tells it apart (its program,
//! title and rectangle) and the taskbar group its button is in. LayoutKeeper
//! watches them; WindowDeck lists them when LayoutKeeper is off, and the
//! hover strip counts them. Read only: nothing here changes a window.

use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Dwm::DWM_CLOAKED_SHELL;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsZoomed,
    SW_SHOWMAXIMIZED, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
};

use crate::core::appid;
use crate::core::taskbar_order::Handle;
use crate::core::virtual_desktops::{self as desktops, Desktop, DesktopId, Reader};
use crate::core::window_identity::WindowIdentity;
use crate::core::window_snapshot;
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
        let exe_path = path_of(&mut path_of_pid, hwnd);
        if !watched(&file_name(&exe_path)) {
            continue;
        }
        windows.push(read_one(hwnd, exe_path).live());
    }
    windows
}

/// What is read about one app window, as plain data that any thread may
/// hold, so a window snapshot (`core::window_snapshot`) can be read on a
/// thread of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Read {
    pub hwnd: isize,
    pub exe_path: String,
    pub title: String,
    pub rect: [i32; 4],
    pub maximized: bool,
    pub app: appid::AppId,
    /// The desktop the window is on, read only for a snapshot.
    pub desktop: Option<DesktopId>,
}

impl Read {
    /// The window as `enumerate` gives it.
    pub fn live(&self) -> LiveWindow {
        let exe = file_name(&self.exe_path);
        let group = appid::group_key(self.app.id.as_deref(), &self.exe_path);
        LiveWindow {
            hwnd: self.hwnd as HWND,
            identity: WindowIdentity::new(&exe, &self.title, self.rect, self.maximized)
                .in_group(&group),
            group,
            app_name: self.app.name.clone(),
            exe_path: self.exe_path.clone(),
        }
    }
}

/// The program path of the window's process, asked once per process.
pub(crate) fn path_of(path_of_pid: &mut HashMap<u32, String>, hwnd: HWND) -> String {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    path_of_pid
        .entry(pid)
        .or_insert_with(|| exe_path(pid))
        .clone()
}

/// Reads one window's title, place and app id; its desktop is left out.
pub(crate) fn read_one(hwnd: HWND, exe_path: String) -> Read {
    let (rect, maximized) = placement(hwnd);
    Read {
        hwnd: hwnd as isize,
        title: window_text(hwnd),
        rect,
        maximized,
        app: appid::read(hwnd),
        exe_path,
        desktop: None,
    }
}

/// WinCraft's own windows are never listed: the strip would show itself.
pub const OWN_EXE: &str = "wincraft.exe";
/// Windows' own helper processes. They can own visible windows with titles
/// (rundll32 hosts the Sound control panel, the shell hosts draw Start,
/// search, the touch keyboard and the lock screen), but they are not apps a
/// person arranges, and a restore must never move them. Always skipped,
/// whatever the settings say. ApplicationFrameHost is not here: it owns the
/// windows of Settings and other Store apps.
pub const SYSTEM_HELPERS: &[&str] = &[
    "rundll32.exe",
    "dllhost.exe",
    "ShellExperienceHost.exe",
    "StartMenuExperienceHost.exe",
    "SearchHost.exe",
    "TextInputHost.exe",
    "LockApp.exe",
    "ShellHost.exe",
];

/// Matches the file name, in any case, whether given alone or as a path.
pub fn is_system_helper(exe: &str) -> bool {
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    SYSTEM_HELPERS
        .iter()
        .any(|helper| helper.eq_ignore_ascii_case(name))
}

/// A program whose windows are worth listing: any but WinCraft itself and
/// Windows' helpers.
pub fn is_app(exe: &str) -> bool {
    let exe = exe.to_lowercase();
    !exe.is_empty() && exe != OWN_EXE && !is_system_helper(&exe)
}

/// A desktop in Task View, or "every desktop" for a window shown on all of
/// them, which counts as on this one too.
pub fn known_desktop(id: DesktopId, registry: &[Desktop]) -> bool {
    id == DesktopId::ALL || registry.iter().any(|desktop| desktop.id == id)
}

/// `enumerate` for the programs `watched` accepts, telling a window on
/// another desktop from one the shell hid with `reader`. Without a reader
/// a cloaked window cannot be placed, and it is kept.
/// When a window snapshot is in use (see `core::window_snapshot`), the
/// windows come from it instead of from Windows.
pub fn on_desktops(watched: &dyn Fn(&str) -> bool, reader: Option<&Reader>) -> Vec<LiveWindow> {
    if reader.is_some() {
        if let Some(live) = window_snapshot::live(watched) {
            return live;
        }
    }
    let registry = desktops::list();
    let on_known_desktop = |hwnd: HWND| match reader {
        Some(reader) => reader
            .read(hwnd)
            .is_some_and(|id| known_desktop(id, &registry)),
        None => true,
    };
    enumerate(watched, &on_known_desktop)
}

/// The windows of one taskbar group, as the order model takes them.
pub fn handles_in(live: &[LiveWindow], key: &str) -> Vec<(Handle, WindowIdentity)> {
    live.iter()
        .filter(|window| window.group == key)
        .map(|window| (window.hwnd as Handle, window.identity.clone()))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_and_wincraft_are_not_apps_in_any_case_or_as_a_path() {
        assert!(is_system_helper("RUNDLL32.EXE"));
        assert!(is_system_helper(r"C:\Windows\System32\rundll32.exe"));
        assert!(is_system_helper("C:/Windows/SystemApps/SearchHost.exe"));
        assert!(!is_system_helper("rundll32.exe.bak"));
        assert!(!is_app("WinCraft.exe"));
        assert!(!is_app(""));
        assert!(!is_app("lockapp.exe"));
        assert!(is_app("chrome.exe"));
        assert!(
            is_app("ApplicationFrameHost.exe"),
            "it owns Store apps' windows"
        );
    }

    #[test]
    fn a_window_on_every_desktop_or_a_listed_one_is_on_a_known_desktop() {
        let registry = [Desktop {
            id: DesktopId(7),
            name: "Desktop 1".to_string(),
        }];
        assert!(known_desktop(DesktopId::ALL, &registry));
        assert!(known_desktop(DesktopId(7), &registry));
        assert!(!known_desktop(DesktopId(8), &registry));
        assert!(known_desktop(DesktopId::ALL, &[]));
    }

    #[test]
    fn a_file_name_is_the_last_part_of_the_path_in_lower_case() {
        assert_eq!(file_name(r"C:\Program Files\App\App.EXE"), "app.exe");
        assert_eq!(file_name("notepad.exe"), "notepad.exe");
        assert_eq!(file_name(""), "");
    }
}
