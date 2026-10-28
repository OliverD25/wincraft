use super::*;

impl Host {
    /// Shows the balloons the plugin bookkeeping asked for. Clicking one
    /// opens ShortcutDetector.
    pub(super) fn show_warnings(&mut self) {
        for text in std::mem::take(&mut self.plugins.warnings) {
            self.balloon_click = BalloonClick::Detector;
            self.tray.balloon("WinCraft", &text);
        }
    }

    pub(super) fn enable_slot(&mut self, index: usize) {
        self.plugins.enable(index);
        self.show_warnings();
    }

    /// Open palette, Settings, then one primary action per enabled plugin,
    /// then Quit. Everything else lives in the settings window now.
    pub(super) fn menu_items(&self) -> Vec<MenuItem> {
        let entry = |id: u32, label: &str| MenuItem::Entry {
            id,
            label: label.to_string(),
            checked: false,
        };
        let mut items = vec![
            entry(MENU_PALETTE, "Open palette"),
            entry(MENU_SETTINGS, "Settings\u{2026}"),
        ];
        if let Some(release) = &self.updates.newer {
            items.push(entry(MENU_UPDATE, &format!("Update to {}", release.tag)));
        }
        items.push(MenuItem::Separator);
        let primary: Vec<MenuItem> = self
            .plugins
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.enabled)
            .flat_map(|(index, slot)| {
                let base = MENU_PLUGIN_ACTION_BASE + index as u32 * 100;
                let first = slot
                    .plugin
                    .tray_actions()
                    .into_iter()
                    .next()
                    .map(|action| entry(base + action.id, action.label));
                let toggles =
                    slot.plugin
                        .tray_toggles()
                        .into_iter()
                        .map(move |toggle| MenuItem::Entry {
                            id: base + toggle.id,
                            label: toggle.label.to_string(),
                            checked: toggle.on,
                        });
                first.into_iter().chain(toggles)
            })
            .collect();
        if !primary.is_empty() {
            items.extend(primary);
            items.push(MenuItem::Separator);
        }
        items.push(entry(MENU_QUIT, "Quit WinCraft"));
        items
    }

    pub(super) fn snapshot(&self) -> UiSnapshot {
        let plugins: Vec<PluginInfo> = (0..self.plugins.slots.len())
            .map(|index| {
                let slot = &self.plugins.slots[index];
                let meta = slot.plugin.metadata();
                let settings = self.plugins.settings_of(meta.id);
                PluginInfo {
                    id: meta.id.to_string(),
                    name: meta.name.to_string(),
                    version: meta.version.to_string(),
                    author: meta.author.to_string(),
                    description: meta.description.to_string(),
                    readme: meta.readme.to_string(),
                    category: meta.category.to_string(),
                    icon: meta.icon.map(str::to_string),
                    enabled: slot.enabled,
                    config_path: config::plugin_path(meta.id),
                    hotkeys: self.plugins.hotkey_infos(index),
                    fields: field_infos(slot.plugin.as_ref(), &settings),
                    status: slot.plugin.status(),
                    page_action: page_button(slot.plugin.as_ref(), index),
                }
            })
            .collect();

        let update = self.update_view();
        let commands = self.commands_with(&plugins);
        UiSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            start_with_windows: self.config.start_with_windows,
            autostart_note: self.autostart_note.clone(),
            theme: self.config.theme,
            mode: self.config.mode,
            palette_hotkey: self.config.palette_hotkey.clone(),
            palette_hotkey_registered: self
                .plugins
                .palette_hotkey
                .map(|(_, ok)| ok)
                .unwrap_or(false),
            config_path: config::config_path(),
            log_path: config::log_path(),
            shortcuts: self.shortcut_infos(&commands),
            commands,
            plugins,
            search: self.config.search.clone(),
            tips: self.config.tips.clone(),
            update,
        }
    }

    pub(super) fn search_router(&self) -> Router {
        let mut providers: Vec<(Option<String>, Box<dyn SearchProvider>)> =
            search::providers::built_in()
                .into_iter()
                .map(|provider| (None, provider))
                .collect();
        for slot in &self.plugins.slots {
            let id = slot.plugin.metadata().id;
            for provider in slot.plugin.search_providers() {
                providers.push((Some(id.to_string()), provider));
            }
        }
        let mut router = Router::new(providers, &self.config.search.prefixes);
        router.load_usage(config::data_dir().join("palette_usage.json"));
        router
    }

    /// Every plugin's own windows, for the UI thread, each with its plugin.
    pub(super) fn plugin_windows(&self) -> Vec<(String, Box<dyn PluginWindow>)> {
        self.plugins
            .slots
            .iter()
            .flat_map(|slot| {
                let id = slot.plugin.metadata().id;
                slot.plugin
                    .windows()
                    .into_iter()
                    .map(move |window| (id.to_string(), window))
            })
            .collect()
    }

    /// Every change that can switch a plugin on or off, or change a
    /// shortcut, ends here, so the shortcuts are registered again first.
    pub(super) fn publish(&mut self) {
        self.register_shortcuts();
        self.to_ui
            .send(UiCommand::Snapshot(Box::new(self.snapshot())));
    }

    /// The palette's commands, with the shortcuts the host keeps filled in.
    pub(super) fn commands(&self) -> Vec<PaletteEntry> {
        let plugins: Vec<PluginInfo> = (0..self.plugins.slots.len())
            .map(|index| PluginInfo {
                hotkeys: self.plugins.hotkey_infos(index),
                ..PluginInfo::default()
            })
            .collect();
        self.commands_with(&plugins)
    }

    pub(super) fn commands_with(&self, plugins: &[PluginInfo]) -> Vec<PaletteEntry> {
        let mut entries = self.plugins.palette_entries(plugins);
        announce_update(&mut entries, &self.update_view());
        for entry in &mut entries {
            let hotkey = matches!(
                entry.id,
                CommandId::Plugin {
                    kind: ActionKind::Hotkey,
                    ..
                }
            );
            if !hotkey {
                entry.shortcut = self
                    .config
                    .shortcuts
                    .get(&entry.shortcut_id)
                    .and_then(|text| shortcuts::parse(text))
                    .map(|keys| hotkeys::format_shortcut(&keys))
                    .unwrap_or_default();
            }
        }
        entries
    }

    /// The rows of the Hotkeys page: the palette hotkey, then
    /// every command the palette lists.
    pub(super) fn shortcut_infos(&self, commands: &[PaletteEntry]) -> Vec<ShortcutInfo> {
        let mut infos = vec![ShortcutInfo {
            id: "wincraft.palette".to_string(),
            command: None,
            group: shortcuts::HOST_GROUP.to_string(),
            plugin: None,
            category: String::new(),
            icon: None,
            label: "Open the palette".to_string(),
            description: "Search apps, windows, files and commands".to_string(),
            binding: self.config.palette_hotkey.clone(),
            default_binding: DEFAULT_PALETTE_HOTKEY.to_string(),
            registered: self.plugins.palette_hotkey.is_none_or(|(_, ok)| ok),
            one_press: true,
        }];
        for entry in commands {
            let index = match entry.id {
                CommandId::Plugin { index, .. } => Some(index),
                CommandId::Host(HostCommand::TogglePlugin(index)) => Some(index),
                CommandId::Host(_) => None,
            };
            let meta = index
                .and_then(|index| self.plugins.slots.get(index))
                .map(|slot| slot.plugin.metadata());
            let category = meta
                .as_ref()
                .map(|meta| meta.category.to_string())
                .unwrap_or_default();
            let icon = meta.and_then(|meta| meta.icon).map(str::to_string);
            let (default_binding, registered) = match entry.id {
                CommandId::Plugin {
                    index,
                    kind: ActionKind::Hotkey,
                    action,
                } => {
                    let name = self.plugins.action_name(index, action);
                    self.plugins
                        .hotkey_infos(index)
                        .into_iter()
                        .find(|info| info.action == name)
                        .map_or((String::new(), true), |info| {
                            (info.default_binding, info.registered)
                        })
                }
                _ => {
                    let first = shortcuts::parse(&entry.shortcut).map(|keys| keys.first);
                    let ok = first.is_none_or(|first| {
                        self.shortcut_keys
                            .iter()
                            .find(|(_, press, _)| hotkeys::same(press.hotkey, first))
                            .is_none_or(|(_, _, ok)| *ok)
                    });
                    (String::new(), ok)
                }
            };
            infos.push(ShortcutInfo {
                id: entry.shortcut_id.clone(),
                command: Some(entry.id),
                group: entry.group.clone(),
                plugin: entry.plugin.clone(),
                category,
                icon,
                label: entry.label.clone(),
                description: entry.subtitle.clone().unwrap_or_default(),
                binding: entry.shortcut.clone(),
                default_binding,
                registered,
                one_press: false,
            });
        }
        infos
    }
}

