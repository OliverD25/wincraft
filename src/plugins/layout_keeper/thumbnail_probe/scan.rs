//! The Win32 and UI Automation reads behind one probe tick. Everything here
//! only reads: no window is moved, shown, activated or sent a message.

use std::collections::{HashMap, HashSet};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetAncestor, GetCursorPos, GetWindow, GetWindowLongW,
    GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, WindowFromPoint, GA_ROOT,
    GWL_EXSTYLE, GWL_STYLE, GW_OWNER, WS_EX_TOPMOST,
};

use super::super::{state, windows};
use super::geometry::{self, Rect};
use super::report::{
    is_likely, pick_popup, popup_rects, ChildWindow, CursorSample, Frame, Group, Popup, Sample,
    UiaNode, WindowInfo,
};
use crate::core::com::ComPtr;
use crate::core::taskbar;
use crate::core::uia::{self, Automation};
use crate::core::windows_list::{class_name, cloak, exe_path, raw_window_text};

/// Explorer's own windows that are always there and are never the popup.
const EXPLORER_FIXED: &[&str] = &[
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Progman",
    "WorkerW",
    "CabinetWClass",
];
const TASKBARS: &[&str] = &["Shell_TrayWnd", "Shell_SecondaryTrayWnd"];
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 400;
const MAX_CHILDREN: usize = 40;
const MAX_PARENTS: usize = 6;

pub struct Scanner {
    automation: Result<Automation, String>,
    own_pid: u32,
    baseline: HashSet<isize>,
    exe_of_pid: HashMap<u32, String>,
    last_taskbar_point: Option<(i32, i32)>,
    hovered_app: Option<String>,
    group_count: Option<usize>,
}

impl Scanner {
    /// Call on a thread that started COM itself. The windows shown now are
    /// the baseline: a popup is expected to appear after it. A cloaked
    /// window is not shown, because Explorer may keep a closed popup
    /// visible and only cloak it.
    pub fn new() -> Self {
        let mut scanner = Self {
            automation: Automation::new(),
            own_pid: std::process::id(),
            baseline: HashSet::new(),
            exe_of_pid: HashMap::new(),
            last_taskbar_point: None,
            hovered_app: None,
            group_count: None,
        };
        scanner.baseline = top_level_windows()
            .into_iter()
            .filter(|hwnd| unsafe { IsWindowVisible(*hwnd) } != 0 && cloak(*hwnd) == 0)
            .map(|hwnd| hwnd as isize)
            .collect();
        scanner
    }

    pub fn automation_note(&self) -> String {
        match &self.automation {
            Ok(_) => "UI Automation started".to_string(),
            Err(reason) => format!("UI Automation could not start: {reason}"),
        }
    }

    /// One look at the screen. Returns the samples, and the rectangles the
    /// mouse hook should count as inside a popup.
    pub fn tick(&mut self) -> (Vec<Sample>, Vec<Rect>) {
        let mut samples = Vec::new();
        let cursor = cursor_pos();
        let under = cursor.map(|(x, y)| self.under(x, y));
        if let (Some((x, y)), Some((_, root_class, _))) = (cursor, &under) {
            if TASKBARS.contains(&root_class.as_str()) && self.last_taskbar_point != Some((x, y)) {
                self.last_taskbar_point = Some((x, y));
                if let Some(app) = taskbar::app_at(x, y) {
                    if self.hovered_app.as_deref() != Some(app.as_str()) {
                        let group = group_of(&app);
                        self.group_count = Some(group.windows.len());
                        self.hovered_app = Some(app);
                        samples.push(Sample::Group(group));
                    }
                }
            }
        }

        let popups = self.popups();
        let grid = pick_popup(&popups).and_then(|popup| {
            self.group_count
                .and_then(|count| geometry::slot_grid(popup.window.rect, count, popup.window.dpi))
        });
        let rects = popup_rects(&popups);

        if let (Some((x, y)), Some((class, root_class, process))) = (cursor, under) {
            let over_popup = rects.iter().any(|rect| geometry::contains(*rect, x, y));
            let chain = (over_popup || TASKBARS.contains(&root_class.as_str()))
                .then(|| self.chain_at(x, y));
            samples.push(Sample::Cursor(CursorSample {
                point: (x, y),
                under: format!("{class} / {root_class} / {process}"),
                chain,
                slot: grid.and_then(|grid| grid.slot_at(x, y)),
            }));
        }
        samples.push(Sample::Frame(Frame { popups, grid }));
        (samples, rects)
    }

