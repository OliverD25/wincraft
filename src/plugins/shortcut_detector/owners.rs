//! Who holds a taken combination. Windows has no call that names the owner
//! of a registered hotkey, so the owner comes from two places read at scan
//! time: the keys of shortcut files, which Explorer registers (certain), and
//! the default keys of known apps that are running (a likely guess).

use std::path::PathBuf;
use std::time::Instant;

use crate::core::traits::Hotkey;
use crate::core::windows_list;

use super::known;
use super::known_apps::{self, KnownApp};
use super::lnk::{self, LinkKey};
use super::probe::{Entry, Status};

pub const SOURCE_LINK: &str = "shortcut file";
pub const SOURCE_KNOWN: &str = "known apps";
pub const SOURCE_PRESS: &str = "press test";

#[derive(Clone, Default)]
pub struct Owners {
    pub links: Vec<LinkKey>,
    /// Lowercase program file names of the processes running at scan time.
    pub running: Vec<String>,
}

/// One answer to "who holds it".
#[derive(Clone, Copy, Debug)]
pub enum Found<'a> {
    Link(&'a LinkKey),
    Known {
        app: &'static KnownApp,
        meaning: &'static str,
    },
}

impl Found<'_> {
    pub fn holder(&self) -> String {
        match self {
            Found::Link(link) => format!("Explorer shortcut: {}", link.name),
            Found::Known { app, .. } => format!("{}, probably", app.name),
        }
    }

    pub fn meaning(&self) -> String {
        match self {
            Found::Link(link) => format!("Open {}", link.name),
            Found::Known { meaning, .. } => meaning.to_string(),
        }
    }

    pub fn source(&self) -> &'static str {
        match self {
            Found::Link(_) => SOURCE_LINK,
            Found::Known { .. } => SOURCE_KNOWN,
        }
    }
}

impl Owners {
    /// Reads the shortcut files and the running programs. Called only when
    /// the window scans, never in the background.
    pub fn collect() -> Owners {
        let start = Instant::now();
        let links = lnk::scan(&lnk::folders());
        let link_ms = start.elapsed().as_millis();
        let running = windows_list::process_names()
            .into_iter()
            .map(|name| name.to_lowercase())
            .collect::<Vec<_>>();
        log::info!(
            "shortcut owners: {} shortcut files with a key in {link_ms} ms, {} programs running, {} ms in all",
            links.len(),
            running.len(),
            start.elapsed().as_millis()
        );
        Owners { links, running }
    }

