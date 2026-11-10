//! "Group related windows": the groups WindowDeck keeps in
//! `window_deck.groups.json`, next to the plugin's settings. While the
//! setting is off nothing reads them, and the file stays for the day it is
//! switched on again (Principle 3).

use std::path::PathBuf;

use crate::core::config;
use crate::core::deck_groups::{self, DeckGroup};

pub const KEY: &str = "group_related_windows";

pub fn path() -> PathBuf {
    config::plugins_dir().join("window_deck.groups.json")
}

pub struct Groups {
    on: bool,
    list: Vec<DeckGroup>,
    /// Where they are saved; None keeps them in memory only (tests).
    file: Option<PathBuf>,
}

impl Groups {
    pub fn new(file: Option<PathBuf>) -> Self {
        Self {
            on: false,
            list: Vec::new(),
            file,
        }
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// The setting changed: the groups are read when it goes on and
    /// forgotten when it goes off.
    pub fn switch(&mut self, on: bool) {
        if on == self.on {
            return;
        }
        self.on = on;
        self.list = match (&self.file, on) {
            (Some(file), true) => deck_groups::load(file),
            _ => Vec::new(),
        };
    }

    /// What the strip gets: None while the setting is off.
    pub fn for_strip(&self) -> Option<Vec<DeckGroup>> {
        self.on.then(|| self.list.clone())
    }

    /// The strip changed a group: keep and save the new list. Dropped while
    /// the setting is off, so a late message cannot write the file.
    pub fn set(&mut self, list: &[DeckGroup]) {
        if !self.on {
            log::info!("WindowDeck: a change to the groups came while grouping is off; dropped");
            return;
        }
        self.list = list.to_vec();
        if let Some(file) = &self.file {
            match deck_groups::save(file, &self.list) {
                Ok(()) => log::info!("WindowDeck: {} window groups saved", self.list.len()),
                Err(err) => log::warn!("WindowDeck: the window groups were not saved: {err}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> DeckGroup {
        DeckGroup {
            app: "chrome".into(),
            name: "NewsHQ".into(),
            members: vec!["a".into(), "b".into()],
        }
    }

    #[test]
    fn groups_are_read_only_while_the_setting_is_on() {
        let dir = std::env::temp_dir().join(format!("wincraft-deck-file-{}", std::process::id()));
        let file = dir.join("window_deck.groups.json");
        deck_groups::save(&file, &[group()]).expect("saved");
        let mut groups = Groups::new(Some(file.clone()));
        assert_eq!(groups.for_strip(), None);
        groups.set(&[]);
        assert_eq!(deck_groups::load(&file), vec![group()], "off: not written");
        groups.switch(true);
        assert_eq!(groups.for_strip(), Some(vec![group()]));
        groups.set(&[]);
        assert!(deck_groups::load(&file).is_empty());
        groups.switch(false);
        assert_eq!(groups.for_strip(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
