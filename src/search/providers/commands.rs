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

    fn blended(&self) -> bool {
        true
    }

    fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem> {
        context
            .commands
            .iter()
            .filter_map(|entry| {
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
        }
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
}
