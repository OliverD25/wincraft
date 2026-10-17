//! The release notes of the installed version, kept in `release_notes.json`
//! so About can show "what is new" after an update without asking GitHub
//! again. Whoever fetches a release that turns out to be the installed one
//! (the update that installed it, the daily check, or the new version
//! itself right after an update) writes it here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Cached {
    /// A version with or without the leading `v`.
    version: String,
    /// Markdown.
    notes: String,
}

pub fn path() -> PathBuf {
    config::data_dir().join("release_notes.json")
}

fn bare(version: &str) -> &str {
    version.trim().trim_start_matches(['v', 'V'])
}

/// Keeps the notes of `version`, in place of the notes of any other.
pub fn store(version: &str, notes: &str) {
    store_at(&path(), version, notes);
}

/// The notes kept for exactly this version, if there are any. Notes of
/// another version are not shown as these.
pub fn load(version: &str) -> Option<String> {
    load_at(&path(), version)
}

fn store_at(path: &Path, version: &str, notes: &str) {
    if notes.trim().is_empty() {
        return;
    }
    let cached = Cached {
        version: bare(version).to_string(),
        notes: notes.to_string(),
    };
    let result = serde_json::to_string_pretty(&cached)
        .map_err(|err| err.to_string())
        .and_then(|text| config::write_atomic(path, &text));
    if let Err(err) = result {
        log::warn!("the release notes of {version} could not be kept: {err}");
    }
}

fn load_at(path: &Path, version: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let cached: Cached = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    (bare(&cached.version) == bare(version) && !cached.notes.trim().is_empty())
        .then_some(cached.notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-notes-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("release_notes.json")
    }

    #[test]
    fn notes_come_back_for_the_version_they_were_kept_for() {
        let path = file("round");
        store_at(&path, "v0.16.2", "## What is new\n- a thing");
        assert_eq!(
            load_at(&path, "0.16.2").as_deref(),
            Some("## What is new\n- a thing")
        );
        assert_eq!(
            load_at(&path, "v0.16.2").as_deref(),
            Some("## What is new\n- a thing")
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn notes_of_another_version_are_not_shown() {
        let path = file("other");
        store_at(&path, "0.16.2", "notes");
        assert_eq!(load_at(&path, "0.16.1"), None);
        // The newer notes replace the older ones.
        store_at(&path, "0.16.3", "newer");
        assert_eq!(load_at(&path, "0.16.2"), None);
        assert_eq!(load_at(&path, "0.16.3").as_deref(), Some("newer"));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn empty_notes_and_unreadable_files_give_nothing() {
        let path = file("empty");
        assert_eq!(load_at(&path, "0.16.2"), None, "no file");
        store_at(&path, "0.16.2", "   ");
        assert_eq!(load_at(&path, "0.16.2"), None, "nothing was kept");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_at(&path, "0.16.2"), None);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
