use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::identity::{Rect, WindowIdentity};
use crate::core::config;

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateFile {
    pub version: u32,
    pub saved: String,
    pub reason: String,
    pub programs: Programs,
}

pub type Programs = BTreeMap<String, ProgramState>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgramState {
    pub windows: Vec<SavedWindow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub title: String,
    pub rect: Rect,
    pub maximized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_name: Option<String>,
    /// Position in the taskbar group, first thumbnail = 0.
    pub taskbar_index: usize,
    /// Position among the program's windows from the top of the z-order.
    pub z_index: usize,
}

impl SavedWindow {
    pub fn identity(&self, exe: &str) -> WindowIdentity {
        WindowIdentity {
            exe: exe.to_string(),
            name: self.name.clone(),
            title: self.title.clone(),
            rect: self.rect,
            maximized: self.maximized,
        }
    }
}

/// Settings stay in layout_keeper.json; this file is written by the plugin
/// alone and changes every time the layout does.
pub fn path() -> PathBuf {
    config::plugins_dir().join("layout_keeper.state.json")
}

pub fn load() -> Option<StateFile> {
    let path = path();
    let text = fs::read_to_string(&path).ok()?;
    match serde_json::from_str(config::strip_bom(&text)) {
        Ok(state) => Some(state),
        Err(err) => {
            log::warn!(
                target: "layout_keeper",
                "{} cannot be read ({err}); the next snapshot replaces it",
                path.display()
            );
            None
        }
    }
}

/// Folds a fresh look at the windows into what was saved before.
///
/// A program with no windows keeps its saved list: Chrome closed before a
/// reboot is exactly the case the file exists for. At shutdown the programs
/// are closing their windows while the snapshot runs, so a list that shrank
/// keeps the earlier, complete one.
pub fn merge(previous: Option<&Programs>, fresh: Programs, shutdown: bool) -> Programs {
    let mut merged = Programs::new();
    for (exe, state) in fresh {
        let before = previous.and_then(|programs| programs.get(&exe));
        let keep_before = match before {
            Some(before) if state.windows.is_empty() => !before.windows.is_empty(),
            Some(before) if shutdown => state.windows.len() < before.windows.len(),
            _ => false,
        };
        let chosen = match (keep_before, before) {
            (true, Some(before)) => before.clone(),
            _ => state,
        };
        if !chosen.windows.is_empty() {
            merged.insert(exe, chosen);
        }
    }
    merged
}

/// Remembers what it last wrote so an unchanged layout costs no disk write.
#[derive(Default)]
pub struct Writer {
    last: Option<Programs>,
}

impl Writer {
    pub fn with_last(last: Option<Programs>) -> Self {
        Self { last }
    }

    pub fn last(&self) -> Option<&Programs> {
        self.last.as_ref()
    }

    /// Returns whether anything was written.
    pub fn write(
        &mut self,
        programs: Programs,
        reason: &str,
        saved: String,
    ) -> Result<bool, String> {
        if self.last.as_ref() == Some(&programs) {
            return Ok(false);
        }
        let file = StateFile {
            version: VERSION,
            saved,
            reason: reason.to_string(),
            programs,
        };
        let text = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("cannot serialise the layout: {e}"))?;
        config::write_atomic(&path(), &text)?;
        self.last = Some(file.programs);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, index: usize) -> SavedWindow {
        SavedWindow {
            name: Some(title.to_string()),
            title: title.to_string(),
            rect: [-11, -11, 3851, 2099],
            maximized: true,
            desktop: Some("{1150CF3A-755E-4731-B223-B07262115AD0}".to_string()),
            desktop_name: Some("Work&Study AI".to_string()),
            taskbar_index: index,
            z_index: index,
        }
    }

    fn programs(titles: &[&str]) -> Programs {
        let windows = titles
            .iter()
            .enumerate()
            .map(|(i, t)| window(t, i))
            .collect();
        Programs::from([("chrome.exe".to_string(), ProgramState { windows })])
    }

    #[test]
    fn the_state_file_round_trips_in_the_documented_shape() {
        let file = StateFile {
            version: VERSION,
            saved: "2026-09-24T21:40:11Z".to_string(),
            reason: "shutdown".to_string(),
            programs: programs(&["Dev Sandbox Workspace"]),
        };
        let text = serde_json::to_string(&file).unwrap();
        assert!(text.contains(r#""rect":[-11,-11,3851,2099]"#), "{text}");
        assert!(text.contains(r#""taskbar_index":0"#));
        let back: StateFile = serde_json::from_str(&text).unwrap();
        assert_eq!(back, file);

        let unnamed = r#"{"title":"x - Google Chrome","rect":[0,0,1,1],"maximized":false,
            "taskbar_index":2,"z_index":0}"#;
        let window: SavedWindow = serde_json::from_str(unnamed).unwrap();
        assert_eq!(window.name, None);
        assert_eq!(window.desktop, None);
    }

    #[test]
    fn a_closed_program_keeps_its_saved_windows() {
        let before = programs(&["A", "B"]);
        let fresh = Programs::from([("chrome.exe".to_string(), ProgramState::default())]);
        assert_eq!(merge(Some(&before), fresh, false), before);
    }

    #[test]
    fn a_list_that_shrinks_at_shutdown_keeps_the_complete_one() {
        let before = programs(&["A", "B", "C"]);
        assert_eq!(merge(Some(&before), programs(&["A"]), true), before);
    }

    #[test]
    fn a_window_closed_during_the_day_is_forgotten() {
        let before = programs(&["A", "B", "C"]);
        let fresh = programs(&["A"]);
        assert_eq!(merge(Some(&before), fresh.clone(), false), fresh);
    }

    #[test]
    fn a_program_never_seen_with_windows_is_left_out() {
        let fresh = Programs::from([("notepad.exe".to_string(), ProgramState::default())]);
        assert!(merge(None, fresh, false).is_empty());
    }
}
