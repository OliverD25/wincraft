//! What the palette's inspector says about one open window: its program and
//! process id, where it is, how much memory its process uses, and whether it
//! shows on every virtual desktop. Read only; nothing here changes a window.

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

use crate::core::desktop_manager::DesktopManager;
use crate::core::monitors;
use crate::core::virtual_desktops::{self, DesktopId, Pinner};
use crate::core::windows_list;

/// Which virtual desktop a window is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopPlace {
    All,
    /// From 1, in Task View order.
    Number {
        index: usize,
        count: usize,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowDetails {
    /// "claude.exe"; empty when Windows will not say.
    pub process: String,
    pub pid: u32,
    /// The process's working set in bytes.
    pub memory: Option<u64>,
    /// "Left", "Centre", "Right"; None with one monitor.
    pub monitor: Option<String>,
    pub desktop: Option<DesktopPlace>,
    /// None when the shell's pinning interface is not available.
    pub pinned: Option<bool>,
}

impl WindowDetails {
    /// "claude.exe · PID 18240".
    pub fn process_line(&self) -> String {
        match (self.process.is_empty(), self.pid) {
            (true, 0) => String::new(),
            (true, pid) => format!("PID {pid}"),
            (false, 0) => self.process.clone(),
            (false, pid) => format!("{} \u{b7} PID {pid}", self.process),
        }
    }

    /// "Centre · Desktop 2 of 6".
    pub fn place_line(&self) -> String {
        let desktop = self.desktop.map(|place| match place {
            DesktopPlace::All => "All desktops".to_string(),
            DesktopPlace::Number { index, count } => format!("Desktop {index} of {count}"),
        });
        let monitor = self.monitor.clone().or_else(|| {
            // One monitor and one desktop say nothing worth a row.
            desktop.as_ref().map(|_| "Main monitor".to_string())
        });
        [monitor, desktop]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    }
}

/// "612 MB", or "1.4 GB" from 1000 MB up.
pub fn memory_text(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{} MB", mb.round() as u64)
    }
}

/// Where a desktop id falls in the registry's list.
pub fn desktop_place(
    id: DesktopId,
    desktops: &[virtual_desktops::Desktop],
) -> Option<DesktopPlace> {
    if id == DesktopId::ALL {
        return Some(DesktopPlace::All);
    }
    let index = desktops.iter().position(|desktop| desktop.id == id)?;
    Some(DesktopPlace::Number {
        index: index + 1,
        count: desktops.len(),
    })
}

/// Reads everything at once. COM must be running on the calling thread.
pub fn read(hwnd: HWND, pinner: Option<&Pinner>) -> WindowDetails {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let path = windows_list::exe_path(pid);
    let process = path.rsplit('\\').next().unwrap_or_default().to_string();
    let screens = monitors::screens();
    let lefts: Vec<i32> = screens.iter().map(|screen| screen.rect[0]).collect();
    let monitor = monitors::screen_of(hwnd, &screens)
        .and_then(|index| monitors::position_names(&lefts).swap_remove(index));
    let desktops = virtual_desktops::list();
    let desktop = if desktops.len() > 1 {
        DesktopManager::new()
            .ok()
            .and_then(|manager| manager.desktop_of(hwnd))
            .map(|guid| DesktopId::from_guid(&guid))
            .and_then(|id| desktop_place(id, &desktops))
    } else {
        None
    };
    let pinned = pinner.and_then(|pinner| pinner.is_pinned(hwnd).ok());
    WindowDetails {
        process,
        pid,
        memory: working_set(pid),
        monitor,
        desktop,
        pinned,
    }
}

fn working_set(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let mut process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
    if process.is_null() {
        process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    }
    if process.is_null() {
        return None;
    }
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    counters.cb = size;
    let ok = unsafe { K32GetProcessMemoryInfo(process, &mut counters, size) };
    unsafe { CloseHandle(process) };
    (ok != 0).then_some(counters.WorkingSetSize as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_reads_in_megabytes_then_gigabytes() {
        assert_eq!(memory_text(612 * 1024 * 1024), "612 MB");
        assert_eq!(memory_text(1500 * 1024 * 1024), "1.5 GB");
        assert_eq!(memory_text(0), "0 MB");
    }

    #[test]
    fn the_place_names_the_monitor_and_the_desktop() {
        let mut details = WindowDetails {
            monitor: Some("Centre".to_string()),
            desktop: Some(DesktopPlace::Number { index: 2, count: 6 }),
            ..Default::default()
        };
        assert_eq!(details.place_line(), "Centre \u{b7} Desktop 2 of 6");
        details.desktop = Some(DesktopPlace::All);
        assert_eq!(details.place_line(), "Centre \u{b7} All desktops");
        details.monitor = None;
        assert_eq!(details.place_line(), "Main monitor \u{b7} All desktops");
        details.desktop = None;
        assert_eq!(details.place_line(), "");
    }

    #[test]
    fn the_process_line_leaves_out_what_is_unknown() {
        let details = WindowDetails {
            process: "claude.exe".to_string(),
            pid: 18240,
            ..Default::default()
        };
        assert_eq!(details.process_line(), "claude.exe \u{b7} PID 18240");
        assert_eq!(WindowDetails::default().process_line(), "");
    }

    #[test]
    fn a_desktop_is_counted_from_one_in_task_view_order() {
        let desktops = [
            virtual_desktops::Desktop {
                id: DesktopId(1),
                name: "Desktop 1".to_string(),
            },
            virtual_desktops::Desktop {
                id: DesktopId(2),
                name: "Work".to_string(),
            },
        ];
        assert_eq!(
            desktop_place(DesktopId(2), &desktops),
            Some(DesktopPlace::Number { index: 2, count: 2 })
        );
        assert_eq!(
            desktop_place(DesktopId::ALL, &desktops),
            Some(DesktopPlace::All)
        );
        assert_eq!(desktop_place(DesktopId(9), &desktops), None);
    }
}
