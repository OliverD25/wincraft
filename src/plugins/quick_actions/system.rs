//! Everything QuickActions asks of Windows, behind one trait. The plugin
//! only talks to `System`, so the tests run on `tests::Fake` and a test
//! instance runs on `ReadOnly`, which reads the real PC but only logs the
//! changes it would make (Principle 1: a test never changes the user's
//! real settings or windows).

use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};
use windows_sys::Win32::System::Registry::{
    RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Shell::{
    SHAppBarMessage, SHChangeNotify, ABM_GETSTATE, ABM_SETSTATE, APPBARDATA, SHCNE_ASSOCCHANGED,
    SHCNF_IDLIST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed, SendMessageTimeoutW,
    SetWindowPlacement, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_BROADCAST, HWND_NOTOPMOST,
    HWND_TOPMOST, SMTO_ABORTIFHUNG, SMTO_BLOCK, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_RESTORE, WINDOWPLACEMENT, WM_COMMAND, WM_SETTINGCHANGE, WS_EX_TOPMOST,
};

use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHQueryRecycleBinW, ShellExecuteW, SHERB_NOCONFIRMATION,
    SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};

use super::audio;
use super::network::{self, Address};
use crate::core::monitors::{self, Rect};
use crate::core::{clipboard, wide, windows_list};

/// The command Explorer's desktop view takes to show or hide its icons,
/// the same as "Show desktop icons" in the desktop's menu.
const TOGGLE_DESKTOP_ICONS: usize = 0x7402;

/// A window's place as Windows keeps it, to put it back exactly on Undo,
/// maximised or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    pub flags: u32,
    pub show: u32,
    pub min: [i32; 2],
    pub max: [i32; 2],
    pub normal: Rect,
}

/// What the window actions need to know about a window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    pub title: String,
    /// The rectangle Windows moves, with the invisible resize border.
    pub outer: Rect,
    /// The visible frame; `outer` again when DWM does not say.
    pub frame: Rect,
    /// The work area of the window's monitor: the screen without the taskbar.
    pub work: Rect,
    pub maximized: bool,
    pub minimized: bool,
    pub topmost: bool,
    pub placement: Placement,
}

pub trait System {
    fn read_dword(&self, key: &str, name: &str) -> Option<u32>;
    fn write_dword(&mut self, key: &str, name: &str, value: u32) -> Result<(), String>;
    /// Tells every window that a setting changed, as Settings does.
    fn announce(&mut self, area: &str);
    /// Asks open Explorer windows and the desktop to read their view
    /// settings again.
    fn refresh_explorer(&mut self);
    fn taskbar_state(&self) -> Option<u32>;
    fn set_taskbar_state(&mut self, state: u32) -> Result<(), String>;
    fn desktop_icons_shown(&self) -> Option<bool>;
    fn toggle_desktop_icons(&mut self) -> Result<(), String>;

    /// The window the user means: the one in front, or, when that is
    /// WinCraft's own (the palette), the one that was in front before it.
    fn target_window(&self) -> Option<isize>;
    fn window(&self, hwnd: isize) -> Option<WindowState>;
    fn set_topmost(&mut self, hwnd: isize, on: bool) -> Result<(), String>;
    /// Brings a maximised or minimised window back to its normal size.
    fn restore(&mut self, hwnd: isize);
    fn move_window(&mut self, hwnd: isize, outer: Rect) -> Result<(), String>;
    fn set_placement(&mut self, hwnd: isize, placement: &Placement) -> Result<(), String>;

    /// Keeps the PC and the screen awake while `on`.
    fn keep_awake(&mut self, on: bool) -> Result<(), String>;

    /// The PC's own IPv4 addresses, Wi-Fi first.
    fn local_addresses(&self) -> Vec<Address>;
    fn flush_dns(&mut self) -> Result<(), String>;
    fn clipboard_text(&self) -> Option<String>;
    /// Replaces everything on the clipboard with this text.
    fn put_clipboard(&mut self, text: &str) -> Result<(), String>;
    /// 16 bytes from Windows' random number source.
    fn random(&self) -> Result<[u8; 16], String>;
    /// Whether the default microphone is muted, and its name.
    fn microphone(&self) -> Result<(bool, String), String>;
    fn set_microphone_muted(&mut self, muted: bool) -> Result<(), String>;
    /// The Recycle Bin's item count and size in bytes, on every drive.
    fn recycle_bin(&self) -> Option<(u64, u64)>;
    /// Empties the Recycle Bin. True when it is done; false when it goes on
    /// in the background and `WM_APP_BIN_DONE` reports the end.
    fn empty_recycle_bin(&mut self) -> Result<bool, String>;
    fn open_environment_variables(&mut self) -> Result<(), String>;
}

