use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWM_CLOAKED_APP, DWM_CLOAKED_SHELL,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

use crate::search::{
    Action, Choice, Completion, Context, IconRef, Query, ResultItem, SearchProvider,
};
use crate::ui::fuzzy;

/// Explorer's desktop and taskbars are windows with titles too.
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

#[derive(Clone, Debug, PartialEq)]
pub struct OpenWindow {
    pub hwnd: isize,
    pub title: String,
    pub exe_path: PathBuf,
    /// "chrome", without the folder and ".exe", for matching and display.
    pub program: String,
    pub other_desktop: bool,
}

/// The windows that have a taskbar button, on every virtual desktop, read
/// once when the palette opens.
#[derive(Default)]
pub struct Windows {
    windows: Vec<OpenWindow>,
}

impl SearchProvider for Windows {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn name(&self) -> &'static str {
        "Windows"
    }

    fn description(&self) -> &'static str {
        "Switch to an open window, on any desktop"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some("<")
    }

    fn blended(&self) -> bool {
        true
    }

    fn placeholder(&self) -> &'static str {
        "Type part of a window title or program\u{2026}"
    }

    fn opened(&mut self) {
        let started = Instant::now();
        self.windows = enumerate();
        log::debug!(
            "listed {} open windows in {:.2} ms",
            self.windows.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        // Without a prefix the palette blends providers, and a list of every
        // window under an empty search box would bury the commands.
        if query.text.is_empty() && query.prefix.is_none() {
            return Vec::new();
        }
        search(&self.windows, &query.text, query.limit)
    }
}

pub fn search(windows: &[OpenWindow], text: &str, limit: usize) -> Vec<ResultItem> {
    let mut found: Vec<(i32, &OpenWindow)> = windows
        .iter()
        .filter_map(|window| {
            if text.is_empty() {
                return Some((0, window));
            }
            fuzzy::score_command(text, &window.program, &window.title).map(|score| (score, window))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found
        .into_iter()
        .take(limit)
        .map(|(score, window)| ResultItem {
            group: "Windows".to_string(),
            title: window.title.clone(),
            subtitle: if window.other_desktop {
                format!("{} \u{b7} on another desktop", window.program)
            } else {
                window.program.clone()
            },
            icon: IconRef::Window {
                hwnd: window.hwnd,
                exe: window.exe_path.clone(),
            },
            score,
            enter: Some(Choice {
                label: "Switch to".to_string(),
                action: Action::Activate(window.hwnd),
            }),
            tab: Some(Completion::quiet(&window.title)),
            ..Default::default()
        })
        .collect()
}

/// Top of the z-order first, leaving out WinCraft's own windows.
fn enumerate() -> Vec<OpenWindow> {
    let mut handles: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        let found = unsafe { &mut *(found as *mut Vec<HWND>) };
        found.push(hwnd);
        1
    }
    unsafe { EnumWindows(Some(collect), &mut handles as *mut Vec<HWND> as LPARAM) };

    let own = unsafe { GetCurrentProcessId() };
    let mut exe_of_pid: HashMap<u32, PathBuf> = HashMap::new();
    let mut windows = Vec::new();
    for hwnd in handles {
        let Some(cloaked) = taskbar_window(hwnd) else {
            continue;
        };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == own {
            continue;
        }
        let title = text_of(hwnd, GetWindowTextW);
        if title.is_empty() || SHELL_CLASSES.contains(&text_of(hwnd, GetClassNameW).as_str()) {
            continue;
        }
        let exe_path = exe_of_pid
            .entry(pid)
            .or_insert_with(|| image_path(pid))
            .clone();
        windows.push(OpenWindow {
            hwnd: hwnd as isize,
            title,
            program: program_name(&exe_path),
            exe_path,
            other_desktop: cloaked & DWM_CLOAKED_SHELL != 0,
        });
    }
    windows
}

/// The taskbar's own rule: visible, not a tool window, and either unowned or
/// asking for a button. The shell cloaks windows on other virtual desktops
/// and they still count; a window its app cloaked, like a suspended Store
/// app, does not. Returns the cloak flags of a window that qualifies.
fn taskbar_window(hwnd: HWND) -> Option<u32> {
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return None;
    }
    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    if ex_style & WS_EX_TOOLWINDOW != 0 {
        return None;
    }
    let owned = !unsafe { GetWindow(hwnd, GW_OWNER) }.is_null();
    if owned && ex_style & WS_EX_APPWINDOW == 0 {
        return None;
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
    (cloaked & DWM_CLOAKED_APP == 0).then_some(cloaked)
}

fn text_of(hwnd: HWND, read: unsafe extern "system" fn(HWND, *mut u16, i32) -> i32) -> String {
    let mut buffer = [0u16; 512];
    let len = unsafe { read(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn image_path(pid: u32) -> PathBuf {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return PathBuf::new();
    }
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
    };
    unsafe { CloseHandle(process) };
    if ok == 0 {
        return PathBuf::new();
    }
    PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize]))
}

fn program_name(exe: &Path) -> String {
    exe.file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, exe: &str) -> OpenWindow {
        let exe_path = PathBuf::from(format!(r"C:\Programs\{exe}.exe"));
        OpenWindow {
            hwnd: 1,
            title: title.to_string(),
            program: program_name(&exe_path),
            exe_path,
            other_desktop: false,
        }
    }

    #[test]
    fn windows_match_by_title_or_by_program() {
        let windows = [
            window("Inbox - Mail", "outlook"),
            window("README.md - Visual Studio Code", "code"),
        ];
        let by_title = search(&windows, "readme", 8);
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].subtitle, "code");
        let by_program = search(&windows, "outlook", 8);
        assert_eq!(by_program[0].title, "Inbox - Mail");
    }

    #[test]
    fn an_empty_search_lists_every_window_in_order() {
        let windows = [window("A", "a"), window("B", "b")];
        let all = search(&windows, "", 8);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].title, "A");
    }

    #[test]
    fn the_program_name_drops_the_folder_and_extension() {
        assert_eq!(program_name(Path::new(r"C:\X\Chrome.EXE")), "chrome");
        assert_eq!(program_name(Path::new("")), "");
    }
}
