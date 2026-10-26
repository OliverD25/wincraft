use crate::core::ui_bridge::{CommandId, HostCommand, PaletteEntry};
use crate::search::{
    Action, Choice, Completion, Context, Glyph, IconRef, Query, ResultItem, SearchProvider,
};
use crate::ui::fuzzy;

/// WinCraft's own commands: host items, then each plugin's hotkey actions,
/// tray actions and palette commands, as the host lists them.
pub struct Commands;

impl SearchProvider for Commands {
    fn id(&self) -> &'static str {
        "commands"
    }

    fn name(&self) -> &'static str {
        "Commands"
    }

    fn description(&self) -> &'static str {
        "WinCraft and plugin commands"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some("!")
    }

    fn blended(&self) -> bool {
        true
    }

    fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem> {
        context
            .commands
            .iter()
            .filter_map(|entry| {
                if entry.quiet && query.text.is_empty() && query.prefix.is_none() {
                    return None;
                }
                let score = if query.text.is_empty() {
                    0
                } else {
                    fuzzy::score_command(&query.text, &entry.group, &entry.label)?
                };
                let action = match (&entry.plugin, entry.disabled) {
                    (Some(plugin), true) => Action::OpenPlugin(plugin.clone()),
                    _ => Action::Command(entry.id),
                };
                Some(ResultItem {
                    group: entry.group.clone(),
                    title: entry.label.clone(),
                    subtitle: entry.subtitle.clone().unwrap_or_default(),
                    icon: IconRef::Glyph(Glyph::Command),
                    hint: entry.hint.clone(),
                    score,
                    disabled: entry.disabled,
                    enter: Some(Choice {
                        label: "Run".to_string(),
                        action,
                    }),
                    tab: Some(Completion::quiet(&entry.label)),
                    usage_key: Some(command_key(entry)),
                    badge: entry.badge.clone(),
                    origin: entry.plugin.clone(),
                    keys: entry.shortcut.clone(),
                    shortcut: (!entry.shortcut_id.is_empty()).then(|| Choice {
                        label: "Change shortcut\u{2026}".to_string(),
                        action: Action::ChangeShortcut {
                            id: entry.shortcut_id.clone(),
                            label: entry.label.clone(),
                        },
                    }),
                    ..Default::default()
                })
            })
            .collect()
    }
}