/// Posted to the plugin's window when emptying the Recycle Bin ended:
/// wParam 1 when it worked.
pub const WM_APP_BIN_DONE: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 50;

/// The real thing. `owner` is the plugin's window: the clipboard names it
/// as its owner, and the Recycle Bin reports to it.
#[derive(Default)]
pub struct Live {
    pub owner: isize,
}

impl System for Live {
    fn read_dword(&self, key: &str, name: &str) -> Option<u32> {
        let (key, name) = (wide(key), wide(name));
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        (status == 0).then_some(value)
    }

    fn write_dword(&mut self, key: &str, name: &str, value: u32) -> Result<(), String> {
        let (wide_key, wide_name) = (wide(key), wide(name));
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                wide_key.as_ptr(),
                wide_name.as_ptr(),
                REG_DWORD,
                (&value as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("Windows refused to save {name} (error {status})"))
        }
    }

    fn announce(&mut self, area: &str) {
        let area = area.to_string();
        // On a thread of its own: the broadcast waits for every window,
        // WinCraft's host window too, which must stay free to answer.
        let _ = std::thread::Builder::new()
            .name("quick-actions-announce".to_string())
            .spawn(move || broadcast(Some(&area)));
    }

    fn refresh_explorer(&mut self) {
        let _ = std::thread::Builder::new()
            .name("quick-actions-refresh".to_string())
            .spawn(|| {
                unsafe {
                    SHChangeNotify(
                        SHCNE_ASSOCCHANGED as i32,
                        SHCNF_IDLIST,
                        std::ptr::null(),
                        std::ptr::null(),
                    )
                };
                broadcast(Some("ShellState"));
                broadcast(None);
            });
    }

    fn taskbar_state(&self) -> Option<u32> {
        let mut data = appbar();
        Some(unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) } as u32)
    }

    fn set_taskbar_state(&mut self, state: u32) -> Result<(), String> {
        let mut data = appbar();
        data.lParam = state as LPARAM;
        unsafe { SHAppBarMessage(ABM_SETSTATE, &mut data) };
        match self.taskbar_state() {
            Some(now) if now & 1 == state & 1 => Ok(()),
            _ => Err("the taskbar did not take the change".to_string()),
        }
    }

    fn desktop_icons_shown(&self) -> Option<bool> {
        let view = desktop_view()?;
        let list = unsafe {
            FindWindowExW(
                view,
                std::ptr::null_mut(),
                wide("SysListView32").as_ptr(),
                std::ptr::null(),
            )
        };
        (!list.is_null()).then(|| unsafe { IsWindowVisible(list) } != 0)
    }

    fn toggle_desktop_icons(&mut self) -> Result<(), String> {
        let view = desktop_view().ok_or("Windows did not show where the desktop icons are")?;
        let before = self.desktop_icons_shown();
        let mut result = 0usize;
        let sent = unsafe {
            SendMessageTimeoutW(
                view,
                WM_COMMAND,
                TOGGLE_DESKTOP_ICONS,
                0,
                SMTO_BLOCK | SMTO_ABORTIFHUNG,
                1000,
                &mut result,
            )
        };
        if sent == 0 {
            return Err("Windows did not answer the request".to_string());
        }
        // The desktop usually switches at once; give it a moment before
        // calling it silent.
        for _ in 0..5 {
            if self.desktop_icons_shown() != before {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(())
    }

    fn target_window(&self) -> Option<isize> {
        let front = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        if usable(front) {
            return Some(front as isize);
        }
        let before = crate::search::actions::front_before_palette();
        usable(before).then_some(before as isize)
    }

    fn window(&self, hwnd: isize) -> Option<WindowState> {
        let hwnd = hwnd as HWND;
        if unsafe { IsWindow(hwnd) } == 0 {
            return None;
        }
        let mut outer: RECT = unsafe { std::mem::zeroed() };
        if unsafe { GetWindowRect(hwnd, &mut outer) } == 0 {
            return None;
        }
        let mut frame: RECT = unsafe { std::mem::zeroed() };
        let framed = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                (&mut frame as *mut RECT).cast(),
                std::mem::size_of::<RECT>() as u32,
            )
        } == 0;
        let outer = corners(outer);
        let screens = monitors::screens();
        let work = monitors::screen_of(hwnd, &screens)
            .and_then(|index| screens.get(index))
            .map(|screen| screen.work)?;
        let mut place: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        unsafe { GetWindowPlacement(hwnd, &mut place) };
        let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        Some(WindowState {
            title: windows_list::window_text(hwnd),
            outer,
            frame: if framed { corners(frame) } else { outer },
            work,
            maximized: unsafe { IsZoomed(hwnd) } != 0,
            minimized: unsafe { IsIconic(hwnd) } != 0,
            topmost: ex_style & WS_EX_TOPMOST != 0,
            placement: Placement {
                flags: place.flags,
                show: place.showCmd,
                min: [place.ptMinPosition.x, place.ptMinPosition.y],
                max: [place.ptMaxPosition.x, place.ptMaxPosition.y],
                normal: corners(place.rcNormalPosition),
            },
        })
    }

    fn set_topmost(&mut self, hwnd: isize, on: bool) -> Result<(), String> {
        let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        unsafe { SetWindowPos(hwnd as HWND, after, 0, 0, 0, 0, flags) };
        match self.window(hwnd) {
            Some(state) if state.topmost == on => Ok(()),
            _ => Err(refused()),
        }
    }

    fn restore(&mut self, hwnd: isize) {
        unsafe { ShowWindow(hwnd as HWND, SW_RESTORE) };
    }

    fn move_window(&mut self, hwnd: isize, outer: Rect) -> Result<(), String> {
        let [left, top, right, bottom] = outer;
        let ok = unsafe {
            SetWindowPos(
                hwnd as HWND,
                std::ptr::null_mut(),
                left,
                top,
                right - left,
                bottom - top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } != 0;
        if ok {
            Ok(())
        } else {
            Err(refused())
        }
    }

    fn set_placement(&mut self, hwnd: isize, placement: &Placement) -> Result<(), String> {
        let [left, top, right, bottom] = placement.normal;
        let mut place: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        place.flags = placement.flags;
        place.showCmd = placement.show;
        place.ptMinPosition.x = placement.min[0];
        place.ptMinPosition.y = placement.min[1];
        place.ptMaxPosition.x = placement.max[0];
        place.ptMaxPosition.y = placement.max[1];
        place.rcNormalPosition = RECT {
            left,
            top,
            right,
            bottom,
        };
        if unsafe { SetWindowPlacement(hwnd as HWND, &place) } != 0 {
            Ok(())
        } else {
            Err(refused())
        }
    }

    fn keep_awake(&mut self, on: bool) -> Result<(), String> {
        let flags = if on {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        if unsafe { SetThreadExecutionState(flags) } == 0 {
            Err("Windows refused the request".to_string())
        } else {
            Ok(())
        }
    }

    fn local_addresses(&self) -> Vec<Address> {
        network::read()
    }

    fn flush_dns(&mut self) -> Result<(), String> {
        network::flush_dns()
    }

    fn clipboard_text(&self) -> Option<String> {
        clipboard::text(self.owner as HWND)
    }

    fn put_clipboard(&mut self, text: &str) -> Result<(), String> {
        if clipboard::put_text(self.owner as HWND, text) {
            Ok(())
        } else {
            Err("another app is using the clipboard".to_string())
        }
    }

    fn random(&self) -> Result<[u8; 16], String> {
        let mut bytes = [0u8; 16];
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                bytes.as_mut_ptr(),
                16,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status == 0 {
            Ok(bytes)
        } else {
            Err(format!("Windows gave no random numbers ({status:#x})"))
        }
    }

    fn microphone(&self) -> Result<(bool, String), String> {
        audio::state()
    }

    fn set_microphone_muted(&mut self, muted: bool) -> Result<(), String> {
        audio::set_muted(muted)?;
        match audio::state() {
            Ok((now, _)) if now == muted => Ok(()),
            _ => Err("the microphone did not take the change".to_string()),
        }
    }

    fn recycle_bin(&self) -> Option<(u64, u64)> {
        let mut info: SHQUERYRBINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
        let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
        (hr >= 0).then(|| (info.i64NumItems.max(0) as u64, info.i64Size.max(0) as u64))
    }

    fn empty_recycle_bin(&mut self) -> Result<bool, String> {
        let owner = self.owner;
        // A full bin takes a while; the host thread must not wait for it.
        std::thread::Builder::new()
            .name("quick-actions-recycle-bin".to_string())
            .spawn(move || {
                let com =
                    unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
                let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
                let hr =
                    unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), flags) };
                if com >= 0 {
                    unsafe { CoUninitialize() };
                }
                if hr < 0 {
                    log::warn!("QuickActions: emptying the Recycle Bin failed ({hr:#x})");
                }
                unsafe { PostMessageW(owner as HWND, WM_APP_BIN_DONE, usize::from(hr >= 0), 0) };
            })
            .map_err(|err| format!("could not start: {err}"))?;
        Ok(false)
    }

    fn open_environment_variables(&mut self) -> Result<(), String> {
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide("rundll32.exe").as_ptr(),
                wide("sysdm.cpl,EditEnvironmentVariables").as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecute reports success as any value above 32.
        if result as usize > 32 {
            Ok(())
        } else {
            Err(format!(
                "Windows could not open it (error {})",
                result as usize
            ))
        }
    }
}

