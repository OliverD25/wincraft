//! `plugins\screen_dimmer.state.json`: what each monitor showed when it was
//! tested, and whether WinCraft left it in standby or dimmed. The host owns
//! `screen_dimmer.json` and writes it from memory, so the plugin keeps its
//! own file beside it, as LayoutKeeper does.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config;

/// What asking the monitor for standby showed the last time it was tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standby {
    Works,
    Ignored,
    NoDdc,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standby: Option<Standby>,
    /// Whether the monitor answered a brightness read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<bool>,
    /// Written before standby is asked for and cleared once the monitor
    /// reports on, so a start after a crash knows to wake it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub left_in_standby: bool,
    /// The brightness before WinCraft set it to 0, written before it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimmed_from: Option<u32>,
}

impl Saved {
    pub fn is_empty(&self) -> bool {
        *self == Saved::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateFile {
    #[serde(default)]
    pub monitors: BTreeMap<String, Saved>,
}

pub fn default_path() -> PathBuf {
    config::plugins_dir().join("screen_dimmer.state.json")
}

pub fn load(path: &Path) -> StateFile {
    let Ok(text) = fs::read_to_string(path) else {
        return StateFile::default();
    };
    match serde_json::from_str(config::strip_bom(&text)) {
        Ok(file) => file,
        Err(err) => {
            log::warn!("{} cannot be read ({err}); starting afresh", path.display());
            StateFile::default()
        }
    }
}

pub fn save(path: &Path, file: &StateFile) -> Result<(), String> {
    let text = serde_json::to_string_pretty(file)
        .map_err(|err| format!("cannot serialise the dimmer state: {err}"))?;
    config::write_atomic(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_round_trips_and_leaves_out_what_is_not_known() {
        let dir = std::env::temp_dir().join(format!("wincraft-dimmer-{}", std::process::id()));
        let path = dir.join("screen_dimmer.state.json");
        let mut file = StateFile::default();
        file.monitors.insert(
            "PHL 323E7".to_string(),
            Saved {
                standby: Some(Standby::Works),
                left_in_standby: true,
                ..Saved::default()
            },
        );
        file.monitors.insert(
            "LG SDQHD".to_string(),
            Saved {
                standby: Some(Standby::Ignored),
                brightness: Some(true),
                dimmed_from: Some(100),
                ..Saved::default()
            },
        );
        save(&path, &file).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"standby\": \"works\""), "{text}");
        assert!(!text.contains("brightness\": null"), "{text}");
        assert_eq!(load(&path), file);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_broken_file_starts_afresh() {
        let dir = std::env::temp_dir().join(format!("wincraft-dimmer-bad-{}", std::process::id()));
        let path = dir.join("screen_dimmer.state.json");
        assert_eq!(load(&path), StateFile::default());
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), StateFile::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
