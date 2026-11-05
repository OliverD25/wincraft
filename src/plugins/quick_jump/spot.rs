//! Where Quick access opens. In the middle of the screen by default; with
//! the setting off, where the user last left it, kept in its own small file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config;

/// The setting's key; on (true) when missing.
pub const CENTRE: &str = "centre_quick_access";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spot {
    pub x: i32,
    pub y: i32,
}

/// Where the window goes when it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Centre,
    At(i32, i32),
}

pub fn path() -> PathBuf {
    config::plugins_dir().join("quick_jump.spot.json")
}

/// Whether the settings ask for the middle of the screen.
pub fn centre_on(settings: &serde_json::Value) -> bool {
    settings
        .get(CENTRE)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

/// The middle when the setting is on, or when no spot was saved yet.
pub fn place(centre: bool, saved: Option<Spot>) -> Place {
    match saved {
        Some(spot) if !centre => Place::At(spot.x, spot.y),
        _ => Place::Centre,
    }
}

pub fn load(path: &Path) -> Option<Spot> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(path: &Path, spot: Spot) {
    let Ok(text) = serde_json::to_string(&spot) else {
        return;
    };
    if let Err(err) = config::write_atomic(path, &text) {
        log::warn!("QuickJump: could not save where Quick access was: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_middle_is_the_default() {
        assert!(centre_on(&json!({})));
        assert!(centre_on(&json!({ CENTRE: true })));
        assert!(!centre_on(&json!({ CENTRE: false })));
    }

    #[test]
    fn a_saved_spot_is_used_only_with_the_setting_off() {
        let spot = Some(Spot { x: 40, y: -300 });
        assert_eq!(place(true, spot), Place::Centre);
        assert_eq!(place(false, spot), Place::At(40, -300));
        assert_eq!(place(false, None), Place::Centre);
    }

    #[test]
    fn a_spot_survives_a_save_and_a_load() {
        let dir = std::env::temp_dir().join(format!("wincraft-qj-spot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("quick_jump.spot.json");
        assert_eq!(load(&file), None, "no file yet");
        save(&file, Spot { x: 2100, y: 80 });
        assert_eq!(load(&file), Some(Spot { x: 2100, y: 80 }));
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(load(&file), None, "a broken file means the middle");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