fn refused() -> String {
    "Windows refused. An app that runs as administrator cannot be changed by WinCraft".to_string()
}

fn corners(rect: RECT) -> Rect {
    [rect.left, rect.top, rect.right, rect.bottom]
}

fn appbar() -> APPBARDATA {
    let mut data: APPBARDATA = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
    data.hWnd = unsafe { FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null()) };
    data
}

fn broadcast(area: Option<&str>) {
    let text = area.map(wide);
    let lparam = text.as_ref().map_or(0, |text| text.as_ptr() as LPARAM);
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            lparam,
            SMTO_ABORTIFHUNG,
            1000,
            &mut result,
        )
    };
}

/// A window of another program that the user works in: never WinCraft's
/// own, never the desktop or the taskbar.
fn usable(hwnd: HWND) -> bool {
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return false;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid != unsafe { GetCurrentProcessId() }
        && windows_list::is_app_window(&windows_list::read(hwnd, &|_| true))
}

/// Explorer's view of the desktop: under Progman, or under a WorkerW
/// after a wallpaper slideshow moved it.
fn desktop_view() -> Option<HWND> {
    let class = wide("SHELLDLL_DefView");
    let child = |parent: HWND| unsafe {
        FindWindowExW(
            parent,
            std::ptr::null_mut(),
            class.as_ptr(),
            std::ptr::null(),
        )
    };
    let progman = unsafe { FindWindowW(wide("Progman").as_ptr(), std::ptr::null()) };
    if !progman.is_null() {
        let view = child(progman);
        if !view.is_null() {
            return Some(view);
        }
    }
    let mut found: HWND = std::ptr::null_mut();
    unsafe { EnumWindows(Some(find_worker_view), &mut found as *mut HWND as LPARAM) };
    (!found.is_null()).then_some(found)
}

