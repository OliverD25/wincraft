use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::identity::{Matching, Rect, WindowIdentity};
use crate::core::config;

/// 3 added each window's monitor and counts `z_index` across all programs;
/// 2 added each window's taskbar group; 1 kept one order per program.
/// Older files need no change to be read: the new fields are optional.
pub const VERSION: u32 = 3;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateFile {
    pub version: u32,
    /// Local time with its UTC offset, "2026-09-25T02:48:11+03:00"; files
    /// from before 0.6.1 have UTC, "2026-09-24T21:40:11Z".
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
    /// Position from the top of the z-order: among all watched windows since
    /// version 3, among the program's own windows before.
    pub z_index: usize,
    /// The monitor the window is on (the one it returns to when minimized).
    /// Recorded for grouping by monitor later; nothing reads it yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<SavedMonitor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedMonitor {
    /// The GDI device name, such as "\\.\DISPLAY1".
    pub device: String,
    /// The name from the monitor itself, such as "DELL U2720Q"; can be empty.
    pub name: String,
    /// Place from the left, 0 first.
    pub position: usize,
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
            group: self.group.clone(),
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
/// A program with no windows keeps its saved list, as long as it is still
/// watched: an app closed before a reboot is exactly the case the file
/// exists for. At shutdown the programs are closing their windows while the
/// snapshot runs, so a list that shrank keeps the earlier, complete one.
pub fn merge(
    previous: Option<&Programs>,
    fresh: Programs,
    shutdown: bool,
    watched: &dyn Fn(&str) -> bool,
) -> Programs {
    let mut merged = Programs::new();
    for (exe, state) in previous.into_iter().flatten() {
        if !fresh.contains_key(exe) && watched(exe) && !state.windows.is_empty() {
            merged.insert(exe.clone(), state.clone());
        }
    }
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

/// For the groups in `held`, the windows saved before replace the fresh ones:
/// a group in the middle of a crash or restart has its windows in a
/// scrambled order, or has too few of them.
pub fn keep_groups(
    previous: Option<&Programs>,
    mut merged: Programs,
    held: &BTreeSet<String>,
) -> Programs {
    if held.is_empty() {
        return merged;
    }
    for program in merged.values_mut() {
        program
            .windows
            .retain(|window| !held.contains(&window.group));
    }
    for (exe, program) in previous.into_iter().flatten() {
        let kept: Vec<SavedWindow> = program
            .windows
            .iter()
            .filter(|window| held.contains(&window.group))
            .cloned()
            .collect();
        if kept.is_empty() {
            continue;
        }
        let entry = merged.entry(exe.clone()).or_default();
        entry.windows.extend(kept);
        entry.windows.sort_by_key(|window| window.taskbar_index);
    }
    merged.retain(|_, program| !program.windows.is_empty());
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

    /// `layout_keeper.state.prev.json`, next to the state file.
    pub fn prev_path(&self) -> PathBuf {
        self.path.with_extension("prev.json")
    }

    /// Copies the state file to `prev_path` before a crash or restart burst
    /// is written over it. Returns false when there is no file yet.
    pub fn keep_previous(&self) -> Result<bool, String> {
        if !self.path.exists() {
            return Ok(false);
        }
        let prev = self.prev_path();
        fs::copy(&self.path, &prev)
            .map(|_| true)
            .map_err(|e| format!("cannot copy the layout to {}: {e}", prev.display()))
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
            monitor: None,
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
    fn the_previous_file_is_kept_next_to_the_state_file() {
        let dir = std::env::temp_dir().join(format!("wincraft-prev-{}", std::process::id()));
        let file = dir.join("layout_keeper.state.json");
        let mut writer = Writer::new(file.clone(), None);
        assert_eq!(
            writer.prev_path(),
            dir.join("layout_keeper.state.prev.json")
        );
        assert_eq!(writer.keep_previous(), Ok(false));

        writer
            .write(programs(&["Before"]), "timer", "t1".into(), false)
            .unwrap();
        assert_eq!(writer.keep_previous(), Ok(true));
        writer
            .write(programs(&["After"]), "timer", "t2".into(), false)
            .unwrap();
        let prev: StateFile =
            serde_json::from_str(&fs::read_to_string(writer.prev_path()).unwrap()).unwrap();
        assert_eq!(prev.programs, programs(&["Before"]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_held_group_keeps_its_saved_windows_and_the_others_update() {
        let mut before = programs(&["A", "B"]);
        let mut app = window("App", 0);
        app.group = "Chrome._crx_app".to_string();
        before.get_mut("chrome.exe").unwrap().windows.push(app);

        // After a restart the Chrome group shows other windows; the app
        // group moved on normally.
        let mut fresh = programs(&["X", "Y", "Z"]);
        let mut moved = window("App moved", 0);
        moved.group = "Chrome._crx_app".to_string();
        fresh
            .get_mut("chrome.exe")
            .unwrap()
            .windows
            .push(moved.clone());

        let held = BTreeSet::from(["Chrome".to_string()]);
        let kept = keep_groups(Some(&before), fresh.clone(), &held);
        let titles: Vec<&str> = kept["chrome.exe"]
            .windows
            .iter()
            .map(|window| window.title.as_str())
            .collect();
        assert_eq!(titles, ["App moved", "A", "B"]);

        assert_eq!(
            keep_groups(Some(&before), fresh.clone(), &BTreeSet::new()),
            fresh
        );
    }

    #[test]
    fn a_version_2_file_reads_as_version_3_without_monitors() {
        let v2 = r#"{"version":2,"saved":"2026-09-24T21:40:11Z","reason":"timer",
            "programs":{"chrome.exe":{"windows":[
              {"name":"Mail","title":"Mail","rect":[0,0,1,1],"maximized":true,
               "group":"Chrome","taskbar_index":0,"z_index":0}
            ]}}}"#;
        let file = migrate(serde_json::from_str(v2).unwrap());
        assert_eq!(file.version, VERSION);
        assert_eq!(file.programs["chrome.exe"].windows[0].monitor, None);

        let mut with_monitor = file.clone();
        with_monitor.programs.get_mut("chrome.exe").unwrap().windows[0].monitor =
            Some(SavedMonitor {
                device: r"\\.\DISPLAY2".to_string(),
                name: "DELL U2720Q".to_string(),
                position: 1,
            });
        let text = serde_json::to_string(&with_monitor).unwrap();
        assert!(text.contains(r#""position":1"#), "{text}");
        let back: StateFile = serde_json::from_str(&text).unwrap();
        assert_eq!(back, with_monitor);
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

    fn every(_: &str) -> bool {
        true
    }

    #[test]
    fn a_closed_program_keeps_its_saved_windows() {
        let before = programs(&["A", "B"]);
        let empty = Programs::from([("chrome.exe".to_string(), ProgramState::default())]);
        assert_eq!(merge(Some(&before), empty, false, &every), before);
        // A closed app is simply missing from the fresh look.
        assert_eq!(merge(Some(&before), Programs::new(), false, &every), before);
    }

    #[test]
    fn a_program_no_longer_watched_is_dropped() {
        let before = programs(&["A", "B"]);
        let not_chrome = |exe: &str| exe != "chrome.exe";
        assert!(merge(Some(&before), Programs::new(), false, &not_chrome).is_empty());
    }

    #[test]
    fn a_list_that_shrinks_at_shutdown_keeps_the_complete_one() {
        let before = programs(&["A", "B", "C"]);
        assert_eq!(merge(Some(&before), programs(&["A"]), true, &every), before);
    }

    #[test]
    fn a_window_closed_during_the_day_is_forgotten() {
        let before = programs(&["A", "B", "C"]);
        let fresh = programs(&["A"]);
        assert_eq!(merge(Some(&before), fresh.clone(), false, &every), fresh);
    }

    #[test]
    fn a_program_never_seen_with_windows_is_left_out() {
        let fresh = Programs::from([("notepad.exe".to_string(), ProgramState::default())]);
        assert!(merge(None, fresh, false, &every).is_empty());
    }
}
