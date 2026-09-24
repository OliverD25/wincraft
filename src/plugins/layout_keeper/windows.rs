use std::collections::HashMap;

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWM_CLOAKED_APP};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongW, GetWindowPlacement, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed, GWL_EXSTYLE,
    GW_OWNER, SW_SHOWMAXIMIZED, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

use super::identity::WindowIdentity;

/// Explorer owns the desktop and the taskbars as well as its folder windows.
/// None of them has a taskbar button.
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

pub struct LiveWindow {
    pub hwnd: HWND,
    pub identity: WindowIdentity,
}

/// The watched programs' windows that have a taskbar button, top of the
/// z-order first, across every virtual desktop.
pub fn enumerate(programs: &[String]) -> Vec<LiveWindow> {
    let mut candidates: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        let found = unsafe { &mut *(found as *mut Vec<HWND>) };
        found.push(hwnd);
        1
    }
    unsafe { EnumWindows(Some(collect), &mut candidates as *mut Vec<HWND> as LPARAM) };

    let mut exe_of_pid: HashMap<u32, String> = HashMap::new();
    let mut windows = Vec::new();
    for hwnd in candidates {
        if !has_taskbar_button(hwnd) {
            continue;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let exe = exe_of_pid
            .entry(pid)
            .or_insert_with(|| exe_name(pid))
            .clone();
        if !programs.iter().any(|program| *program == exe) {
            continue;
        }
        let title = window_text(hwnd);
        if title.is_empty() || SHELL_CLASSES.contains(&class_name(hwnd).as_str()) {
            continue;
        }
        let (rect, maximized) = placement(hwnd);
        windows.push(LiveWindow {
            hwnd,
            identity: WindowIdentity::new(&exe, &title, rect, maximized),
        });
    }
    windows
}

/// The taskbar's own rule: visible, not a tool window, and either unowned or
/// explicitly asking for a button. Windows on other virtual desktops are
/// cloaked by the shell and still count; windows an app cloaked itself do not.
fn has_taskbar_button(hwnd: HWND) -> bool {
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return false;
    }
    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    if ex_style & WS_EX_TOOLWINDOW != 0 {
        return false;
    }
    let owned = !unsafe { GetWindow(hwnd, GW_OWNER) }.is_null();
    if owned && ex_style & WS_EX_APPWINDOW == 0 {
        return false;
    }
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    cloaked & DWM_CLOAKED_APP == 0
}

/// Lower-case file name of the process's image, like "chrome.exe".
pub fn exe_name(pid: u32) -> String {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return String::new();
    }
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
    };
    unsafe { CloseHandle(process) };
    if ok == 0 {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    path.rsplit('\\').next().unwrap_or("").to_lowercase()
}

pub fn window_text(hwnd: HWND) -> String {
    let mut buffer = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
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
