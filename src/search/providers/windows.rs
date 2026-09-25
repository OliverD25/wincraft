use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, BOOL, GUID, HRESULT};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWM_CLOAKED_APP, DWM_CLOAKED_INHERITED, DWM_CLOAKED_SHELL,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GetClassNameW, GetWindow, GetWindowLongW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};

use super::explorer;
use crate::core::wide;
use crate::search::com::{self, ComPtr};
use crate::search::{
    Action, Choice, Completion, Context, IconRef, Query, Reply, ResultItem, SearchProvider,
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
    /// A File Explorer window, whose folder Tab can browse.
    pub explorer: bool,
}

const EXPLORER_CLASS: &str = "CabinetWClass";
const BROWSE: &str = "browse";

/// The windows Alt+Tab would show, on every virtual desktop, read once when
/// the palette opens.
#[derive(Default)]
pub struct Windows {
    windows: Vec<OpenWindow>,
    /// Explorer windows' folders, looked up when first needed while the
    /// palette is open; None for a folder without a path, like This PC.
    folders: HashMap<isize, Option<String>>,
}

impl Windows {
    fn folder(&mut self, command: &str) -> Option<String> {
        let hwnd: isize = command.strip_prefix(BROWSE)?.trim().parse().ok()?;
        if let Some(known) = self.folders.get(&hwnd) {
            return known.clone();
        }
        let title = self
            .windows
            .iter()
            .find(|window| window.hwnd == hwnd)
            .map(|window| window.title.clone())
            .unwrap_or_default();
        let started = Instant::now();
        let folder = explorer::folder_of(hwnd, &title);
        log::debug!(
            "read the folder of an Explorer window in {:.1} ms: {}",
            started.elapsed().as_secs_f64() * 1000.0,
            folder.as_deref().unwrap_or("none")
        );
        self.folders.insert(hwnd, folder.clone());
        folder
    }
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
        self.folders.clear();
        self.windows = enumerate();
        log::debug!(
            "listed {} open windows in {:.2} ms",
            self.windows.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
    }

    fn available(&mut self, command: &str) -> bool {
        self.folder(command).is_some()
    }

    /// Tab on an Explorer window: browse its folder under the paths prefix,
    /// without bringing the window forward.
    fn act(&mut self, command: &str, _context: &Context) -> Reply {
        match self.folder(command) {
            Some(folder) => Reply::Switch {
                provider: "paths",
                text: if folder.ends_with('\\') {
                    folder
                } else {
                    format!("{folder}\\")
                },
            },
            None => Reply::Stay,
        }
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
            tab_action: window.explorer.then(|| Choice {
                label: "Browse folder".to_string(),
                action: Action::Provider {
                    provider: "windows".to_string(),
                    command: format!("{BROWSE} {}", window.hwnd),
                },
            }),
            ..Default::default()
        })
        .collect()
}

/// What the Alt+Tab rules look at, read from one top-level window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub visible: bool,
    pub ex_style: u32,
    /// None when the window has no owner, else whether its owner is visible.
    pub owner_visible: Option<bool>,
    pub title: String,
    pub class: String,
    /// DWMWA_CLOAKED.
    pub cloak: u32,
    /// Whether the virtual desktop manager names a real desktop for it.
    pub on_a_desktop: bool,
    /// For a Store app's frame: whether its Windows.UI.Core.CoreWindow child
    /// is visible, and that child's cloak flags.
    pub core_window: Option<(bool, u32)>,
}

const FRAME_CLASS: &str = "ApplicationFrameWindow";
const CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

