//! Whether each hotkey works right now, for the Hotkeys page: one state
//! per row and the summary line above the list.

use std::collections::BTreeMap;

use crate::core::hotkeys;
use crate::core::shortcuts;
use crate::core::ui_bridge::{ShortcutInfo, UiSnapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyState {
    Registered,
    /// Another app holds the first key press (`None`), or another WinCraft
    /// command that registered it first (`Some(its name)`).
    Taken(Option<String>),
    /// A disabled plugin registers nothing, so its keys are neither live nor
    /// taken.
    PluginOff,
}

impl HotkeyState {
    pub fn tooltip(&self) -> String {
        match self {
            HotkeyState::Registered => "Working".to_string(),
            HotkeyState::Taken(Some(holder)) => format!("Taken by {holder}"),
            HotkeyState::Taken(None) => "Taken by another app".to_string(),
            HotkeyState::PluginOff => "Plugin is off".to_string(),
        }
    }
}

/// The state of every row that has a shortcut, by command id. For a chord
/// this is the state of its first press.
pub fn states(snapshot: &UiSnapshot) -> BTreeMap<String, HotkeyState> {
    let off = |info: &ShortcutInfo| {
        info.plugin.as_ref().is_some_and(|id| {
            snapshot
                .plugins
                .iter()
                .any(|plugin| &plugin.id == id && !plugin.enabled)
        })
    };
    let all = &snapshot.shortcuts;
    let mut found = BTreeMap::new();
    for info in all {
        let Some(keys) = shortcuts::parse(&info.binding) else {
            continue;
        };
        let state = if off(info) {
            HotkeyState::PluginOff
        } else if info.registered {
            HotkeyState::Registered
        } else {
            let holder = all.iter().find(|other| {
                other.id != info.id
                    && other.registered
                    && !off(other)
                    && shortcuts::parse(&other.binding)
                        .is_some_and(|theirs| hotkeys::same(keys.first, theirs.first))
            });
            HotkeyState::Taken(holder.map(|other| other.label.clone()))
        };
        found.insert(info.id.clone(), state);
    }
    found
}

/// "12 working · 1 taken by another app · 2 plugins off": zero parts are
/// left out.
pub fn summary(states: &BTreeMap<String, HotkeyState>) -> String {
    if states.is_empty() {
        return "No command has a hotkey yet.".to_string();
    }
    let count = |keep: fn(&HotkeyState) -> bool| states.values().filter(|s| keep(s)).count();
    let working = count(|s| matches!(s, HotkeyState::Registered));
    let by_app = count(|s| matches!(s, HotkeyState::Taken(None)));
    let by_command = count(|s| matches!(s, HotkeyState::Taken(Some(_))));
    let off = count(|s| matches!(s, HotkeyState::PluginOff));
    if by_app + by_command + off == 0 {
        return "All hotkeys are working.".to_string();
    }
    let mut parts = Vec::new();
    if working > 0 {
        parts.push(format!("{working} working"));
    }
    if by_app > 0 {
        parts.push(format!("{by_app} taken by another app"));
    }
    if by_command > 0 {
        parts.push(format!("{by_command} taken by another command"));
    }
    if off > 0 {
        let noun = if off == 1 { "plugin" } else { "plugins" };
        parts.push(format!("{off} {noun} off"));
    }
    parts.join(" \u{00B7} ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::PluginInfo;

    fn info(id: &str, plugin: Option<&str>, binding: &str, registered: bool) -> ShortcutInfo {
        ShortcutInfo {
            id: id.to_string(),
            command: None,
            group: "G".to_string(),
            plugin: plugin.map(str::to_string),
            category: String::new(),
            icon: None,
            label: format!("Label {id}"),
            description: String::new(),
            binding: binding.to_string(),
            default_binding: String::new(),
            registered,
            one_press: false,
        }
    }

    fn plugin(id: &str, enabled: bool) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            hotkeys: Vec::new(),
            version: String::new(),
            author: String::new(),
            description: String::new(),
            readme: String::new(),
            category: String::new(),
            icon: None,
            config_path: Default::default(),
            fields: Vec::new(),
            status: None,
            page_action: None,
        }
    }

    fn snapshot(shortcuts: Vec<ShortcutInfo>) -> UiSnapshot {
        UiSnapshot {
            shortcuts,
            plugins: vec![plugin("dimmer", true), plugin("keeper", false)],
            ..UiSnapshot::default()
        }
    }

    #[test]
    fn each_row_gets_its_state_and_rows_without_keys_get_none() {
        let snap = snapshot(vec![
            info("palette", None, "Win+Alt+P", false),
            info("a", Some("dimmer"), "Win+Alt+F1", true),
            info("b", Some("dimmer"), "Win+Alt+F2, S", false),
            info("c", Some("keeper"), "Win+Alt+L", true),
            info("d", Some("dimmer"), "", true),
        ]);
        let found = states(&snap);
        assert_eq!(found["palette"], HotkeyState::Taken(None));
        assert_eq!(found["a"], HotkeyState::Registered);
        assert_eq!(found["b"], HotkeyState::Taken(None));
        assert_eq!(found["c"], HotkeyState::PluginOff);
        assert!(!found.contains_key("d"));
    }

    #[test]
    fn a_plugin_off_wins_over_a_failed_registration() {
        let snap = snapshot(vec![info("c", Some("keeper"), "Win+Alt+L", false)]);
        assert_eq!(states(&snap)["c"], HotkeyState::PluginOff);
    }

    #[test]
    fn a_key_held_by_another_command_names_it() {
        let snap = snapshot(vec![
            info("a", Some("dimmer"), "Win+Alt+F1", true),
            info("b", Some("dimmer"), "Win+Alt+F1, S", false),
        ]);
        let found = states(&snap);
        assert_eq!(found["b"], HotkeyState::Taken(Some("Label a".to_string())));
        assert_eq!(found["b"].tooltip(), "Taken by Label a");
    }

    #[test]
    fn tooltips_say_it_plainly() {
        assert_eq!(HotkeyState::Registered.tooltip(), "Working");
        assert_eq!(HotkeyState::Taken(None).tooltip(), "Taken by another app");
        assert_eq!(HotkeyState::PluginOff.tooltip(), "Plugin is off");
    }

    fn line(rows: Vec<ShortcutInfo>) -> String {
        summary(&states(&snapshot(rows)))
    }

    #[test]
    fn the_summary_leaves_out_zero_parts() {
        assert_eq!(line(Vec::new()), "No command has a hotkey yet.");
        assert_eq!(
            line(vec![info("a", Some("dimmer"), "Win+Alt+F1", true)]),
            "All hotkeys are working."
        );
        assert_eq!(
            line(vec![
                info("a", Some("dimmer"), "Win+Alt+F1", true),
                info("b", Some("dimmer"), "Win+Alt+F2", true),
                info("c", Some("dimmer"), "Win+Alt+F3", false),
            ]),
            "2 working \u{00B7} 1 taken by another app"
        );
        assert_eq!(
            line(vec![
                info("a", Some("dimmer"), "Win+Alt+F1", true),
                info("c", Some("keeper"), "Win+Alt+L", true),
                info("d", Some("keeper"), "Win+Alt+M", true),
            ]),
            "1 working \u{00B7} 2 plugins off"
        );
        assert_eq!(
            line(vec![
                info("c", Some("keeper"), "Win+Alt+L", true),
                info("p", None, "Win+Alt+P", false),
            ]),
            "1 taken by another app \u{00B7} 1 plugin off"
        );
    }
}