unsafe extern "system" fn find_worker_view(hwnd: HWND, data: LPARAM) -> i32 {
    if windows_list::class_name(hwnd) != "WorkerW" {
        return 1;
    }
    let view = unsafe {
        FindWindowExW(
            hwnd,
            std::ptr::null_mut(),
            wide("SHELLDLL_DefView").as_ptr(),
            std::ptr::null(),
        )
    };
    if view.is_null() {
        return 1;
    }
    unsafe { *(data as *mut HWND) = view };
    0
}

/// A test instance: reads the real PC, logs each change instead of making
/// it, and remembers the change so the next read and Undo behave as if it
/// had happened.
pub struct ReadOnly<S: System> {
    inner: S,
    registry: HashMap<(String, String), u32>,
    taskbar: Option<u32>,
    icons: Option<bool>,
    microphone: Option<bool>,
}

impl<S: System> ReadOnly<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            registry: HashMap::new(),
            taskbar: None,
            icons: None,
            microphone: None,
        }
    }
}

fn skipped(what: &str) {
    log::info!("read-only test instance: QuickActions {what} skipped");
}

impl<S: System> System for ReadOnly<S> {
    fn read_dword(&self, key: &str, name: &str) -> Option<u32> {
        self.registry
            .get(&(key.to_string(), name.to_string()))
            .copied()
            .or_else(|| self.inner.read_dword(key, name))
    }

