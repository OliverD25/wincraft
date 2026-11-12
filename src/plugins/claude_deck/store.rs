//! The user's own arrangement of ClaudeDeck's list, kept in one small file
//! next to the window's place: the groups (name, colour, fold, order), which
//! chat sits in which group and where, folded forks, hidden and pinned
//! chats, and whether the cold line is open. All of it is about ClaudeDeck's
//! list only; nothing here touches a real chat.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config;

/// The form of the file. A file from a newer WinCraft is read but never
/// written, so fields this copy does not know are not lost.
pub const VERSION: u32 = 1;

/// The colours a group can have: a small fixed set of theme colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Colour {
    Blue,
    Green,
    Yellow,
    Red,
    Grey,
}

impl Colour {
    pub const ALL: [Colour; 5] = [
        Colour::Blue,
        Colour::Green,
        Colour::Yellow,
        Colour::Red,
        Colour::Grey,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Colour::Blue => "Blue",
            Colour::Green => "Green",
            Colour::Yellow => "Yellow",
            Colour::Red => "Red",
            Colour::Grey => "Grey",
        }
    }
}

/// One group of the list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub colour: Option<Colour>,
    pub folded: bool,
    /// The project folder (lower case) the group was made for, if any.
    pub folder: Option<String>,
    /// The user made or changed it: it stays even with no chats in it.
    pub keep: bool,
    /// The chats at the top of their family, in the user's order. A fork
    /// is not here: it hangs under its chat.
    pub chats: Vec<String>,
}

/// Everything the file holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub version: u32,
    pub groups: Vec<Group>,
    /// Chats whose forks are folded.
    pub folded_chats: BTreeSet<String>,
    /// Chats left out of the list (with everything below them).
    pub hidden: BTreeSet<String>,
    /// Chats kept at the top of their group.
    pub pinned: BTreeSet<String>,
    pub cold_open: bool,
    /// Saved chats that are not listed now, with when they were first
    /// missed (Unix ms). Dropped after `KEEP_GONE_MS`.
    pub gone: BTreeMap<String, u64>,
}

/// How long a chat that is no longer listed keeps its place.
pub const KEEP_GONE_MS: u64 = 30 * super::model::DAY_MS;

pub fn path() -> PathBuf {
    config::plugins_dir().join("claude_deck.list.json")
}

/// What reading the file found.
#[derive(Debug, PartialEq, Eq)]
pub enum Loaded {
    /// No file yet: the first run.
    Missing,
    Read(Saved),
    /// A file from a newer WinCraft: used, but never written.
    Newer(Saved),
    /// A file that could not be read. It was moved aside to the path given
    /// (None when that failed, and then nothing may be written).
    Broken(Option<PathBuf>),
}

impl Loaded {
    /// The arrangement to start from, and whether it may be written.
    pub fn start(self) -> (Saved, bool) {
        match self {
            Loaded::Missing | Loaded::Broken(Some(_)) => (Saved::default(), true),
            Loaded::Read(saved) => (saved, true),
            Loaded::Newer(saved) => (saved, false),
            Loaded::Broken(None) => (Saved::default(), false),
        }
    }
}

pub fn load(path: &Path) -> Loaded {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(err) => {
            log::warn!("ClaudeDeck: could not read {}: {err}", path.display());
            return Loaded::Broken(None);
        }
    };
    match serde_json::from_str::<Saved>(config::strip_bom(&text)) {
        Ok(saved) if saved.version > VERSION => Loaded::Newer(saved),
        Ok(saved) => Loaded::Read(saved),
        Err(err) => {
            log::warn!("ClaudeDeck: {} is broken ({err})", path.display());
            Loaded::Broken(put_aside(path))
        }
    }
}

/// Moves a broken file to `<name>.bad` (or `.bad.1` to `.bad.9` when that
/// is taken), so it is never overwritten with an empty list.
fn put_aside(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy().to_string();
    let aside = (0..10)
        .map(|n| match n {
            0 => path.with_file_name(format!("{name}.bad")),
            n => path.with_file_name(format!("{name}.bad.{n}")),
        })
        .find(|aside| !aside.exists())?;
    match std::fs::rename(path, &aside) {
        Ok(()) => {
            log::warn!("ClaudeDeck: kept the broken file as {}", aside.display());
            Some(aside)
        }
        Err(err) => {
            log::warn!("ClaudeDeck: could not move the broken file aside: {err}");
            None
        }
    }
}

pub fn save(path: &Path, saved: &Saved) {
    let saved = Saved {
        version: VERSION,
        ..saved.clone()
    };
    let Ok(text) = serde_json::to_string_pretty(&saved) else {
        return;
    };
    if let Err(err) = config::write_atomic(path, &text) {
        log::warn!("ClaudeDeck: could not save the list's order: {err}");
    }
}
