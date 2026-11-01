//! The running app's windows counted per virtual desktop: the inspector's
//! summary and Ctrl+D's view.

use std::collections::HashMap;

use super::{title_matches, window_row, RunningApp};
use crate::core::virtual_desktops::{Desktop, DesktopId};
use crate::search::{Action, Choice, ResultItem};

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