    fn write_dword(&mut self, key: &str, name: &str, value: u32) -> Result<(), String> {
        skipped(&format!("setting {name} to {value}"));
        self.registry
            .insert((key.to_string(), name.to_string()), value);
        Ok(())
    }

    fn announce(&mut self, area: &str) {
        skipped(&format!("announcing {area}"));
    }

    fn refresh_explorer(&mut self) {
        skipped("refreshing Explorer");
    }

    fn taskbar_state(&self) -> Option<u32> {
        self.taskbar.or_else(|| self.inner.taskbar_state())
    }

    fn set_taskbar_state(&mut self, state: u32) -> Result<(), String> {
        skipped(&format!("setting the taskbar state to {state}"));
        self.taskbar = Some(state);
        Ok(())
    }

    fn desktop_icons_shown(&self) -> Option<bool> {
        self.icons.or_else(|| self.inner.desktop_icons_shown())
    }

    fn toggle_desktop_icons(&mut self) -> Result<(), String> {
        skipped("switching the desktop icons");
        let now = self.desktop_icons_shown().ok_or("no desktop view")?;
        self.icons = Some(!now);
        Ok(())
    }

    fn target_window(&self) -> Option<isize> {
        self.inner.target_window()
    }

    fn window(&self, hwnd: isize) -> Option<WindowState> {
        self.inner.window(hwnd)
    }

    fn set_topmost(&mut self, hwnd: isize, on: bool) -> Result<(), String> {
        skipped(&format!("setting always on top {on} for window {hwnd:#x}"));
        Ok(())
    }

    fn restore(&mut self, hwnd: isize) {
        skipped(&format!("restoring window {hwnd:#x}"));
    }

    fn move_window(&mut self, hwnd: isize, outer: Rect) -> Result<(), String> {
        skipped(&format!("moving window {hwnd:#x} to {outer:?}"));
        Ok(())
    }

    fn set_placement(&mut self, hwnd: isize, placement: &Placement) -> Result<(), String> {
        skipped(&format!(
            "putting window {hwnd:#x} back to {:?}",
            placement.normal
        ));
        Ok(())
    }

    fn keep_awake(&mut self, on: bool) -> Result<(), String> {
        skipped(&format!("keep awake {on}"));
        Ok(())
    }

    fn local_addresses(&self) -> Vec<Address> {
        self.inner.local_addresses()
    }

    fn flush_dns(&mut self) -> Result<(), String> {
        skipped("clearing the DNS cache");
        Ok(())
    }

    fn clipboard_text(&self) -> Option<String> {
        self.inner.clipboard_text()
    }

    fn put_clipboard(&mut self, text: &str) -> Result<(), String> {
        skipped(&format!(
            "putting {} characters on the clipboard",
            text.chars().count()
        ));
        Ok(())
    }

    fn random(&self) -> Result<[u8; 16], String> {
        self.inner.random()
    }

    fn microphone(&self) -> Result<(bool, String), String> {
        let (muted, name) = self.inner.microphone()?;
        Ok((self.microphone.unwrap_or(muted), name))
    }

    fn set_microphone_muted(&mut self, muted: bool) -> Result<(), String> {
        skipped(&format!("muting the microphone {muted}"));
        self.microphone = Some(muted);
        Ok(())
    }

    fn recycle_bin(&self) -> Option<(u64, u64)> {
        self.inner.recycle_bin()
    }

    fn empty_recycle_bin(&mut self) -> Result<bool, String> {
        skipped("emptying the Recycle Bin");
        Ok(true)
    }

