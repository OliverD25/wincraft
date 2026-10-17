//! The user's bookmarks: a short phrase that opens a folder, a file or a web
//! address. They live in `bookmarks.json` next to the other WinCraft files,
//! as a list of `{ phrase, target, created }`.
//!
//! The palette's provider and the Bookmarks settings page both read and
//! write the same store, so it is one shared object.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::core::{config, expand_env};
use crate::search::usage;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub phrase: String,
    /// A folder or file path, or an http(s) address.
    pub target: String,
    /// Unix seconds.
    #[serde(default)]
    pub created: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Url,
    Path,
}

pub fn kind(target: &str) -> Kind {
    let lower = target.trim_start().to_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        Kind::Url
    } else {
        Kind::Path
    }
}

/// A target as it is kept: no quotes around it, `%VARS%` filled in and
/// backslashes for a path, no closing backslash except on a drive root.
pub fn clean_target(text: &str) -> String {
    let text = text.trim().trim_matches('"').trim();
    if kind(text) == Kind::Url {
        return text.to_string();
    }
    let path = expand_env(text).replace('/', "\\");
    let trimmed = path.trim_end_matches('\\');
    if trimmed.ends_with(':') || (trimmed.is_empty() && !path.is_empty()) {
        // "C:" alone means the current folder of that drive; "C:\" the root.
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    }
}

/// A phrase as it is kept: the words with single spaces between them.
pub fn clean_phrase(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The same place spelled in different case or with a closing backslash.
pub fn same_target(a: &str, b: &str) -> bool {
    let key = |text: &str| text.trim().trim_end_matches('\\').to_lowercase();
    key(a) == key(b)
}

/// What to offer as the phrase for a folder, file or site: its name in lower
/// case, or the host of an address without `www.`.
pub fn suggest_phrase(name: &str) -> String {
    let name = name.trim();
    if kind(name) == Kind::Url {
        let rest = name.split_once("://").map_or(name, |(_, rest)| rest);
        let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        return host.strip_prefix("www.").unwrap_or(host).to_lowercase();
    }
    name.trim_end_matches('\\').to_lowercase()
}

/// What `Store::set` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    Added,
    /// The phrase pointed somewhere else before.
    Replaced {
        old_target: String,
    },
    /// The phrase already pointed there.
    Unchanged,
}

#[derive(Default)]
pub struct Store {
    /// None for a store that is never saved, as in tests.
    path: Option<PathBuf>,
    items: Vec<Bookmark>,
}

impl Store {
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Reads the file, or starts empty. A file that cannot be read is kept as
    /// `bookmarks.json.bad` for a look later, and a new one starts.
    pub fn load(path: PathBuf) -> Self {
        let items = match std::fs::read_to_string(&path) {
            Err(_) => Vec::new(),
            Ok(text) => {
                match serde_json::from_str::<Vec<Bookmark>>(text.trim_start_matches('\u{feff}')) {
                    Ok(items) => items,
                    Err(err) => {
                        let bad = path.with_extension("json.bad");
                        log::warn!(
                            "{} cannot be read ({err}); kept as {} and starting fresh",
                            path.display(),
                            bad.display()
                        );
                        if let Err(err) = std::fs::rename(&path, &bad) {
                            log::warn!("could not rename it: {err}");
                        }
                        Vec::new()
                    }
                }
            }
        };
        log::info!("bookmarks: {} saved", items.len());
        Self {
            path: Some(path),
            items,
        }
    }

    #[cfg(test)]
    pub fn items(&self) -> &[Bookmark] {
        &self.items
    }

    /// By phrase, in the order the phrases sort in.
    pub fn sorted(&self) -> Vec<Bookmark> {
        let mut items = self.items.clone();
        items.sort_by_key(|item| item.phrase.to_lowercase());
        items
    }

    pub fn find(&self, phrase: &str) -> Option<&Bookmark> {
        let wanted = clean_phrase(phrase).to_lowercase();
        self.items
            .iter()
            .find(|item| item.phrase.to_lowercase() == wanted)
    }

    /// The phrase that already opens this place, if one does.
    pub fn phrase_for(&self, target: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|item| same_target(&item.target, target))
            .map(|item| item.phrase.as_str())
    }

    /// Adds a bookmark, or points an existing phrase somewhere else.
    pub fn set(&mut self, phrase: &str, target: &str, now: i64) -> Saved {
        let phrase = clean_phrase(phrase);
        let target = clean_target(target);
        let wanted = phrase.to_lowercase();
        let saved = match self
            .items
            .iter_mut()
            .find(|item| item.phrase.to_lowercase() == wanted)
        {
            Some(item) if same_target(&item.target, &target) => return Saved::Unchanged,
            Some(item) => {
                let old_target = std::mem::replace(&mut item.target, target);
                item.phrase = phrase;
                item.created = now;
                Saved::Replaced { old_target }
            }
            None => {
                self.items.push(Bookmark {
                    phrase,
                    target,
                    created: now,
                });
                Saved::Added
            }
        };
        self.save();
        saved
    }

    pub fn remove(&mut self, phrase: &str) -> bool {
        let wanted = clean_phrase(phrase).to_lowercase();
        let before = self.items.len();
        self.items
            .retain(|item| item.phrase.to_lowercase() != wanted);
        let removed = self.items.len() != before;
        if removed {
            self.save();
        }
        removed
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let result = serde_json::to_string_pretty(&self.items)
            .map_err(|err| err.to_string())
            .and_then(|text| config::write_atomic(path, &text));
        if let Err(err) = result {
            log::warn!("bookmarks could not be saved: {err}");
        }
    }
}

pub type Shared = Arc<Mutex<Store>>;