    /// A shortcut file's key wins over a known app's default, because
    /// Explorer really registered it.
    pub fn find(&self, hotkey: Hotkey) -> Option<Found<'_>> {
        if let Some(link) = self.links.iter().find(|link| link.hotkey == hotkey) {
            return Some(Found::Link(link));
        }
        known_apps::running_owner(hotkey, &self.running)
            .map(|(app, meaning)| Found::Known { app, meaning })
    }

    /// Names the holder of a taken combination when one is found. WinCraft's
    /// own keys and the ones Windows reserves stay as they are. A Windows
    /// list entry that an app holds becomes the app's: Alt+Space, say, is
    /// the window menu until a launcher registers it.
    pub fn apply(&self, entry: &mut Entry) {
        if !entry.taken || entry.status == Status::WinCraft || known::is_reserved(entry.hotkey) {
            return;
        }
        if let Some(found) = self.find(entry.hotkey) {
            entry.status = Status::TakenByApp;
            entry.holder = found.holder();
            entry.meaning = found.meaning();
            entry.source = found.source();
        }
    }

    /// The shortcut file that holds `hotkey`, for Change and Remove key.
    pub fn link_path(&self, hotkey: Hotkey) -> Option<PathBuf> {
        match self.find(hotkey)? {
            Found::Link(link) => Some(link.path.clone()),
            Found::Known { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::probe::{classify, verdict_text};
    use super::*;
    use crate::core::hotkeys;

    fn key(text: &str) -> Hotkey {
        hotkeys::parse(text).expect(text)
    }

    fn owners(links: &[(&str, &str)], running: &[&str]) -> Owners {
        Owners {
            links: links
                .iter()
                .map(|(keys, name)| LinkKey {
                    hotkey: key(keys),
                    name: name.to_string(),
                    path: PathBuf::from(format!(r"C:\fake\{name}.lnk")),
                })
                .collect(),
            running: running.iter().map(|name| name.to_lowercase()).collect(),
        }
    }

    fn applied(owners: &Owners, keys: &str, taken: bool) -> Entry {
        let mut entry = classify(key(keys), taken, None);
        owners.apply(&mut entry);
        entry
    }

    #[test]
    fn a_shortcut_file_names_its_owner_for_certain() {
        let found = owners(&[("Ctrl+Alt+N", "Notepad++")], &[]);
        let entry = applied(&found, "Ctrl+Alt+N", true);
        assert_eq!(entry.status, Status::TakenByApp);
        assert_eq!(entry.owner(), "Explorer shortcut: Notepad++");
        assert_eq!(entry.meaning, "Open Notepad++");
        assert_eq!(entry.source, SOURCE_LINK);
        assert_eq!(
            verdict_text(&entry),
            "App \u{b7} Explorer shortcut: Notepad++ \u{2014} Open Notepad++"
        );
        assert_eq!(
            found.link_path(key("Ctrl+Alt+N")),
            Some(PathBuf::from(r"C:\fake\Notepad++.lnk"))
        );
    }

    #[test]
    fn a_known_app_is_named_only_while_it_runs_and_only_as_a_guess() {
        let off = owners(&[], &["notepad.exe"]);
        let entry = applied(&off, "Win+`", true);
        assert_eq!(entry.owner(), "unknown app");
        assert_eq!(entry.source, "probe");
        assert_eq!(verdict_text(&entry), "App \u{b7} unknown app");

        let on = owners(&[], &["WindowsTerminal.exe"]);
        let entry = applied(&on, "Win+`", true);
        assert_eq!(entry.owner(), "Windows Terminal, probably");
        assert_eq!(entry.meaning, "Open the Quake mode window");
        assert_eq!(entry.source, SOURCE_KNOWN);
        assert!(matches!(on.find(key("Win+`")), Some(Found::Known { .. })));
        assert_eq!(on.link_path(key("Win+`")), None);
    }

    #[test]
    fn a_shortcut_file_wins_over_a_known_app() {
        let both = owners(&[("Ctrl+PrintScreen", "Snap")], &["ShareX.exe"]);
        let entry = applied(&both, "Ctrl+PrintScreen", true);
        assert_eq!(entry.owner(), "Explorer shortcut: Snap");
        assert!(matches!(
            both.find(key("Ctrl+PrintScreen")),
            Some(Found::Link(_))
        ));
    }

    #[test]
    fn a_free_combination_wincraft_and_reserved_keys_are_left_alone() {
        let all = owners(
            &[("Ctrl+Alt+N", "Notes"), ("Ctrl+Alt+Delete", "Odd")],
            &["WindowsTerminal.exe"],
        );
        let free = applied(&all, "Ctrl+Alt+N", false);
        assert_eq!(free.status, Status::Free);
        assert_eq!(free.owner(), "");

        let mut mine = classify(key("Ctrl+Alt+N"), true, Some(("QuickJump", "Open")));
        all.apply(&mut mine);
        assert_eq!(mine.status, Status::WinCraft);
        assert_eq!(mine.owner(), "WinCraft");

        let reserved = applied(&all, "Ctrl+Alt+Delete", true);
        assert_eq!(reserved.status, Status::Windows);
        assert_eq!(reserved.owner(), "Windows");
    }

    #[test]
    fn an_app_that_holds_a_windows_list_key_is_named() {
        let launcher = owners(&[], &["Flow.Launcher.exe"]);
        let entry = applied(&launcher, "Alt+Space", true);
        assert_eq!(entry.status, Status::TakenByApp);
        assert_eq!(entry.owner(), "Flow Launcher, probably");
        // Not running: the Windows meaning stays.
        let entry = applied(&Owners::default(), "Alt+Space", true);
        assert_eq!(entry.status, Status::Windows);
        assert_eq!(entry.owner(), "Windows");
    }
}
