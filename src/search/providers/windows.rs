use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

use super::explorer;
use crate::core::desktop_manager::{self, DesktopManager};
use crate::core::{appid, window_names, windows_list};
use crate::search::{
    self, Action, Choice, Completion, Context, IconRef, Query, Reply, ResultItem, SearchProvider,
};
use crate::ui::fuzzy;

#[derive(Clone, Debug, PartialEq)]
pub struct OpenWindow {
    pub hwnd: isize,
    /// What the title bar shows: the user's name for a renamed window.
    pub title: String,
    /// The app's own title when the window has a name from WindowNamer.
    pub app_title: Option<String>,
    pub exe_path: PathBuf,
    /// "chrome", without the folder and ".exe", for matching and display.
    pub program: String,
    pub other_desktop: bool,
    /// A File Explorer window, whose folder Tab can browse.
    pub explorer: bool,
    /// Its taskbar group, as LayoutKeeper keys it: the AppUserModelID, or
    /// the program's path lowercased. Picks are counted per app, not per
    /// title, which changes with every tab.
    pub group: String,
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
            .map(|window| window.app_title.clone().unwrap_or(window.title.clone()))
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
            let by_name = fuzzy::score_command(text, &window.program, &window.title);
            let by_app_title = window
                .app_title
                .as_deref()
                .and_then(|title| fuzzy::score_command(text, &window.program, title));
            by_name.max(by_app_title).map(|score| (score, window))
        })
        .collect();
    found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    found
        .into_iter()
        .take(limit)
        .map(|(score, window)| ResultItem {
            group: "Windows".to_string(),
            title: window.title.clone(),
            subtitle: subtitle(window),
            icon: IconRef::Window {
                hwnd: window.hwnd,
                exe: window.exe_path.clone(),
            },
            score,
            usage_key: Some(window.group.clone()),
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

/// The program, then the app's own title when the user renamed the window,
/// so the window can still be recognised by what the app calls it.
fn subtitle(window: &OpenWindow) -> String {
    let mut parts = vec![window.program.clone()];
    if let Some(title) = &window.app_title {
        parts.push(title.clone());
    }
    if window.other_desktop {
        parts.push("on another desktop".to_string());
    }
    parts.join(" \u{b7} ")
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
        let group = appid::group_key(appid::read(hwnd).id.as_deref(), &exe_path.to_string_lossy());
        let name = window_names::custom(hwnd as isize);
        windows.push(OpenWindow {
            hwnd: hwnd as isize,
            group,
            program: program_name(&exe_path),
            exe_path,
            other_desktop: windows_list::on_other_desktop(&record),
            explorer: record.class == EXPLORER_CLASS,
            title: name.clone().unwrap_or_else(|| record.title.clone()),
            app_title: name.map(|_| record.title),
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
            app_title: None,
            program: program_name(&exe_path),
            exe_path,
            other_desktop: false,
            explorer: false,
            group: format!(r"c:\programs\{exe}.exe"),
        }
    }

    #[test]
    fn a_window_is_counted_by_its_app_not_its_title() {
        let mut first = window("Inbox - Gmail", "chrome");
        first.group = "Chrome.UserData.Profile3".to_string();
        let mut second = window("Some other tab", "chrome");
        second.group = "Chrome.UserData.Profile3".to_string();
        let rows = search(&[first, second], "", 8);
        assert_eq!(rows[0].usage_key, rows[1].usage_key);
        assert_eq!(
            rows[0].usage_key.as_deref(),
            Some("Chrome.UserData.Profile3")
        );
        let plain = search(&[window("Notes", "notepad")], "", 8);
        assert_eq!(
            plain[0].usage_key.as_deref(),
            Some(r"c:\programs\notepad.exe")
        );
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
    fn a_renamed_window_is_found_by_its_name_or_its_app_title() {
        let mut renamed = window("Work notes", "notepad");
        renamed.app_title = Some("todo.txt - Notepad".to_string());
        let windows = [renamed, window("Inbox - Mail", "outlook")];
        let by_name = search(&windows, "work", 8);
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].title, "Work notes");
        let by_app_title = search(&windows, "todo", 8);
        assert_eq!(by_app_title.len(), 1);
        assert_eq!(by_app_title[0].title, "Work notes");
        assert_eq!(
            by_app_title[0].subtitle,
            "notepad \u{b7} todo.txt - Notepad"
        );
    }

    #[test]
    fn the_subtitle_adds_the_app_title_and_the_other_desktop() {
        let mut far = window("Work notes", "notepad");
        far.other_desktop = true;
        assert_eq!(subtitle(&far), "notepad \u{b7} on another desktop");
        far.app_title = Some("todo.txt - Notepad".to_string());
        assert_eq!(
            subtitle(&far),
            "notepad \u{b7} todo.txt - Notepad \u{b7} on another desktop"
        );
        assert_eq!(subtitle(&window("Notes", "notepad")), "notepad");
    }

    #[test]
    fn the_program_name_drops_the_folder_and_extension() {
        assert_eq!(program_name(Path::new(r"C:\X\Chrome.EXE")), "chrome");
        assert_eq!(program_name(Path::new("")), "");
    }
}
