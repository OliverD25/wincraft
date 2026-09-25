use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

use super::explorer;
use crate::core::desktop_manager::{self, DesktopManager};
use crate::core::windows_list;
use crate::search::{
    self, Action, Choice, Completion, Context, IconRef, Query, Reply, ResultItem, SearchProvider,
};
use crate::ui::fuzzy;

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

/// Top of the z-order first, leaving out WinCraft's own windows.
fn enumerate() -> Vec<OpenWindow> {
    let own = unsafe { GetCurrentProcessId() };
    search::start_com();
    let desktops = DesktopManager::new().ok();
    let on_a_desktop = |hwnd: HWND| {
        desktops
            .as_ref()
            .and_then(|manager| manager.desktop_of(hwnd))
            .is_some_and(|desktop| !desktop_manager::is_null(&desktop))
    };
    let mut exe_of_pid: HashMap<u32, PathBuf> = HashMap::new();
    let mut windows = Vec::new();
    for (hwnd, record) in windows_list::app_window_records(&on_a_desktop) {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == own {
            continue;
        }
        let exe_path = exe_of_pid
            .entry(pid)
            .or_insert_with(|| PathBuf::from(windows_list::exe_path(pid)))
            .clone();
        windows.push(OpenWindow {
            hwnd: hwnd as isize,
            program: program_name(&exe_path),
            exe_path,
            other_desktop: windows_list::on_other_desktop(&record),
            explorer: record.class == EXPLORER_CLASS,
            title: record.title,
        });
    }
    windows
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

    #[test]
    fn the_program_name_drops_the_folder_and_extension() {
        assert_eq!(program_name(Path::new(r"C:\X\Chrome.EXE")), "chrome");
        assert_eq!(program_name(Path::new("")), "");
    }
}
