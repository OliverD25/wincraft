//! Reads the open app windows for matching: only reads, never changes one.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsIconic};

use crate::core::virtual_desktops::{self, Reader};
use crate::core::window_info::{desktop_place, DesktopPlace};
use crate::core::{appid, monitors, window_names, windows_list};

use super::matching::Live;

/// Whether the window belongs to WinCraft itself, such as the pin list.
pub fn is_own(hwnd: HWND) -> bool {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid == unsafe { GetCurrentProcessId() }
}

/// Every app window Alt+Tab shows, top of the z-order first, without
/// WinCraft's own.
pub fn read() -> Vec<Live> {
    let desktops = virtual_desktops::list();
    let reader = (desktops.len() > 1).then(|| Reader::new().ok()).flatten();
    let known = |hwnd: HWND| {
        reader
            .as_ref()
            .and_then(|reader| reader.read(hwnd))
            .is_some_and(|id| desktops.iter().any(|desktop| desktop.id == id))
    };
    let screens = monitors::screens();
    let lefts: Vec<i32> = screens.iter().map(|screen| screen.rect[0]).collect();
    let names = monitors::position_names(&lefts);
    windows_list::app_window_records(&known)
        .into_iter()
        .filter(|(hwnd, _)| !is_own(*hwnd))
        .map(|(hwnd, record)| {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            let desktop = reader
                .as_ref()
                .and_then(|reader| reader.read(hwnd))
                .and_then(|id| desktop_place(id, &desktops))
                .and_then(|place| match place {
                    DesktopPlace::Number { index, .. } => Some(index),
                    DesktopPlace::All => None,
                });
            Live {
                hwnd: hwnd as isize,
                exe: windows_list::exe_path(pid),
                app_id: appid::read(hwnd).id,
                shown: window_names::display_title(hwnd),
                title: record.title.clone(),
                desktop,
                other_desktop: windows_list::on_other_desktop(&record),
                monitor: monitors::screen_of(hwnd, &screens)
                    .and_then(|index| names.get(index).cloned().flatten()),
                minimized: unsafe { IsIconic(hwnd) } != 0,
            }
        })
        .filter(|live| !live.exe.is_empty())
        .collect()
}