    fn process_of(&mut self, hwnd: HWND) -> (u32, String) {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        let name = self
            .exe_of_pid
            .entry(pid)
            .or_insert_with(|| windows::file_name(&exe_path(pid)))
            .clone();
        (pid, name)
    }

    /// The class of the window under a point, its root's class, and the
    /// root's program.
    fn under(&mut self, x: i32, y: i32) -> (String, String, String) {
        let hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
        if hwnd.is_null() {
            return ("none".to_string(), "none".to_string(), String::new());
        }
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let (_, process) = self.process_of(root);
        (class_name(hwnd), class_name(root), process)
    }

    /// Explorer's shown top-level windows besides the fixed ones, and any
    /// other program's always-on-top window that appeared after the start.
    fn popups(&mut self) -> Vec<Popup> {
        let mut popups = Vec::new();
        for hwnd in top_level_windows() {
            if unsafe { IsWindowVisible(hwnd) } == 0 {
                continue;
            }
            let rect = window_rect(hwnd);
            if geometry::area(rect) == 0 {
                continue;
            }
            let (pid, process) = self.process_of(hwnd);
            if pid == self.own_pid {
                continue;
            }
            let class = class_name(hwnd);
            let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
            let new_since_start = !self.baseline.contains(&(hwnd as isize));
            let wanted = if process == "explorer.exe" {
                !EXPLORER_FIXED.contains(&class.as_str())
            } else {
                new_since_start && ex_style & WS_EX_TOPMOST != 0
            };
            if !wanted {
                continue;
            }
            let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
            let window = WindowInfo {
                hwnd: hwnd as isize,
                title: raw_window_text(hwnd),
                process,
                rect,
                dpi: monitor_dpi(hwnd),
                style: unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32,
                ex_style,
                owner: owner as isize,
                owner_class: if owner.is_null() {
                    String::new()
                } else {
                    class_name(owner)
                },
                cloak: cloak(hwnd),
                frame_bounds: frame_bounds(hwnd),
                new_since_start,
                children: children(hwnd),
                class,
            };
            let uia = is_likely(&window).then(|| self.tree(hwnd));
            popups.push(Popup { window, uia });
        }
        popups
    }

    fn tree(&self, hwnd: HWND) -> Result<Vec<UiaNode>, String> {
        let automation = self.automation.as_ref().map_err(Clone::clone)?;
        let root = automation.element_from_handle(hwnd)?;
        let mut nodes = Vec::new();
        walk(automation, &root, 0, &mut nodes);
        Ok(nodes)
    }

    fn chain_at(&self, x: i32, y: i32) -> Result<Vec<UiaNode>, String> {
        let automation = self.automation.as_ref().map_err(Clone::clone)?;
        let mut element = automation.element_from_point(x, y)?;
        let mut chain = vec![read_node(&element, 0)];
        for _ in 0..MAX_PARENTS {
            match automation.parent(&element) {
                Some(parent) => {
                    chain.push(read_node(&parent, 0));
                    element = parent;
                }
                None => break,
            }
        }
        Ok(chain)
    }
}

fn walk(automation: &Automation, element: &ComPtr, depth: usize, nodes: &mut Vec<UiaNode>) {
    if nodes.len() >= MAX_NODES {
        return;
    }
    nodes.push(read_node(element, depth));
    if depth >= MAX_DEPTH {
        return;
    }
    let mut child = automation.first_child(element);
    while let Some(current) = child {
        if nodes.len() >= MAX_NODES {
            return;
        }
        walk(automation, &current, depth + 1, nodes);
        child = automation.next_sibling(&current);
    }
}

fn read_node(element: &ComPtr, depth: usize) -> UiaNode {
    UiaNode {
        depth,
        control_type: uia::control_type(element)
            .map(uia::control_type_name)
            .unwrap_or_else(|| "?".to_string()),
        name: uia::name(element).unwrap_or_default(),
        automation_id: uia::automation_id(element).unwrap_or_default(),
        class: uia::class_name(element).unwrap_or_default(),
        rect: uia::bounding_rect(element),
        offscreen: uia::is_offscreen(element),
        native_window: uia::native_window(element),
    }
}

