//! The open windows of the best-match app, nested under its row (design
//! "Open or Switch Options", option 3a). Enter on the app still opens a new
//! instance; Shift+Enter, ↓, Alt+1 to Alt+9 and Ctrl+D reach the windows
//! that are open. Words typed after the app's name filter the windows by
//! title. Nothing here moves or closes a window: it only builds rows, and a
//! row's action runs when the user picks it (Principle 1).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::virtual_desktops::{Desktop, DesktopId};
use crate::search::providers::apps::{self, App};
use crate::search::providers::windows::OpenWindow;
use crate::search::{Action, Choice, IconRef, Nested, ResultItem};
use crate::ui::palette::actions::{Hint, Key, RowAction, Run};

/// Windows shown under the app row while the list is collapsed.
pub const SHOWN: usize = 3;
/// With fewer windows than this, all of them show and there is no "more" row.
pub const ALL_UNDER: usize = 5;
/// Rows the expanded list shows before it scrolls inside itself.
pub const EXPANDED_ROWS: usize = 8;
/// Alt+1 to Alt+9.
pub const ALT_KEYS: usize = 9;

/// Apps that keep one window and bring it forward instead of opening a
/// second one, by the start of their app ID (lower case). For these, Enter
/// switches. Only apps known for sure are here.
const ONE_WINDOW_APPS: &[&str] = &[
    "windows.immersivecontrolpanel_cw5n1h2txyewy!",
    "microsoft.windowsstore_8wekyb3d8bbwe!",
];

/// What the palette keeps about running apps during one opening.
#[derive(Default)]
pub struct Running {
    /// The app whose windows are nested under the first row now.
    pub app: Option<RunningApp>,
    /// Its desktop summary, for the inspector.
    pub summary: Option<DeskSummary>,
    /// The key of the app whose nested list is expanded.
    pub expanded: Option<String>,
    /// Ctrl+D's view, while it is open.
    pub by_desktop: Option<ByDesktop>,
    /// Each window's desktop, read once per opening.
    pub places: HashMap<isize, Option<DesktopId>>,
    /// The desktops in Task View order and the current one, read once per
    /// opening.
    pub desktops: Option<(Vec<Desktop>, Option<DesktopId>)>,
    /// Apps and windows a test gives in place of the real ones, so a test
    /// never reads or touches the user's windows.
    #[cfg(test)]
    pub fake: Option<(Vec<App>, Vec<OpenWindow>)>,
}

/// Ctrl+D's view of one app's windows.
pub struct ByDesktop {
    /// The app, with every window shown (no filter).
    pub app: RunningApp,
    pub summary: DeskSummary,
    /// The prefix and the text to put back when the view closes.
    pub back: (Option<String>, String),
}

/// The best-match app and its open windows.
#[derive(Clone, Debug, PartialEq)]
pub struct RunningApp {
    /// The app row's usage key, as the router gives it ("apps:…").
    pub key: String,
    pub name: String,
    /// What Enter opens: a new instance.
    pub target: PathBuf,
    /// The app keeps one window, so Enter switches.
    pub single: bool,
    /// Every open window of the app, the one used last first.
    pub windows: Vec<OpenWindow>,
    /// The words typed after the app's name; empty for none.
    pub filter: String,
    /// The places in `windows` of those the filter keeps, in order. Alt+1
    /// is the first of these.
    pub shown: Vec<usize>,
}

impl RunningApp {
    pub fn new(app: &App, key: String, windows: Vec<OpenWindow>, filter: &str) -> Self {
        let shown = windows
            .iter()
            .enumerate()
            .filter(|(_, window)| title_matches(filter, &window.title))
            .map(|(index, _)| index)
            .collect();
        Self {
            key,
            name: app.name.trim().to_string(),
            target: app.target.clone(),
            single: app
                .aumid
                .as_deref()
                .is_some_and(|id| ONE_WINDOW_APPS.iter().any(|start| id.starts_with(start))),
            windows,
            filter: filter.trim().to_string(),
            shown,
        }
    }

    /// The same app with every window shown, for Ctrl+D's view.
    pub fn unfiltered(&self) -> Self {
        Self {
            filter: String::new(),
            shown: (0..self.windows.len()).collect(),
            ..self.clone()
        }
    }

    /// The window Alt+N switches to, N from 1: the Nth on screen.
    pub fn alt_target(&self, n: usize) -> Option<isize> {
        if n == 0 || n > ALT_KEYS {
            return None;
        }
        self.shown.get(n - 1).map(|index| self.windows[*index].hwnd)
    }