/// A known newer release puts a badge on "Update WinCraft", for when it is
/// typed. Under an empty box the palette pins its own "Update ready" row
/// instead, so the command stays found by typing only. A copy that cannot
/// update itself keeps the plain entry.
pub(super) fn announce_update(entries: &mut [PaletteEntry], update: &UpdateView) {
    let (Some(release), None) = (&update.available, &update.blocked) else {
        return;
    };
    let version = release.tag.trim_start_matches('v');
    for entry in entries {
        if entry.id == CommandId::Host(HostCommand::UpdateWinCraft) {
            entry.badge = Some(format!("{version} available"));
            entry.subtitle = Some(format!("Installs WinCraft {version} now"));
        }
    }
}

/// Whether Ctrl+Del hid the release `tag` from the palette: `hidden` is the
/// tag hidden then, and only a newer tag shows again.
pub(super) fn update_hidden(tag: &str, hidden: &str) -> bool {
    !hidden.is_empty() && !github::is_newer(tag, hidden)
}

pub(super) fn field_infos(plugin: &dyn WinCraftPlugin, settings: &Value) -> Vec<FieldInfo> {
    let fixed = plugin.settings_fields().into_iter().map(|field| FieldInfo {
        key: field.key.to_string(),
        label: field.label.to_string(),
        help: field.help.to_string(),
        kind: field.kind,
        value: settings.get(field.key).cloned().unwrap_or(Value::Null),
    });
    let dynamic = plugin.dynamic_fields().into_iter().map(|field| FieldInfo {
        key: field.key,
        label: field.label,
        help: field.help,
        kind: field.kind,
        value: field.value,
    });
    fixed.chain(dynamic).collect()
}

/// The button under a plugin's status line: one of its hotkey actions, or
/// failing that one of its palette commands.
pub(super) fn page_button(
    plugin: &dyn WinCraftPlugin,
    index: usize,
) -> Option<(String, CommandId)> {
    let action = plugin.page_action()?;
    let hotkey = plugin
        .hotkey_actions()
        .into_iter()
        .find(|hotkey| hotkey.id == action)
        .map(|hotkey| (hotkey.label, ActionKind::Hotkey));
    let found = hotkey.or_else(|| {
        plugin
            .palette_commands()
            .into_iter()
            .find(|command| command.id == action)
            .map(|command| (command.label, ActionKind::Palette))
    });
    found.map(|(label, kind)| {
        (
            label.to_string(),
            CommandId::Plugin {
                index,
                kind,
                action,
            },
        )
    })
}