/// The store the palette and the settings window share, read from
/// `bookmarks.json` the first time it is asked for.
pub fn shared() -> Shared {
    static STORE: OnceLock<Shared> = OnceLock::new();
    STORE
        .get_or_init(|| {
            Arc::new(Mutex::new(Store::load(
                config::data_dir().join("bookmarks.json"),
            )))
        })
        .clone()
}

pub fn now() -> i64 {
    usage::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-bookmarks-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("bookmarks.json")
    }

    #[test]
    fn a_bookmark_survives_a_trip_through_the_file() {
        let path = temp_file("round");
        let mut store = Store::load(path.clone());
        assert_eq!(
            store.set("pswds", r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive", 100),
            Saved::Added
        );
        assert_eq!(
            store.set("Docs", "https://example.com/docs", 200),
            Saved::Added
        );
        let again = Store::load(path.clone());
        assert_eq!(again.items(), store.items());
        assert_eq!(again.items()[0].phrase, "pswds");
        assert_eq!(again.items()[0].created, 100);
        // The file is a plain list, as written in the plan.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.trim_start().starts_with('['));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_file_that_cannot_be_read_is_kept_aside_and_a_new_one_starts() {
        let path = temp_file("corrupt");
        std::fs::write(&path, "{ not a list").unwrap();
        let mut store = Store::load(path.clone());
        assert!(store.items().is_empty());
        let bad = path.with_extension("json.bad");
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "{ not a list");
        store.set("a", r"C:\a", 1);
        assert_eq!(Store::load(path.clone()).items().len(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_missing_file_and_a_byte_order_mark_are_both_fine() {
        let path = temp_file("bom");
        assert!(Store::load(path.clone()).items().is_empty());
        std::fs::write(
            &path,
            "\u{feff}[{\"phrase\":\"x\",\"target\":\"C:\\\\x\",\"created\":5}]",
        )
        .unwrap();
        assert_eq!(Store::load(path.clone()).items()[0].phrase, "x");
        // `created` may be missing from a hand-written file.
        std::fs::write(&path, "[{\"phrase\":\"y\",\"target\":\"C:\\\\y\"}]").unwrap();
        assert_eq!(Store::load(path.clone()).items()[0].created, 0);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_phrase_is_found_whatever_its_case_and_spacing() {
        let mut store = Store::in_memory();
        store.set("  My   Docs ", r"C:\docs", 1);
        assert_eq!(store.items()[0].phrase, "My Docs");
        assert_eq!(store.find("my docs").unwrap().target, r"C:\docs");
        assert_eq!(store.find("MY  DOCS").unwrap().target, r"C:\docs");
        assert!(store.find("docs").is_none());
    }

    #[test]
    fn setting_a_phrase_again_replaces_what_it_pointed_to() {
        let mut store = Store::in_memory();
        assert_eq!(store.set("pswds", r"D:\old", 1), Saved::Added);
        assert_eq!(store.set("PSWDS", r"d:\old\", 2), Saved::Unchanged);
        assert_eq!(
            store.set("pswds", r"D:\new", 3),
            Saved::Replaced {
                old_target: r"D:\old".to_string()
            }
        );
        assert_eq!(store.items().len(), 1);
        assert_eq!(store.items()[0].target, r"D:\new");
        assert_eq!(store.items()[0].created, 3);
    }

    #[test]
    fn removing_forgets_the_bookmark_and_says_whether_there_was_one() {
        let mut store = Store::in_memory();
        store.set("a", r"C:\a", 1);
        store.set("b", r"C:\b", 1);
        assert!(store.remove("A"));
        assert!(!store.remove("a"));
        assert_eq!(store.items().len(), 1);
        assert_eq!(store.items()[0].phrase, "b");
    }

    #[test]
    fn sorted_lists_by_phrase_without_changing_the_saved_order() {
        let mut store = Store::in_memory();
        store.set("zeta", "C:\\z", 1);
        store.set("Alpha", "C:\\a", 1);
        let phrases: Vec<String> = store.sorted().into_iter().map(|item| item.phrase).collect();
        assert_eq!(phrases, ["Alpha", "zeta"]);
        assert_eq!(store.items()[0].phrase, "zeta");
    }

    #[test]
    fn targets_are_told_apart_and_tidied() {
        assert_eq!(kind("https://example.com"), Kind::Url);
        assert_eq!(kind("HTTP://example.com"), Kind::Url);
        assert_eq!(kind(r"D:\x"), Kind::Path);
        assert_eq!(kind("www.example.com"), Kind::Path);
        assert_eq!(clean_target(r#"  "D:/Docs/Work/"  "#), r"D:\Docs\Work");
        assert_eq!(clean_target("D:"), r"D:\");
        assert_eq!(clean_target(r"D:\"), r"D:\");
        assert_eq!(
            clean_target(" https://example.com/a b "),
            "https://example.com/a b"
        );
        assert!(same_target(r"D:\Docs\", r"d:\docs"));
        assert!(!same_target(r"D:\Docs", r"D:\Docs2"));
    }

    #[test]
    fn a_suggested_phrase_is_the_name_in_lower_case() {
        assert_eq!(suggest_phrase("pswds_OneDrive"), "pswds_onedrive");
        assert_eq!(suggest_phrase("Project Files"), "project files");
        assert_eq!(
            suggest_phrase("https://www.Example.com/a/b?x=1"),
            "example.com"
        );
        assert_eq!(suggest_phrase("D:\\"), "d:");
    }

    #[test]
    fn the_target_of_a_phrase_is_found_by_place() {
        let mut store = Store::in_memory();
        store.set("pswds", r"D:\Docs\pswds", 1);
        assert_eq!(store.phrase_for(r"d:\docs\PSWDS\"), Some("pswds"));
        assert_eq!(store.phrase_for(r"D:\Docs"), None);
    }
}