    /// Shift+Enter's window: the one used last, or the top match while a
    /// filter is on.
    pub fn switch_target(&self) -> Option<&OpenWindow> {
        self.shown.first().map(|index| &self.windows[*index])
    }

    /// The nested rows show every window the filter keeps.
    pub fn rows_shown(&self, expanded: bool) -> usize {
        if expanded || self.shown.len() < ALL_UNDER {
            self.shown.len()
        } else {
            SHOWN
        }
    }
}

/// Whether a window belongs to the app: by the program's path, by the app
/// ID, or, for a Store app whose windows carry no app ID, by the package
/// folder the program runs from.
pub fn belongs(exe: Option<&str>, aumid: Option<&str>, window: &OpenWindow) -> bool {
    let path = window.exe_path.to_string_lossy().to_lowercase();
    if exe.is_some_and(|exe| !exe.is_empty() && exe == path) {
        return true;
    }
    let Some(aumid) = aumid.filter(|id| !id.is_empty()) else {
        return false;
    };
    if window.group.to_lowercase() == aumid {
        return true;
    }
    in_package(aumid, &path)
}

/// "microsoft.windowsnotepad_8wekyb3d8bbwe!app" runs from
/// "…\windowsapps\microsoft.windowsnotepad_11.2409.9.0_x64__8wekyb3d8bbwe\…".
fn in_package(aumid: &str, path: &str) -> bool {
    let Some((family, _)) = aumid.split_once('!') else {
        return false;
    };
    let Some((name, publisher)) = family.rsplit_once('_') else {
        return false;
    };
    let Some(at) = path.find(r"\windowsapps\") else {
        return false;
    };
    let folder = path[at + r"\windowsapps\".len()..]
        .split('\\')
        .next()
        .unwrap_or_default();
    folder.starts_with(&format!("{name}_")) && folder.ends_with(&format!("_{publisher}"))
}

/// The app's open windows, in the order given (the one used last first).
pub fn windows_of(app: &App, windows: &[OpenWindow]) -> Vec<OpenWindow> {
    windows
        .iter()
        .filter(|window| belongs(app.exe.as_deref(), app.aumid.as_deref(), window))
        .cloned()
        .collect()
}

/// Every word of the filter appears in the title, in any case.
pub fn title_matches(filter: &str, title: &str) -> bool {
    let title = title.to_lowercase();
    filter
        .split_whitespace()
        .all(|word| title.contains(&word.to_lowercase()))
}

/// The first word and the rest, when more than one word was typed.
pub fn split_words(query: &str) -> Option<(&str, &str)> {
    let query = query.trim();
    let (first, rest) = query.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    (!rest.is_empty()).then_some((first, rest))
}

/// What nesting found for the results of one query.
#[derive(Debug, PartialEq)]
pub struct Found {
    pub app: RunningApp,
    /// A row to put first, when the app came from the first word only and
    /// is not in the results yet.
    pub row: Option<ResultItem>,
}

/// The app whose windows nest. The first row, when it is an app with open
/// windows; else, with more than one word typed and no prefix, the first
/// app that matches the first word and has open windows, while the whole
/// query does not match its name. Then the other words filter its windows.
pub fn find(
    items: &[ResultItem],
    query: &str,
    blended: bool,
    index: &[App],
    windows: &[OpenWindow],
) -> Option<Found> {
    if query.trim().is_empty() || windows.is_empty() {
        return None;
    }
    if let Some(top) = items.first().filter(|item| item.provider == "apps") {
        if let Some(app) = top.usage_key.as_deref().and_then(|key| app_of(index, key)) {
            let open = windows_of(app, windows);
            if !open.is_empty() {
                let key = top.usage_key.clone().unwrap_or_default();
                return Some(Found {
                    app: RunningApp::new(app, key, open, ""),
                    row: None,
                });
            }
        }
    }
    if !blended {
        return None;
    }
    let (first, rest) = split_words(query)?;
    for row in apps::search(index, first, 10) {
        let Some(app) = row.usage_key.as_deref().and_then(|key| own_app(index, key)) else {
            continue;
        };
        if crate::ui::fuzzy::score(query.trim(), &app.name).is_some() {
            continue;
        }
        let open = windows_of(app, windows);
        if open.is_empty() {
            continue;
        }
        let key = format!("apps:{}", app.usage_key);
        let mut row = row;
        row.provider = "apps".to_string();
        row.usage_key = Some(key.clone());
        return Some(Found {
            app: RunningApp::new(app, key, open, rest),
            row: Some(row),
        });
    }
    None
}

/// The app a row's router key ("apps:<key>") names.
fn app_of<'a>(index: &'a [App], key: &str) -> Option<&'a App> {
    own_app(index, key.strip_prefix("apps:")?)
}