/// The rules Alt+Tab follows. None: Alt+Tab would not show the window.
/// Some(true): it would, and the window is on another virtual desktop.
///
/// The shell cloaks windows on other desktops, and those count. Windows
/// cloaked by their app, or cloaked without belonging to any desktop, are
/// the hidden ones Windows keeps alive, such as a suspended Settings window
/// or Windows Input Experience. A Store app's frame counts only while its
/// app window is inside it.
pub fn alt_tab(window: &Candidate) -> Option<bool> {
    let app_window = window.ex_style & WS_EX_APPWINDOW != 0;
    if !window.visible
        || (window.ex_style & WS_EX_TOOLWINDOW != 0 && !app_window)
        || (window.owner_visible == Some(true) && !app_window)
        || window.title.trim().is_empty()
        || SHELL_CLASSES.contains(&window.class.as_str())
    {
        return None;
    }
    let other_desktop = window.cloak & DWM_CLOAKED_SHELL != 0;
    let cloak = window.cloak & !DWM_CLOAKED_INHERITED;
    if cloak != 0 && !(cloak == DWM_CLOAKED_SHELL && window.on_a_desktop) {
        return None;
    }
    if window.class == FRAME_CLASS {
        match window.core_window {
            Some((true, child_cloak)) if child_cloak & DWM_CLOAKED_APP == 0 => {}
            _ => return None,
        }
    }
    Some(other_desktop)
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
    let desktops = DesktopReader::new();
    let mut exe_of_pid: HashMap<u32, PathBuf> = HashMap::new();
    let mut windows = Vec::new();
    for hwnd in handles {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == own {
            continue;
        }
        let window = candidate(hwnd, desktops.as_ref());
        let Some(other_desktop) = alt_tab(&window) else {
            continue;
        };
        let exe_path = exe_of_pid
            .entry(pid)
            .or_insert_with(|| image_path(pid))
            .clone();
        windows.push(OpenWindow {
            hwnd: hwnd as isize,
            program: program_name(&exe_path),
            exe_path,
            other_desktop,
            explorer: window.class == EXPLORER_CLASS,
            title: window.title,
        });
    }
    windows
}

/// Reads what `alt_tab` needs, asking the slower questions only when the
/// cheap ones have not already ruled the window out.
fn candidate(hwnd: HWND, desktops: Option<&DesktopReader>) -> Candidate {
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    if !visible {
        return Candidate::default();
    }
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
    let cloak = cloak_of(hwnd);
    let class = text_of(hwnd, GetClassNameW);
    let core_window = (class == FRAME_CLASS).then(|| {
        let child = unsafe {
            FindWindowExW(
                hwnd,
                std::ptr::null_mut(),
                wide(CORE_WINDOW_CLASS).as_ptr(),
                std::ptr::null(),
            )
        };
        if child.is_null() {
            (false, 0)
        } else {
            (unsafe { IsWindowVisible(child) } != 0, cloak_of(child))
        }
    });
    Candidate {
        visible,
        ex_style: unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32,
        owner_visible: (!owner.is_null()).then(|| unsafe { IsWindowVisible(owner) } != 0),
        title: text_of(hwnd, GetWindowTextW),
        class,
        cloak,
        on_a_desktop: cloak != 0 && desktops.is_some_and(|reader| reader.on_a_desktop(hwnd)),
        core_window,
    }
}

