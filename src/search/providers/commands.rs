use crate::search::{Action, Choice, Context, Query, ResultItem, SearchProvider};
use crate::ui::fuzzy;

/// WinCraft's own commands: host items, then each plugin's hotkey actions,
/// tray actions and palette commands, as the host lists them.
pub struct Commands;

impl SearchProvider for Commands {
    fn id(&self) -> &'static str {
        "commands"
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
                    hint: entry.hint.clone(),
                    score,
                    disabled: entry.disabled,
                    enter: Some(Choice {
                        label: "Run".to_string(),
                        action,
                    }),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::{CommandId, HostCommand, PaletteEntry};

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
            plugins: &[],
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