fn own_app<'a>(index: &'a [App], key: &str) -> Option<&'a App> {
    index.iter().find(|app| app.usage_key == key)
}

/// Puts the app's row first when it came from the first word, gives it its
/// window words and keys, and puts the nested rows under it.
pub fn nest(items: &mut Vec<ResultItem>, found: Found, expanded: bool) -> RunningApp {
    let Found { app, row } = found;
    if let Some(row) = row {
        items.retain(|item| item.usage_key != row.usage_key);
        items.insert(0, row);
    }
    if let Some(top) = items.first_mut() {
        decorate(top, &app);
        let group = top.group.clone();
        let rows = nested_rows(&app, expanded, &group);
        items.splice(1..1, rows);
    }
    app
}

/// The app row's second line, Enter and Shift+Enter, now that windows are
/// open.
pub fn decorate(row: &mut ResultItem, app: &RunningApp) {
    let total = app.windows.len();
    let extra = if !app.filter.is_empty() {
        Some(format!(
            "{} of {total} {} \u{201c}{}\u{201d}",
            app.shown.len(),
            if total == 1 {
                "window matches"
            } else {
                "windows match"
            },
            app.filter
        ))
    } else if total >= ALL_UNDER {
        Some(format!("{total} windows open"))
    } else {
        None
    };
    if let Some(extra) = extra {
        row.subtitle = if row.subtitle.is_empty() {
            extra
        } else {
            format!("{} \u{b7} {extra}", row.subtitle)
        };
    }
    let switch = app.switch_target().map(|window| Choice {
        label: format!("Switch to {}", window.title),
        action: Action::Activate(window.hwnd),
    });
    if app.single {
        if let Some(switch) = switch {
            let open = row.enter.take();
            row.enter = Some(switch);
            row.shift_enter = open.map(|choice| Choice {
                label: "Open".to_string(),
                ..choice
            });
        }
    } else {
        row.shift_enter = switch;
    }
}

/// The rows under the app: its windows (three, then one "N more windows"
/// row, unless expanded or fewer than five), or one line saying no title
/// matches the filter.
pub fn nested_rows(app: &RunningApp, expanded: bool, group: &str) -> Vec<ResultItem> {
    if app.shown.is_empty() {
        return vec![ResultItem {
            group: group.to_string(),
            title: format!(
                "No {} window matches \u{201c}{}\u{201d}",
                app.name, app.filter
            ),
            nested: Some(Nested::NoMatch),
            ..Default::default()
        }];
    }
    let count = app.rows_shown(expanded);
    let mut rows: Vec<ResultItem> = app.shown[..count]
        .iter()
        .enumerate()
        .map(|(order, index)| {
            let mut row = window_row(&app.windows[*index], order, group);
            row.nested = Some(Nested::Window { order });
            row
        })
        .collect();
    let hidden = app.shown.len() - count;
    if hidden > 0 {
        rows.push(ResultItem {
            group: group.to_string(),
            title: format!("{hidden} more windows"),
            subtitle: if app.filter.is_empty() {
                "Enter to expand \u{b7} or keep typing to filter".to_string()
            } else {
                "Enter to expand".to_string()
            },
            nested: Some(Nested::More { hidden }),
            ..Default::default()
        });
    }
    rows
}

/// One open window as a row: Enter switches to it, and its keycap is the
/// Alt key that reaches it.
pub fn window_row(window: &OpenWindow, order: usize, group: &str) -> ResultItem {
    ResultItem {
        group: group.to_string(),
        title: window.title.clone(),
        subtitle: window.program.clone(),
        icon: IconRef::Window {
            hwnd: window.hwnd,
            exe: window.exe_path.clone(),
        },
        hint: if order < ALT_KEYS {
            format!("Alt+{}", order + 1)
        } else {
            String::new()
        },
        open: true,
        enter: Some(Choice {
            label: "Switch to".to_string(),
            action: Action::Activate(window.hwnd),
        }),
        provider: "windows".to_string(),
        ..Default::default()
    }
}

