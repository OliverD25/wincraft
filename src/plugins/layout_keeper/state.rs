use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::identity::{Matching, Rect, WindowIdentity};
use crate::core::config;

/// 2 added each window's taskbar group; 1 kept one order per program.
pub const VERSION: u32 = 2;

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
    /// The taskbar group (AppUserModelID or program path). Empty in files
    /// from version 1, which had one order per program.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Position in the taskbar group, first thumbnail = 0.
    pub taskbar_index: usize,
    /// Position among the program's windows from the top of the z-order.
    pub z_index: usize,
}

impl SavedWindow {
    /// The label a person recognises: the name when there is one.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.title)
    }

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
        Ok(state) => Some(migrate(state)),
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

/// Brings an older file up to this version. Version 1 kept one taskbar order
/// per program and did not know the group of each window: its windows keep
/// an empty group and, when restored, join the group of the live window they
/// are matched to. For a program without app IDs of its own that is the
/// group of its path, which is the group Windows gave them.
pub fn migrate(mut file: StateFile) -> StateFile {
    if file.version < VERSION {
        log::info!(
            target: "layout_keeper",
            "the layout file is version {}; reading it as version {VERSION}",
            file.version
        );
        file.version = VERSION;
    }
    file
}

/// The saved windows (sorted by taskbar position) that make up live group
/// `group`, in their saved order: those saved with that group, plus windows
/// from a version 1 file whose matched live window is in it.
pub fn saved_for_group(
    saved: &[SavedWindow],
    matching: &Matching,
    live_groups: &[String],
    group: &str,
) -> Vec<usize> {
    (0..saved.len())
        .filter(|s| {
            if saved[*s].group.is_empty() {
                matching
                    .pairs
                    .iter()
                    .any(|(ms, l)| ms == s && live_groups[*l] == group)
            } else {
                saved[*s].group == group
            }
        })
        .collect()
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
pub struct Writer {
    path: PathBuf,
    last: Option<Programs>,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new(path(), None)
    }
}

impl Writer {
    pub fn new(path: PathBuf, last: Option<Programs>) -> Self {
        Self { path, last }
    }

    pub fn last(&self) -> Option<&Programs> {
        self.last.as_ref()
    }

    /// Returns whether anything was written. `force` writes even an
    /// unchanged layout, for a save the user asked for.
    pub fn write(
        &mut self,
        programs: Programs,
        reason: &str,
        saved: String,
        force: bool,
    ) -> Result<bool, String> {
        if !force && self.last.as_ref() == Some(&programs) {
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
        config::write_atomic(&self.path, &text)?;
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
            group: "Chrome".to_string(),
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
    fn an_asked_for_save_writes_even_an_unchanged_layout() {
        let dir = std::env::temp_dir().join(format!("wincraft-state-{}", std::process::id()));
        let file = dir.join("layout_keeper.state.json");
        let mut writer = Writer::new(file.clone(), None);
        let layout = programs(&["A", "B"]);

        assert_eq!(
            writer.write(layout.clone(), "timer", "t1".into(), false),
            Ok(true)
        );
        assert_eq!(
            writer.write(layout.clone(), "timer", "t2".into(), false),
            Ok(false)
        );
        assert_eq!(
            writer.write(layout.clone(), "manual", "t3".into(), true),
            Ok(true)
        );

        let written: StateFile = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written.reason, "manual");
        assert_eq!(written.saved, "t3");
        assert_eq!(written.programs, layout);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_1_file_is_read_and_its_windows_follow_their_matches() {
        let v1 = r#"{"version":1,"saved":"2026-09-24T21:40:11Z","reason":"timer",
            "programs":{"chrome.exe":{"windows":[
              {"name":"Mail","title":"Mail","rect":[0,0,1,1],"maximized":true,"taskbar_index":0,"z_index":1},
              {"name":"Gemini","title":"Gemini","rect":[0,0,1,1],"maximized":true,"taskbar_index":1,"z_index":0}
            ]}}}"#;
        let file = migrate(serde_json::from_str(v1).unwrap());
        assert_eq!(file.version, VERSION);
        let saved = &file.programs["chrome.exe"].windows;
        assert!(saved.iter().all(|window| window.group.is_empty()));

        // Mail matched a plain Chrome window, Gemini a window of its web app.
        let matching = Matching {
            pairs: vec![(0, 1), (1, 0)],
            unmatched_saved: vec![],
            unmatched_live: vec![],
        };
        let live_groups = ["Chrome._crx_gemini".to_string(), "Chrome".to_string()];
        assert_eq!(
            saved_for_group(saved, &matching, &live_groups, "Chrome"),
            [0]
        );
        assert_eq!(
            saved_for_group(saved, &matching, &live_groups, "Chrome._crx_gemini"),
            [1]
        );
    }

    #[test]
    fn version_2_windows_stay_in_their_saved_group_even_unmatched() {
        let mut saved = vec![window("A", 0), window("B", 1)];
        saved[1].group = "Other".to_string();
        let matching = Matching::default();
        assert_eq!(saved_for_group(&saved, &matching, &[], "Chrome"), [0]);
        assert_eq!(saved_for_group(&saved, &matching, &[], "Other"), [1]);
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