/// The group's windows on the current desktop as LayoutKeeper sees them,
/// and LayoutKeeper's saved order for the group, read from its state file.
fn group_of(app: &str) -> Group {
    // A window the shell cloaked sits on another desktop or is hidden;
    // answering "unknown desktop" leaves both out, which is what the
    // current desktop's popup shows.
    let live = windows::enumerate(&|_| true, &|_| false);
    let windows = live
        .iter()
        .filter(|window| taskbar::same_app(app, &window.group))
        .map(|window| (window.hwnd as isize, windows::window_text(window.hwnd)))
        .collect();
    let mut saved: Vec<(usize, String)> = state::load(&state::path())
        .map(|file| {
            file.programs
                .values()
                .flat_map(|program| program.windows.iter())
                .filter(|window| !window.group.is_empty() && taskbar::same_app(app, &window.group))
                .map(|window| (window.taskbar_index, window.title.clone()))
                .collect()
        })
        .unwrap_or_default();
    saved.sort_by_key(|(index, _)| *index);
    Group {
        app_id: app.to_string(),
        windows,
        saved_order: saved.into_iter().map(|(_, title)| title).collect(),
    }
}

fn cursor_pos() -> Option<(i32, i32)> {
    let mut point = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}

fn top_level_windows() -> Vec<HWND> {
    let mut found: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        let found = unsafe { &mut *(found as *mut Vec<HWND>) };
        found.push(hwnd);
        1
    }
    unsafe { EnumWindows(Some(collect), &mut found as *mut Vec<HWND> as LPARAM) };
    found
}

fn children(parent: HWND) -> Vec<ChildWindow> {
    let mut found: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(hwnd: HWND, found: LPARAM) -> BOOL {
        let found = unsafe { &mut *(found as *mut Vec<HWND>) };
        found.push(hwnd);
        BOOL::from(found.len() < MAX_CHILDREN)
    }
    unsafe {
        EnumChildWindows(
            parent,
            Some(collect),
            &mut found as *mut Vec<HWND> as LPARAM,
        )
    };
    found
        .into_iter()
        .map(|hwnd| ChildWindow {
            class: class_name(hwnd),
            rect: window_rect(hwnd),
            visible: unsafe { IsWindowVisible(hwnd) } != 0,
        })
        .collect()
}

fn to_rect(rect: RECT) -> Rect {
    [rect.left, rect.top, rect.right, rect.bottom]
}

fn window_rect(hwnd: HWND) -> Rect {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return [0; 4];
    }
    to_rect(rect)
}

fn frame_bounds(hwnd: HWND) -> Option<Rect> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut rect as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    };
    (hr >= 0).then(|| to_rect(rect))
}

fn monitor_dpi(hwnd: HWND) -> u32 {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let (mut x, mut y) = (0, 0);
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) } < 0 {
        return 0;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::com;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    /// One tick on the real desktop of the machine running the tests. It
    /// only reads, so it is safe next to the user's own windows; what it
    /// finds depends on the machine, so only the shape is checked.
    #[test]
    fn a_tick_on_the_real_desktop_reads_without_failing() {
        let (samples, rects) = std::thread::spawn(|| {
            let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
            let result = {
                let mut scanner = Scanner::new();
                assert!(scanner.automation_note().starts_with("UI Automation"));
                scanner.tick()
            };
            if com::ok(hr) {
                unsafe { CoUninitialize() };
            }
            result
        })
        .join()
        .unwrap();
        assert!(samples
            .iter()
            .any(|sample| matches!(sample, Sample::Frame(_))));
        assert!(rects.iter().all(|rect| geometry::area(*rect) > 0));
    }

    #[test]
    fn a_group_that_does_not_exist_has_no_windows() {
        let group = group_of("WinCraft.Test.NoSuchApp");
        assert_eq!(group.app_id, "WinCraft.Test.NoSuchApp");
        assert!(group.windows.is_empty());
        assert!(group.saved_order.is_empty());
    }

    #[test]
    fn a_window_that_is_gone_reads_as_empty() {
        assert_eq!(window_rect(std::ptr::null_mut()), [0; 4]);
        assert_eq!(frame_bounds(std::ptr::null_mut()), None);
    }
}
