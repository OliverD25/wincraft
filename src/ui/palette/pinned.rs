//! The "Update ready" row (design 1b, "Pinned at the top"): with nothing
//! typed and a newer release known, one framed row sits above Recent. It is
//! not selected when the palette opens, so Enter still does what it did;
//! ↑ reaches it. The palette keeps what it saw when it opened, so the row
//! never comes or goes while the user looks (Principle 1). No drawing here.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::clock;
use crate::core::ui_bridge::{CommandId, HostCommand, UiSnapshot};
use crate::search::{Action, Choice, IconRef, ResultItem};

pub const UPDATE_GROUP: &str = "Update ready";

static SCENE_SELECT: AtomicBool = AtomicBool::new(false);

/// `--select-update`, a test scene: the palette opens with the pinned row
/// selected, for a screenshot of its inspector. Nothing runs.
pub fn set_scene_select_update(on: bool) {
    SCENE_SELECT.store(on, Ordering::Relaxed);
}

pub fn scene_select_update() -> bool {
    SCENE_SELECT.load(Ordering::Relaxed)
}

/// Everything the row, its inspector and its keys need, taken from the
/// snapshot when the palette opened.
#[derive(Clone, Debug, PartialEq)]
pub struct PinnedUpdate {
    pub row: ResultItem,
    /// The release tag, such as "v0.20.60"; Ctrl+Del hides this one.
    pub tag: String,
    /// "0.20.60".
    pub version: String,
    /// The running version, "0.20.59".
    pub current: String,
    /// Markdown.
    pub notes: String,
    pub page: String,
    /// Downloaded and checked ahead already.
    pub downloaded: bool,
    /// When the check that found it answered, unix seconds; 0 for unknown.
    pub found: u64,
    /// "Change hotkey…" of the Update WinCraft command, when it has one.
    pub shortcut: Option<Choice>,
}

/// The pinned row when it belongs on screen: a newer release is known,
/// this copy can update itself, and the user did not hide that release.
pub fn pinned_update(snapshot: &UiSnapshot) -> Option<PinnedUpdate> {
    let view = &snapshot.update;
    let release = view.available.as_ref()?;
    if view.blocked.is_some() || view.hidden {
        return None;
    }
    let version = release.tag.trim_start_matches(['v', 'V']).to_string();
    let current = snapshot.version.clone();
    let entry = snapshot
        .commands
        .iter()
        .find(|entry| entry.id == CommandId::Host(HostCommand::UpdateWinCraft));
    let shortcut = entry
        .filter(|entry| !entry.shortcut_id.is_empty())
        .map(|entry| Choice {
            label: "Change hotkey\u{2026}".to_string(),
            action: Action::ChangeShortcut {
                id: entry.shortcut_id.clone(),
                label: entry.label.clone(),
            },
        });
    let subtitle = first_line(&release.notes)
        .unwrap_or_else(|| format!("You have {current} \u{b7} WinCraft restarts after it"));
    let row = ResultItem {
        group: UPDATE_GROUP.to_string(),
        title: format!("Update WinCraft to {version}"),
        subtitle,
        icon: IconRef::None,
        enter: Some(Choice {
            label: "Update now".to_string(),
            action: Action::Command(CommandId::Host(HostCommand::UpdateWinCraft)),
        }),
        badge: Some("New".to_string()),
        keys: entry
            .map(|entry| entry.shortcut.clone())
            .unwrap_or_default(),
        provider: "update".to_string(),
        ..Default::default()
    };
    Some(PinnedUpdate {
        row,
        tag: release.tag.clone(),
        version,
        current,
        notes: release.notes.clone(),
        page: release.page.clone(),
        downloaded: view.downloaded,
        found: view.last_check,
        shortcut,
    })
}

impl PinnedUpdate {
    /// What Update does, in the inspector under the versions.
    pub fn description(&self) -> String {
        let current = &self.current;
        if self.downloaded {
            format!(
                "Already downloaded and checked. Restarts WinCraft. \
                 If it does not start properly, {current} comes back."
            )
        } else {
            format!(
                "Downloads {}, checks it and restarts WinCraft. \
                 If it does not start properly, {current} comes back.",
                self.version
            )
        }
    }

    /// The inspector's last line: "Found by the update check · today, 09:14".
    pub fn found_line(&self, now: u64) -> String {
        const FOUND: &str = "Found by the update check";
        if self.found == 0 {
            return FOUND.to_string();
        }
        let when = clock::when(self.found as i64, now as i64);
        let when = match when.split_once(", ") {
            Some((day @ ("Today" | "Yesterday"), time)) => {
                format!("{}, {time}", day.to_lowercase())
            }
            _ => when,
        };
        format!("{FOUND} \u{b7} {when}")
    }

    /// The lines of "What's new", markdown taken out, at most `most`.
    pub fn news(&self, most: usize) -> Vec<String> {
        plain_lines(&self.notes).take(most).collect()
    }
}

/// The first line of release notes as plain text: headings and rules are
/// skipped, list marks and inline markdown are taken out. None when the
/// notes have nothing else, so the row says the versions instead.
pub fn first_line(notes: &str) -> Option<String> {
    plain_lines(notes).next()
}

fn plain_lines(notes: &str) -> impl Iterator<Item = String> + '_ {
    notes
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !is_rule(line))
        .map(plain)
        .filter(|line| !line.is_empty())
}