    fn open_environment_variables(&mut self) -> Result<(), String> {
        skipped("opening Environment Variables");
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A pretend PC: settings in memory, windows that are only records,
    /// and a list of the calls that would have reached Windows.
    pub struct Fake {
        pub registry: HashMap<(String, String), u32>,
        pub taskbar: u32,
        pub icons: Option<bool>,
        /// The desktop takes the toggle but nothing changes.
        pub icons_ignore: bool,
        pub windows: HashMap<isize, WindowState>,
        pub target: Option<isize>,
        /// Windows refuses every window change, as for an app run as
        /// administrator.
        pub refuse: bool,
        pub awake: bool,
        pub addresses: Vec<Address>,
        pub clipboard: Option<String>,
        /// None: no microphone.
        pub microphone: Option<bool>,
        pub bin: Option<(u64, u64)>,
        pub log: Vec<String>,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                registry: HashMap::new(),
                taskbar: 0x2,
                icons: Some(true),
                icons_ignore: false,
                windows: HashMap::new(),
                target: None,
                refuse: false,
                awake: false,
                addresses: Vec::new(),
                clipboard: None,
                microphone: Some(false),
                bin: Some((0, 0)),
                log: Vec::new(),
            }
        }
    }

    impl Fake {
        pub fn dword(&self, key: &str, name: &str) -> Option<u32> {
            self.read_dword(key, name)
        }

        pub fn set_dword(&mut self, key: &str, name: &str, value: u32) {
            self.registry
                .insert((key.to_string(), name.to_string()), value);
        }

        pub fn calls(&self) -> Vec<&str> {
            self.log.iter().map(String::as_str).collect()
        }

        /// A normal window of 800 x 600 at (100, 100) on a 1920 x 1080
        /// screen, with Windows 11's 7 px invisible border.
        pub fn with_window(mut self, hwnd: isize) -> Self {
            self.windows.insert(
                hwnd,
                WindowState {
                    title: "Notes".to_string(),
                    outer: [93, 100, 907, 707],
                    frame: [100, 100, 900, 700],
                    work: [0, 0, 1920, 1032],
                    placement: Placement {
                        show: 1,
                        normal: [93, 100, 907, 707],
                        ..Placement::default()
                    },
                    ..WindowState::default()
                },
            );
            self.target = Some(hwnd);
            self
        }

        fn refusal(&self) -> Result<(), String> {
            if self.refuse {
                Err(refused())
            } else {
                Ok(())
            }
        }
    }

    impl System for Fake {
        fn read_dword(&self, key: &str, name: &str) -> Option<u32> {
            self.registry
                .get(&(key.to_string(), name.to_string()))
                .copied()
        }

        fn write_dword(&mut self, key: &str, name: &str, value: u32) -> Result<(), String> {
            self.set_dword(key, name, value);
            Ok(())
        }

        fn announce(&mut self, area: &str) {
            self.log.push(format!("announce {area}"));
        }

        fn refresh_explorer(&mut self) {
            self.log.push("refresh explorer".to_string());
        }

        fn taskbar_state(&self) -> Option<u32> {
            Some(self.taskbar)
        }

        fn set_taskbar_state(&mut self, state: u32) -> Result<(), String> {
            self.taskbar = state;
            Ok(())
        }

        fn desktop_icons_shown(&self) -> Option<bool> {
            self.icons
        }

        fn toggle_desktop_icons(&mut self) -> Result<(), String> {
            self.log.push("toggle desktop icons".to_string());
            if !self.icons_ignore {
                self.icons = self.icons.map(|shown| !shown);
            }
            Ok(())
        }

        fn target_window(&self) -> Option<isize> {
            self.target
        }

        fn window(&self, hwnd: isize) -> Option<WindowState> {
            self.windows.get(&hwnd).cloned()
        }

        fn set_topmost(&mut self, hwnd: isize, on: bool) -> Result<(), String> {
            self.refusal()?;
            self.log.push(format!("topmost {on}"));
            if let Some(window) = self.windows.get_mut(&hwnd) {
                window.topmost = on;
            }
            Ok(())
        }

        fn restore(&mut self, hwnd: isize) {
            self.log.push("restore".to_string());
            if let Some(window) = self.windows.get_mut(&hwnd) {
                window.maximized = false;
                window.minimized = false;
                window.outer = window.placement.normal;
                let n = window.placement.normal;
                window.frame = [n[0] + 7, n[1], n[2] - 7, n[3] - 7];
            }
        }

        fn move_window(&mut self, hwnd: isize, outer: Rect) -> Result<(), String> {
            self.refusal()?;
            self.log.push(format!("move {outer:?}"));
            if let Some(window) = self.windows.get_mut(&hwnd) {
                window.outer = outer;
                window.frame = [outer[0] + 7, outer[1], outer[2] - 7, outer[3] - 7];
            }
            Ok(())
        }

        fn set_placement(&mut self, hwnd: isize, placement: &Placement) -> Result<(), String> {
            self.refusal()?;
            self.log.push(format!("placement {:?}", placement.normal));
            if let Some(window) = self.windows.get_mut(&hwnd) {
                window.placement = *placement;
                window.outer = placement.normal;
                window.maximized = placement.show == 3;
            }
            Ok(())
        }

        fn keep_awake(&mut self, on: bool) -> Result<(), String> {
            self.log.push(format!("awake {on}"));
            self.awake = on;
            Ok(())
        }

        fn local_addresses(&self) -> Vec<Address> {
            self.addresses.clone()
        }

        fn flush_dns(&mut self) -> Result<(), String> {
            self.log.push("flush dns".to_string());
            Ok(())
        }

        fn clipboard_text(&self) -> Option<String> {
            self.clipboard.clone()
        }

        fn put_clipboard(&mut self, text: &str) -> Result<(), String> {
            self.log.push(format!("clipboard {text}"));
            self.clipboard = Some(text.to_string());
            Ok(())
        }

        fn random(&self) -> Result<[u8; 16], String> {
            Ok(*b"0123456789abcdef")
        }

        fn microphone(&self) -> Result<(bool, String), String> {
            self.microphone
                .map(|muted| (muted, "Headset".to_string()))
                .ok_or_else(|| "No microphone is connected".to_string())
        }

        fn set_microphone_muted(&mut self, muted: bool) -> Result<(), String> {
            self.log.push(format!("mic muted {muted}"));
            self.microphone = Some(muted);
            Ok(())
        }

        fn recycle_bin(&self) -> Option<(u64, u64)> {
            self.bin
        }

        fn empty_recycle_bin(&mut self) -> Result<bool, String> {
            self.log.push("empty bin".to_string());
            self.bin = Some((0, 0));
            Ok(true)
        }

        fn open_environment_variables(&mut self) -> Result<(), String> {
            self.log.push("environment variables".to_string());
            Ok(())
        }
    }

    #[test]
    fn a_test_instance_remembers_its_pretend_changes_and_keeps_the_real_ones() {
        let mut real = Fake::default().with_window(7);
        real.set_dword("k", "v", 1);
        let mut copy = ReadOnly::new(real);
        copy.write_dword("k", "v", 0).expect("pretend");
        assert_eq!(copy.read_dword("k", "v"), Some(0));
        assert_eq!(copy.inner.read_dword("k", "v"), Some(1), "the real value");
        copy.set_taskbar_state(0x3).expect("pretend");
        assert_eq!(copy.taskbar_state(), Some(0x3));
        assert_eq!(copy.inner.taskbar, 0x2);
        copy.toggle_desktop_icons().expect("pretend");
        assert_eq!(copy.desktop_icons_shown(), Some(false));
        assert_eq!(copy.inner.icons, Some(true));
        copy.announce("ImmersiveColorSet");
        copy.refresh_explorer();
        copy.set_topmost(7, true).expect("pretend");
        copy.restore(7);
        copy.move_window(7, [0, 0, 10, 10]).expect("pretend");
        copy.set_placement(7, &Placement::default())
            .expect("pretend");
        copy.keep_awake(true).expect("pretend");
        copy.flush_dns().expect("pretend");
        copy.put_clipboard("x").expect("pretend");
        copy.set_microphone_muted(true).expect("pretend");
        assert_eq!(copy.microphone(), Ok((true, "Headset".to_string())));
        assert_eq!(copy.inner.microphone, Some(false));
        assert_eq!(copy.empty_recycle_bin(), Ok(true));
        copy.open_environment_variables().expect("pretend");
        assert!(copy.inner.log.is_empty(), "{:?}", copy.inner.log);
        assert!(copy.random().is_ok());
        assert!(copy.local_addresses().is_empty());
        assert!(copy.clipboard_text().is_none());
        assert_eq!(copy.recycle_bin(), Some((0, 0)));
        assert_eq!(copy.target_window(), Some(7));
        assert_eq!(copy.window(7).map(|w| w.outer), Some([93, 100, 907, 707]));
    }
}
