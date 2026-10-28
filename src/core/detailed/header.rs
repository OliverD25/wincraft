//! The session header: what a reader of a detailed log needs to know about
//! the PC before the first event. Written when the log turns on and at
//! every start while it is on. Gathered on a thread of its own, since
//! listing the processes takes a moment. Everything here only reads.

use std::path::Path;

use windows_sys::Win32::System::ProcessStatus::K32EnumProcesses;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA};

use crate::core::{monitor_list, virtual_desktops, windows_list};

/// Programs known to watch or change the keyboard, by the start of their
/// file name (lower case). A low-level keyboard hook in one of them can
/// take a key before WinCraft's.
pub const KEYBOARD_TOOLS: &[&str] = &[
    "autohotkey",
    "powertoys",
    "kanata",
    "keytweak",
    "flow.launcher",
    "listary",
    "keypirinha",
    "wox",
    "ueli",
    "displayfusion",
    "logioptionsplus",
    "razer",
    "icue",
    "steelseriesgg",
    "translucenttb",
    "startallback",
    "explorerpatcher",
    "windhawk",
    "stardock",
    "fences",
    "groupy",
    "everything",
    "ditto",
    "sharex",
    "greenshot",
    "keyboard",
    "hotkey",
];

/// Writes the header on a thread of its own. `plugins` are the plugins on.
pub fn write_later(plugins: Vec<String>) {
    let started = std::thread::Builder::new()
        .name("detailed header".to_string())
        .spawn(move || {
            for line in lines(&plugins) {
                crate::detailed!("header: {line}");
            }
        });
    if started.is_err() {
        crate::detailed!("header: could not be gathered");
    }
}

fn lines(plugins: &[String]) -> Vec<String> {
    let mut lines = vec![
        format!("WinCraft {}", env!("CARGO_PKG_VERSION")),
        format!(
            "Windows build {}",
            virtual_desktops::os_build().map_or("unknown".to_string(), |build| build.to_string())
        ),
        format!("plugins on: {}", plugins.join(", ")),
        format!("taskbar: {}", taskbar_side()),
    ];
    for monitor in monitor_list::list() {
        let (handle, rect, work, primary) = monitor.parts();
        let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
        unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let id = monitor.id();
        lines.push(format!(
            "monitor {} \"{}\"{}: {rect:?}, work area {work:?}, DPI {dpi_x}",
            id.device,
            id.name,
            if primary { " (primary)" } else { "" },
        ));
    }
    let processes = process_names();
    let tools = keyboard_tools(&processes);
    lines.push(format!(
        "keyboard tools running: {}",
        if tools.is_empty() {
            "none known".to_string()
        } else {
            tools.join(", ")
        }
    ));
    for script in autohotkey_scripts() {
        lines.push(format!("AutoHotkey script: {script}"));
    }
    if let Some(modules) = powertoys_modules() {
        lines.push(format!("PowerToys modules on: {modules}"));
    }
    lines
}

/// Which edge of the main monitor the taskbar is on.
fn taskbar_side() -> &'static str {
    let mut data: APPBARDATA = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) } == 0 {
        return "unknown";
    }
    match data.uEdge {
        0 => "left",
        1 => "top",
        2 => "right",
        3 => "bottom",
        _ => "unknown",
    }
}

/// The file names of the running processes Windows lets WinCraft read.
fn process_names() -> Vec<String> {
    let mut ids = vec![0u32; 4096];
    let mut bytes = 0u32;
    let size = (ids.len() * std::mem::size_of::<u32>()) as u32;
    if unsafe { K32EnumProcesses(ids.as_mut_ptr(), size, &mut bytes) } == 0 {
        return Vec::new();
    }
    ids.truncate(bytes as usize / std::mem::size_of::<u32>());
    ids.into_iter()
        .filter(|pid| *pid != 0)
        .map(windows_list::exe_path)
        .filter(|path| !path.is_empty())
        .map(|path| path.rsplit('\\').next().unwrap_or("").to_string())
        .collect()
}

/// The keyboard tools among `processes`, each name once, sorted.
pub(crate) fn keyboard_tools(processes: &[String]) -> Vec<String> {
    let mut found: Vec<String> = processes
        .iter()
        .filter(|name| {
            let lower = name.to_lowercase();
            KEYBOARD_TOOLS.iter().any(|tool| lower.starts_with(tool))
        })
        .cloned()
        .collect();
    found.sort_by_key(|name| name.to_lowercase());
    found.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    found
}

/// The scripts AutoHotkey runs: each has a hidden main window of class
/// "AutoHotkey" whose title is the script's path.
fn autohotkey_scripts() -> Vec<String> {
    windows_list::top_windows()
        .into_iter()
        .filter(|hwnd| windows_list::class_name(*hwnd) == "AutoHotkey")
        .map(|hwnd| script_of_title(&windows_list::window_text(hwnd)))
        .collect()
}

/// "C:\x.ahk - AutoHotkey v2.0.2" → "C:\x.ahk".
pub(crate) fn script_of_title(title: &str) -> String {
    match title.rfind(" - AutoHotkey") {
        Some(at) => title[..at].to_string(),
        None => title.to_string(),
    }
}

/// The modules switched on in PowerToys' own settings file, when there is
/// one.
fn powertoys_modules() -> Option<String> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let path = Path::new(&base).join(r"Microsoft\PowerToys\settings.json");
    let text = std::fs::read_to_string(path).ok()?;
    Some(enabled_modules(&text))
}

pub(crate) fn enabled_modules(settings: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(settings) else {
        return "settings unreadable".to_string();
    };
    let Some(enabled) = value.get("enabled").and_then(|value| value.as_object()) else {
        return "none listed".to_string();
    };
    let on: Vec<&str> = enabled
        .iter()
        .filter(|(_, value)| value.as_bool() == Some(true))
        .map(|(name, _)| name.as_str())
        .collect();
    if on.is_empty() {
        "none".to_string()
    } else {
        on.join(", ")
    }
}