fn is_rule(line: &str) -> bool {
    line.len() >= 3 && line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '))
}

/// One line of markdown as the words a reader sees.
fn plain(line: &str) -> String {
    let mut line = line.trim_start_matches('>').trim_start();
    for mark in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(mark) {
            line = rest;
            break;
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            line = rest;
        }
    }
    let mut out = String::new();
    let mut rest = line;
    // [text](address) becomes text.
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match after.find("](").and_then(|close| {
            after[close + 2..]
                .find(')')
                .map(|end| (close, close + 2 + end))
        }) {
            Some((close, end)) => {
                out.push_str(&rest[..open]);
                out.push_str(&after[..close]);
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::{NewRelease, PaletteEntry, UpdateView};

    fn snapshot(notes: &str) -> UiSnapshot {
        UiSnapshot {
            version: "0.20.59".to_string(),
            update: UpdateView {
                available: Some(NewRelease {
                    tag: "v0.20.60".to_string(),
                    notes: notes.to_string(),
                    page: "https://github.com/OliverD25/wincraft/releases/tag/v0.20.60".to_string(),
                }),
                last_check: 0,
                ..UpdateView::default()
            },
            ..UiSnapshot::default()
        }
    }

    #[test]
    fn a_newer_release_pins_a_row_unless_blocked_or_hidden() {
        let pinned = pinned_update(&snapshot("Toggle rows show their state.")).unwrap();
        assert_eq!(pinned.row.group, UPDATE_GROUP);
        assert_eq!(pinned.row.title, "Update WinCraft to 0.20.60");
        assert_eq!(pinned.row.subtitle, "Toggle rows show their state.");
        assert_eq!(pinned.row.badge.as_deref(), Some("New"));
        assert_eq!(
            pinned.row.enter.as_ref().map(|choice| &choice.action),
            Some(&Action::Command(CommandId::Host(
                HostCommand::UpdateWinCraft
            )))
        );
        assert_eq!(
            (pinned.tag.as_str(), pinned.current.as_str()),
            ("v0.20.60", "0.20.59")
        );

        assert_eq!(pinned_update(&UiSnapshot::default()), None, "nothing newer");
        let mut blocked = snapshot("");
        blocked.update.blocked = Some("a development build".to_string());
        assert_eq!(pinned_update(&blocked), None);
        let mut hidden = snapshot("");
        hidden.update.hidden = true;
        assert_eq!(pinned_update(&hidden), None);
    }

    #[test]
    fn without_notes_the_row_names_the_running_version() {
        let pinned = pinned_update(&snapshot("## What's new\n\n---\n")).unwrap();
        assert_eq!(
            pinned.row.subtitle,
            "You have 0.20.59 \u{b7} WinCraft restarts after it"
        );
    }

    #[test]
    fn the_first_line_of_the_notes_is_plain_text() {
        assert_eq!(first_line(""), None);
        assert_eq!(
            first_line("## Changes\n\n- **Toggle** rows show their `state`.\n- Second"),
            Some("Toggle rows show their state.".to_string())
        );
        assert_eq!(
            first_line("1. See [the docs](https://example.com) now"),
            Some("See the docs now".to_string())
        );
        assert_eq!(
            first_line("* a [bracket without a link"),
            Some("a [bracket without a link".to_string())
        );
        assert_eq!(
            first_line("> quoted __words__"),
            Some("quoted words".to_string())
        );
    }

    #[test]
    fn the_inspector_says_what_update_does_and_when_it_was_found() {
        let mut pinned = pinned_update(&snapshot("- One\n- Two\n- Three")).unwrap();
        assert_eq!(
            pinned.description(),
            "Downloads 0.20.60, checks it and restarts WinCraft. \
             If it does not start properly, 0.20.59 comes back."
        );
        pinned.downloaded = true;
        assert!(pinned
            .description()
            .starts_with("Already downloaded and checked."));
        assert_eq!(
            pinned.found_line(1_800_000_000),
            "Found by the update check"
        );
        pinned.found = 1_800_000_000;
        let line = pinned.found_line(1_800_000_000);
        assert!(
            line.starts_with("Found by the update check \u{b7} today, "),
            "{line}"
        );
        assert_eq!(pinned.news(2), ["One", "Two"]);
    }

    #[test]
    fn the_row_offers_ctrl_k_only_when_the_command_has_a_hotkey_id() {
        let mut with = snapshot("");
        with.commands = vec![PaletteEntry {
            id: CommandId::Host(HostCommand::UpdateWinCraft),
            group: "WinCraft".to_string(),
            label: "Update WinCraft".to_string(),
            hint: String::new(),
            words: String::new(),
            confirm: String::new(),
            list: false,
            disabled: false,
            plugin: None,
            subtitle: None,
            shortcut_id: "wincraft.update".to_string(),
            shortcut: "Win+Alt+U".to_string(),
            quiet: true,
            badge: None,
            toggle: None,
        }];
        let pinned = pinned_update(&with).unwrap();
        assert!(matches!(
            pinned.shortcut.as_ref().map(|choice| &choice.action),
            Some(Action::ChangeShortcut { id, .. }) if id == "wincraft.update"
        ));
        assert_eq!(pinned.row.keys, "Win+Alt+U");
        assert_eq!(pinned_update(&snapshot("")).unwrap().shortcut, None);
    }
}