/// The same for a command across restarts and whatever order the plugins
/// load in: the plugin's id and the action, never the plugin's place in the
/// list that `CommandId` carries.
pub fn command_key(entry: &PaletteEntry) -> String {
    let plugin = entry.plugin.as_deref().unwrap_or("host");
    match entry.id {
        CommandId::Host(HostCommand::TogglePlugin(_)) => format!("toggle/{plugin}"),
        CommandId::Host(command) => format!("host/{command:?}").to_lowercase(),
        CommandId::Plugin { kind, action, .. } => {
            format!("{plugin}/{kind:?}/{action}").to_lowercase()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::ActionKind;

    #[test]
    fn a_command_key_names_the_plugin_and_action_not_its_place() {
        let mut toggle = entry("ScreenDimmer", "Toggle monitor 1", false);
        toggle.id = CommandId::Plugin {
            index: 3,
            kind: ActionKind::Hotkey,
            action: 1,
        };
        assert_eq!(command_key(&toggle), "screen_dimmer/hotkey/1");
        toggle.id = CommandId::Plugin {
            index: 0,
            kind: ActionKind::Hotkey,
            action: 1,
        };
        assert_eq!(command_key(&toggle), "screen_dimmer/hotkey/1");

        let mut switch = entry("ScreenDimmer", "Turn off ScreenDimmer", false);
        switch.id = CommandId::Host(HostCommand::TogglePlugin(7));
        assert_eq!(command_key(&switch), "toggle/screen_dimmer");

        let mut settings = entry("WinCraft", "Open settings", false);
        settings.plugin = None;
        assert_eq!(command_key(&settings), "host/opensettings");
    }

    fn entry(group: &str, label: &str, disabled: bool) -> PaletteEntry {
        PaletteEntry {
            id: CommandId::Host(HostCommand::OpenSettings),
            group: group.to_string(),
            label: label.to_string(),
            hint: String::new(),
            disabled,
            plugin: Some("screen_dimmer".to_string()),
            subtitle: None,
            shortcut_id: String::new(),
            shortcut: String::new(),
            quiet: false,
            badge: None,
        }
    }

    #[test]
    fn a_badge_on_the_entry_reaches_the_row() {
        let mut update = host_entry(HostCommand::UpdateWinCraft, "WinCraft", "Update WinCraft");
        update.badge = Some("0.20.20 available".to_string());
        let commands = [update];
        let context = Context {
            commands: &commands,
            ..crate::search::test_context()
        };
        let rows = Commands.query(&Query::default(), &context);
        assert_eq!(rows[0].badge.as_deref(), Some("0.20.20 available"));
    }

    #[test]
    fn settings_the_store_and_the_update_are_found_by_typing_only() {
        let mut update = host_entry(HostCommand::UpdateWinCraft, "WinCraft", "Update WinCraft");
        update.quiet = true;
        let commands = [
            update,
            host_entry(HostCommand::OpenAbout, "WinCraft", "About WinCraft"),
        ];
        let context = Context {
            commands: &commands,
            ..crate::search::test_context()
        };
        let mut router = router();
        let titles = |router: &mut crate::search::Router, prefix, text| -> Vec<String> {
            router
                .search(prefix, text, &context)
                .items
                .into_iter()
                .map(|item| item.title)
                .collect()
        };
        assert_eq!(titles(&mut router, None, ""), ["About WinCraft"]);
        assert_eq!(titles(&mut router, None, "update"), ["Update WinCraft"]);
        assert_eq!(
            titles(&mut router, Some("!"), ""),
            ["Update WinCraft", "About WinCraft"],
            "the bang still lists every command"
        );
    }

    #[test]
    fn a_command_whose_plugin_is_off_opens_the_plugin_page() {
        let commands = [
            entry("ScreenDimmer", "Toggle monitor 1", true),
            entry("ScreenDimmer", "Wake all monitors", false),
        ];
        let context = Context {
            commands: &commands,
            ..crate::search::test_context()
        };
        let query = Query {
            text: "monitor".to_string(),
            prefix: None,
            limit: 8,
        };
        let found = Commands.query(&query, &context);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0].enter.as_ref().map(|choice| &choice.action),
            Some(&Action::OpenPlugin("screen_dimmer".to_string()))
        );
        assert_eq!(
            found[1].enter.as_ref().map(|choice| &choice.action),
            Some(&Action::Command(CommandId::Host(HostCommand::OpenSettings)))
        );
    }

    fn router() -> crate::search::Router {
        crate::search::Router::new(
            vec![
                (None, Box::new(Commands) as Box<dyn SearchProvider>),
                (None, Box::new(crate::search::providers::calc::Calculator)),
            ],
            &std::collections::BTreeMap::new(),
        )
    }

    fn host_entry(command: HostCommand, group: &str, label: &str) -> PaletteEntry {
        PaletteEntry {
            id: CommandId::Host(command),
            plugin: None,
            ..entry(group, label, false)
        }
    }

    #[test]
    fn the_bang_prefix_lists_every_command_and_only_commands() {
        let commands = [
            host_entry(HostCommand::OpenSettings, "WinCraft", "Open settings"),
            host_entry(HostCommand::UpdateWinCraft, "WinCraft", "Update WinCraft"),
            entry("ScreenDimmer", "Wake all monitors", true),
        ];
        let context = Context {
            commands: &commands,
            ..crate::search::test_context()
        };
        let mut router = router();
        assert_eq!(router.split("!upd"), Some(("!".to_string(), "upd")));

        let all = router.search(Some("!"), "", &context);
        let titles: Vec<&str> = all.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Open settings", "Update WinCraft", "Wake all monitors"],
            "host commands first, in the host's order, then each plugin"
        );

        let found = router.search(Some("!"), "2+2", &context);
        assert!(found.items.is_empty(), "no calculator answer after a bang");
    }

    #[test]
    fn typing_upd_finds_the_one_update_command() {
        let commands = [
            host_entry(HostCommand::OpenSettings, "WinCraft", "Open settings"),
            host_entry(HostCommand::UpdateWinCraft, "WinCraft", "Update WinCraft"),
        ];
        let context = Context {
            commands: &commands,
            ..crate::search::test_context()
        };
        let mut router = router();
        let has = |router: &mut crate::search::Router, prefix, text, title: &str| {
            let found = router.search(prefix, text, &context);
            found.items.iter().any(|item| item.title == title)
        };
        assert!(has(&mut router, None, "upd", "Update WinCraft"));
        assert!(has(&mut router, None, "update", "Update WinCraft"));
        let found = router.search(Some("!"), "upd", &context);
        let titles: Vec<&str> = found.items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Update WinCraft"]);
        assert!(!titles.contains(&"Open settings"));
    }

    #[test]
    fn the_prefix_list_offers_the_bang() {
        let mut router = router();
        let help = router.search(Some("?"), "", &crate::search::test_context());
        let row = help
            .items
            .iter()
            .find(|item| item.hint == "!")
            .expect("a row for !");
        assert_eq!(row.title, "Commands");
        assert_eq!(row.subtitle, "WinCraft and plugin commands");
    }
}