fn cloak_of(hwnd: HWND) -> u32 {
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

#[repr(C)]
struct IVirtualDesktopManagerVtbl {
    _base: IUnknown_Vtbl,
    _is_window_on_current_virtual_desktop:
        unsafe extern "system" fn(*mut c_void, HWND, *mut BOOL) -> HRESULT,
    get_window_desktop_id: unsafe extern "system" fn(*mut c_void, HWND, *mut GUID) -> HRESULT,
    _move_window_to_desktop: unsafe extern "system" fn(*mut c_void, HWND, *const GUID) -> HRESULT,
}

const CLSID_VIRTUAL_DESKTOP_MANAGER: GUID = com::guid(
    0xaa509086,
    0x5ca9,
    0x4c25,
    [0x8f, 0x95, 0x58, 0x9d, 0x3c, 0x07, 0xb4, 0x8a],
);
const IID_IVIRTUAL_DESKTOP_MANAGER: GUID = com::guid(
    0xa5cd92ff,
    0x29be,
    0x454c,
    [0x8d, 0x04, 0xd8, 0x28, 0x79, 0xfb, 0x3f, 0x1b],
);

/// The documented IVirtualDesktopManager, made once per listing.
struct DesktopReader(ComPtr);

impl DesktopReader {
    fn new() -> Option<Self> {
        ComPtr::create(
            &CLSID_VIRTUAL_DESKTOP_MANAGER,
            &IID_IVIRTUAL_DESKTOP_MANAGER,
        )
        .map(Self)
    }

    /// A window the shell hides without giving it a desktop reads as the
    /// null GUID, or fails.
    fn on_a_desktop(&self, hwnd: HWND) -> bool {
        let mut desktop = com::guid(0, 0, 0, [0; 8]);
        let hr = unsafe {
            (self
                .0
                .vtable::<IVirtualDesktopManagerVtbl>()
                .get_window_desktop_id)(self.0.as_raw(), hwnd, &mut desktop)
        };
        com::ok(hr)
            && (desktop.data1, desktop.data2, desktop.data3, desktop.data4) != (0, 0, 0, [0; 8])
    }
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
            explorer: false,
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

    fn normal(title: &str) -> Candidate {
        Candidate {
            visible: true,
            title: title.to_string(),
            class: "Chrome_WidgetWin_1".to_string(),
            ..Candidate::default()
        }
    }

    #[test]
    fn an_ordinary_window_on_this_desktop_is_listed() {
        assert_eq!(alt_tab(&normal("Inbox")), Some(false));
    }

    #[test]
    fn a_window_on_another_desktop_is_listed_and_marked() {
        let mut window = normal("Notes");
        window.cloak = DWM_CLOAKED_SHELL;
        window.on_a_desktop = true;
        assert_eq!(alt_tab(&window), Some(true));
    }

    #[test]
    fn hidden_system_windows_are_left_out() {
        let mut input = normal("Windows Input Experience");
        input.cloak = DWM_CLOAKED_SHELL;
        input.on_a_desktop = false;
        assert_eq!(alt_tab(&input), None);

        let mut suspended = normal("Settings");
        suspended.cloak = DWM_CLOAKED_APP;
        assert_eq!(alt_tab(&suspended), None);

        let mut both = normal("Settings");
        both.cloak = DWM_CLOAKED_SHELL | DWM_CLOAKED_APP;
        both.on_a_desktop = true;
        assert_eq!(alt_tab(&both), None);
    }

    #[test]
    fn tool_windows_owned_windows_and_untitled_ones_are_left_out() {
        let mut tool = normal("Palette");
        tool.ex_style = WS_EX_TOOLWINDOW;
        assert_eq!(alt_tab(&tool), None);
        tool.ex_style |= WS_EX_APPWINDOW;
        assert_eq!(alt_tab(&tool), Some(false));

        let mut dialog = normal("Save as");
        dialog.owner_visible = Some(true);
        assert_eq!(alt_tab(&dialog), None);
        dialog.owner_visible = Some(false);
        assert_eq!(alt_tab(&dialog), Some(false));

        assert_eq!(alt_tab(&normal("  ")), None);
        let mut hidden = normal("Hidden");
        hidden.visible = false;
        assert_eq!(alt_tab(&hidden), None);
    }

    #[test]
    fn a_store_app_frame_needs_its_app_window_inside() {
        let mut frame = normal("Settings");
        frame.class = FRAME_CLASS.to_string();
        assert_eq!(alt_tab(&frame), None);
        frame.core_window = Some((false, 0));
        assert_eq!(alt_tab(&frame), None);
        frame.core_window = Some((true, DWM_CLOAKED_APP));
        assert_eq!(alt_tab(&frame), None);
        frame.core_window = Some((true, 0));
        assert_eq!(alt_tab(&frame), Some(false));
    }

    #[test]
    fn the_program_name_drops_the_folder_and_extension() {
        assert_eq!(program_name(Path::new(r"C:\X\Chrome.EXE")), "chrome");
        assert_eq!(program_name(Path::new("")), "");
    }
}