/// Where ↓ (or ↑) goes from `selected`, round the ends, past the line that
/// says no window matches: it can not be picked.
pub fn step(items: &[ResultItem], selected: usize, down: bool) -> usize {
    let count = items.len();
    if count == 0 {
        return 0;
    }
    let mut at = selected.min(count - 1);
    for _ in 0..count {
        at = if down {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        if items[at].nested != Some(Nested::NoMatch) {
            return at;
        }
    }
    selected
}

/// PgDn or PgUp in the expanded list: one page of windows, staying on the
/// windows.
pub fn page(items: &[ResultItem], selected: usize, down: bool) -> usize {
    let windows: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item.nested, Some(Nested::Window { .. })))
        .map(|(at, _)| at)
        .collect();
    let Some(place) = windows.iter().position(|at| *at == selected) else {
        return selected;
    };
    let next = if down {
        (place + EXPANDED_ROWS).min(windows.len() - 1)
    } else {
        place.saturating_sub(EXPANDED_ROWS)
    };
    windows[next]
}

/// One line of the desktop summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskLine {
    /// "Desktop 1 · this one", "Work", "All desktops".
    pub name: String,
    pub count: usize,
    pub current: bool,
    /// The windows on it, as places in the app's window list.
    pub windows: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeskSummary {
    pub windows: usize,
    pub lines: Vec<DeskLine>,
}

impl DeskSummary {
    /// "15 windows on 6 desktops".
    pub fn title(&self) -> String {
        let desktops = self
            .lines
            .iter()
            .filter(|line| line.name != ALL_DESKTOPS && line.name != OTHER)
            .count()
            .max(1);
        format!(
            "{} {} on {desktops} {}",
            self.windows,
            if self.windows == 1 {
                "window"
            } else {
                "windows"
            },
            if desktops == 1 { "desktop" } else { "desktops" }
        )
    }
}

const ALL_DESKTOPS: &str = "All desktops";
const OTHER: &str = "Another desktop";

/// The app's windows counted per desktop: the current desktop first, then
/// the others in Task View order, then windows shown on every desktop.
/// `places` holds each window's desktop, in the app's window order. With
/// no desktop list (one desktop), every window is on this one.
pub fn summary(
    places: &[Option<DesktopId>],
    desktops: &[Desktop],
    current: Option<DesktopId>,
) -> DeskSummary {
    let mut lines: Vec<DeskLine> = Vec::new();
    if desktops.is_empty() {
        lines.push(DeskLine {
            name: "Desktop 1 \u{b7} this one".to_string(),
            count: places.len(),
            current: true,
            windows: (0..places.len()).collect(),
        });
    } else {
        let mut order: Vec<&Desktop> = desktops.iter().collect();
        if let Some(at) = order.iter().position(|desk| Some(desk.id) == current) {
            let here = order.remove(at);
            order.insert(0, here);
        }
        for desk in order {
            let windows: Vec<usize> = places
                .iter()
                .enumerate()
                .filter(|(_, place)| **place == Some(desk.id))
                .map(|(at, _)| at)
                .collect();
            if windows.is_empty() {
                continue;
            }
            let current = Some(desk.id) == current;
            lines.push(DeskLine {
                name: if current {
                    format!("{} \u{b7} this one", desk.name)
                } else {
                    desk.name.clone()
                },
                count: windows.len(),
                current,
                windows,
            });
        }
        let known = |place: &Option<DesktopId>| {
            place.is_some_and(|id| desktops.iter().any(|desk| desk.id == id))
        };
        let everywhere: Vec<usize> = places
            .iter()
            .enumerate()
            .filter(|(_, place)| **place == Some(DesktopId::ALL))
            .map(|(at, _)| at)
            .collect();
        let other: Vec<usize> = places
            .iter()
            .enumerate()
            .filter(|(_, place)| !known(place) && **place != Some(DesktopId::ALL))
            .map(|(at, _)| at)
            .collect();
        for (name, windows) in [(ALL_DESKTOPS, everywhere), (OTHER, other)] {
            if !windows.is_empty() {
                lines.push(DeskLine {
                    name: name.to_string(),
                    count: windows.len(),
                    current: false,
                    windows,
                });
            }
        }
    }
    DeskSummary {
        windows: places.len(),
        lines,
    }
}

/// Ctrl+D's view: the app's windows under one header per desktop, the
/// current one first, "Desktop 1 · this one · 4". Typing filters by title.
/// Alt+N follows the order the windows are listed in the app row.
pub fn by_desktop_rows(app: &RunningApp, summary: &DeskSummary, typed: &str) -> Vec<ResultItem> {
    let mut rows = Vec::new();
    for line in &summary.lines {
        let group = format!("{} \u{b7} {}", line.name, line.count);
        for index in &line.windows {
            let window = &app.windows[*index];
            if !title_matches(typed, &window.title) {
                continue;
            }
            let mut row = window_row(window, *index, &group);
            row.ctrl_enter = Some(Choice {
                label: "Open new window".to_string(),
                action: Action::Open(app.target.clone()),
            });
            rows.push(row);
        }
    }
    rows
}

/// The places in the app's window list of the windows' desktops, read once
/// per window and opening.
pub fn places(
    app: &RunningApp,
    cache: &mut HashMap<isize, Option<DesktopId>>,
    mut read: impl FnMut(isize) -> Option<DesktopId>,
) -> Vec<Option<DesktopId>> {
    app.windows
        .iter()
        .map(|window| {
            *cache
                .entry(window.hwnd)
                .or_insert_with(|| read(window.hwnd))
        })
        .collect()
}

/// Where the cursor is, for the keys and the footer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    App,
    Window,
    More,
    ByDesktop,
}

fn hint(key: &'static str, label: &str) -> Hint {
    Hint {
        key,
        label: label.to_string(),
        accent: false,
    }
}

/// The footer while the cursor is on the app or its windows, as the design
/// has it.
pub fn hints(spot: Spot, app: &RunningApp, expanded: bool, escape: &str) -> Vec<Hint> {
    // Esc folds an expanded list before it closes the palette.
    let escape = if expanded { "Collapse" } else { escape };
    match spot {
        Spot::App => {
            let filtering = !app.filter.is_empty();
            let (enter, shift) = if app.single {
                ("Switch", "Open")
            } else if filtering {
                ("Open new", "Switch to match")
            } else {
                ("Open new", "Switch")
            };
            vec![
                hint("\u{21B5}", enter),
                hint("Shift+\u{21B5}", shift),
                hint(
                    "\u{2193}",
                    if filtering { "Matches" } else { "Open windows" },
                ),
                hint("Ctrl+D", "By desktop"),
                hint("Esc", escape),
            ]
        }
        Spot::Window if expanded => vec![
            hint("\u{21B5}", "Switch"),
            hint("\u{2191}\u{2193}", "Move"),
            hint("PgUp/PgDn", "Page"),
            hint("\u{2190}", "Collapse"),
            hint("Esc", "Collapse"),
        ],
        Spot::Window => vec![
            hint("\u{21B5}", "Switch"),
            hint("\u{2191}", "Back to app"),
            hint("Shift+\u{2192}", "Next monitor"),
            hint("Ctrl+W", "Close window"),
            hint("Esc", escape),
        ],
        Spot::More => vec![
            hint("\u{21B5}", "Expand"),
            hint("\u{2191}\u{2193}", "Move"),
            hint("Ctrl+D", "By desktop"),
            hint("Esc", escape),
        ],
        Spot::ByDesktop => vec![
            Hint {
                key: "Backspace",
                label: "Back".to_string(),
                accent: true,
            },
            hint("\u{21B5}", "Switch"),
            hint("Ctrl+\u{21B5}", "Open new"),
            hint("Ctrl+D", "Back"),
            hint("Esc", "Back"),
        ],
    }
}

/// The selected row's actions, worded for the app and its windows: the
/// app's Enter opens a new window, "N more" expands, a window in Ctrl+D's
/// view opens a new one with Ctrl+Enter.
pub fn adjust(mut actions: Vec<RowAction>, spot: Spot, app: &RunningApp) -> Vec<RowAction> {
    match spot {
        Spot::More => vec![RowAction {
            key: Key::Enter,
            label: "Show all windows".to_string(),
            short: "Expand".to_string(),
            run: Run::Expand,
            listed: true,
            hinted: true,
        }],
        Spot::App => {
            for action in &mut actions {
                match action.key {
                    Key::Enter if !app.single => {
                        action.label = "Open new window".to_string();
                        action.short = "Open new".to_string();
                    }
                    Key::ShiftEnter if !app.single => action.short = "Switch".to_string(),
                    _ => {}
                }
            }
            actions
        }
        Spot::Window => {
            for action in &mut actions {
                if action.key == Key::CtrlW {
                    action.short = "Close window".to_string();
                }
            }
            actions
        }
        Spot::ByDesktop => {
            for action in &mut actions {
                if action.key == Key::CtrlEnter {
                    action.short = "Open new".to_string();
                }
            }
            actions
        }
    }
}

#[cfg(test)]
mod tests;
