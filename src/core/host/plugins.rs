use super::*;

/// Global hotkeys, behind a trait so the bookkeeping runs in tests with a
/// fake that never registers anything with Windows.
pub(super) trait Registrar {
    /// False when the keys are taken, by another app or by WinCraft itself.
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool;
    fn unregister(&mut self, id: i32);
}

/// The hotkeys of the host window.
pub(super) struct WindowHotkeys(pub(super) HWND);

impl Registrar for WindowHotkeys {
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool {
        unsafe { RegisterHotKey(self.0, id, hotkey.modifiers, hotkey.vk) != 0 }
    }

    fn unregister(&mut self, id: i32) {
        unsafe { UnregisterHotKey(self.0, id) };
    }
}

/// The plugins and their hotkeys: which plugins run, what each one's
/// settings file says, and which global hotkeys are registered. Kept apart
/// from the tray and the UI, so it runs in tests with fake plugins and a
/// fake registrar.
pub(super) struct PluginSet {
    /// The host window, which plugins get in their `HostContext`.
    pub(super) hwnd: HWND,
    pub(super) slots: Vec<PluginSlot>,
    pub(super) configs: BTreeMap<String, PluginConfig>,
    pub(super) keys: Box<dyn Registrar>,
    pub(super) next_hotkey_id: i32,
    pub(super) palette_hotkey: Option<(Hotkey, bool)>,
    /// The folder of config.json and the plugin files.
    pub(super) data: PathBuf,
    /// Balloons for the host to show; a click on one opens ShortcutDetector.
    pub(super) warnings: Vec<String>,
}

impl PluginSet {
    pub(super) fn new(
        hwnd: HWND,
        plugins: Vec<Box<dyn WinCraftPlugin>>,
        keys: Box<dyn Registrar>,
        data: PathBuf,
    ) -> Self {
        Self {
            hwnd,
            slots: plugins
                .into_iter()
                .map(|plugin| PluginSlot {
                    plugin,
                    enabled: false,
                    registered: Vec::new(),
                })
                .collect(),
            configs: BTreeMap::new(),
            keys,
            next_hotkey_id: 1,
            palette_hotkey: None,
            data,
            warnings: Vec::new(),
        }
    }

    pub(super) fn load_configs(&mut self) {
        for index in 0..self.slots.len() {
            self.load_config(index);
        }
    }

    pub(super) fn load_config(&mut self, index: usize) {
        let slot = &self.slots[index];
        let meta = slot.plugin.metadata();
        let defaults: Vec<(&str, String)> = slot
            .plugin
            .hotkey_actions()
            .iter()
            .map(|action| (action.name, hotkeys::format(action.default)))
            .collect();
        let settings = slot.plugin.default_settings();

        let mut plugin = PluginConfig::load_in(&self.data, meta.id);
        if let Some(from) = slot.plugin.split_from() {
            let first_start = !PluginConfig::exists_in(&self.data, meta.id);
            if first_start && PluginConfig::exists_in(&self.data, from) {
                let mut old = self
                    .configs
                    .get(from)
                    .cloned()
                    .unwrap_or_else(|| PluginConfig::load_in(&self.data, from));
                if slot.plugin.take_over(&mut old, &mut plugin) {
                    log::info!(
                        "{}: took over its settings from {from}; it starts {}",
                        meta.id,
                        if plugin.enabled { "on" } else { "off" }
                    );
                    if let Err(err) = old.save_in(&self.data, from) {
                        log::error!("{err}");
                    }
                    if self.configs.contains_key(from) {
                        self.configs.insert(from.to_string(), old);
                    }
                }
            }
        }
        plugin.merge_defaults(&defaults, &settings);
        slot.plugin
            .migrate_hotkeys(&mut plugin.hotkeys, &mut plugin.settings);
        slot.plugin.migrate_settings(&mut plugin.settings);
        self.configs.insert(meta.id.to_string(), plugin);
        self.save(meta.id);
    }

    /// The plugins to start: those their file has on, and any without one.
    pub(super) fn wanted(&self) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|index| {
                let id = self.slots[*index].plugin.metadata().id;
                self.configs
                    .get(id)
                    .map(|plugin| plugin.enabled)
                    .unwrap_or(true)
            })
            .collect()
    }

    pub(super) fn register_palette(&mut self, text: &str) {
        let hotkey = match hotkeys::parse(text) {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("palette_hotkey \"{text}\": {err}; using {DEFAULT_PALETTE_HOTKEY}");
                hotkeys::parse(DEFAULT_PALETTE_HOTKEY).expect("the built-in default parses")
            }
        };
        let ok = self.keys.register(PALETTE_HOTKEY_ID, hotkey);
        let keys = hotkeys::format(hotkey);
        if ok {
            log::info!("registered {keys} for host.palette");
        } else {
            let taken = self.taken_message(hotkey);
            log::warn!("could not register {keys} for host.palette: {taken}");
            self.warnings.push(taken);
        }
        self.palette_hotkey = Some((hotkey, ok));
    }

    /// Who holds keys that could not be registered: a WinCraft command,
    /// whose name the user can look for in Settings, or another app.
    pub(super) fn holder(&self, hotkey: Hotkey) -> Option<String> {
        if self
            .palette_hotkey
            .is_some_and(|(held, ok)| ok && hotkeys::same(held, hotkey))
        {
            return Some("Open the palette".to_string());
        }
        self.slots.iter().find_map(|slot| {
            slot.registered
                .iter()
                .find(|entry| entry.active && entry.ok && hotkeys::same(entry.hotkey, hotkey))
                .map(|entry| format!("{} ({})", entry.label, slot.plugin.metadata().name))
        })
    }

    pub(super) fn taken_message(&self, hotkey: Hotkey) -> String {
        let keys = hotkeys::format(hotkey);
        match self.holder(hotkey) {
            Some(holder) => format!("{keys} is used by {holder} in WinCraft"),
            None => format!("{keys} is used by another app"),
        }
    }

    /// Frees the palette hotkey and registers `binding` instead. Returns
    /// the binding as the config file spells it.
    pub(super) fn rebind_palette(&mut self, binding: &str) -> Result<String, String> {
        let hotkey = hotkeys::parse(binding)?;
        self.free_palette();
        let text = hotkeys::format(hotkey);
        self.register_palette(&text);
        Ok(text)
    }

    pub(super) fn free_palette(&mut self) {
        if let Some((_, true)) = self.palette_hotkey {
            self.keys.unregister(PALETTE_HOTKEY_ID);
        }
    }

    pub(super) fn config(&self, id: &str) -> Option<&PluginConfig> {
        self.configs.get(id)
    }

    pub(super) fn config_mut(&mut self, id: &str) -> &mut PluginConfig {
        self.configs.entry(id.to_string()).or_default()
    }

    pub(super) fn settings_of(&self, id: &str) -> Value {
        self.config(id)
            .map(|plugin| plugin.settings.clone())
            .unwrap_or_else(|| serde_json::json!({}))
    }

    /// The action's shortcut: the file's text, one press or a chord; None
    /// when the user removed it (empty text); the default when the text
    /// does not parse.
    pub(super) fn resolve_shortcut(
        &self,
        plugin_id: &str,
        name: &str,
        default: Hotkey,
    ) -> Option<Shortcut> {
        let Some(text) = self
            .config(plugin_id)
            .and_then(|plugin| plugin.hotkeys.get(name))
        else {
            return Some(Shortcut::single(default));
        };
        if text.trim().is_empty() {
            return None;
        }
        match hotkeys::parse_shortcut(text) {
            Ok(keys) => Some(keys),
            Err(err) => {
                log::warn!("{plugin_id}.{name}: {err}; using the plugin default");
                Some(Shortcut::single(default))
            }
        }
    }

    /// Writes an action's shortcut text (empty for none) to the plugin's
    /// file and starts the plugin again with it.
    pub(super) fn set_shortcut_text(&mut self, plugin_id: &str, action: &str, text: &str) {
        self.config_mut(plugin_id)
            .hotkeys
            .insert(action.to_string(), text.to_string());
        self.save(plugin_id);
        if let Some(index) = self.index_of(plugin_id) {
            self.restart(index);
        }
        log::info!("{plugin_id}.{action} shortcut set to \"{text}\"");
    }

    /// The chords of the plugins that are on: their first press is
    /// registered with the other shortcuts.
    pub(super) fn plugin_chords(&self) -> Vec<(Shortcut, CommandId)> {
        let mut chords = Vec::new();
        for (index, slot) in self.slots.iter().enumerate().filter(|(_, s)| s.enabled) {
            for entry in slot.registered.iter().filter(|entry| !entry.active) {
                if let Ok(keys) = hotkeys::parse_shortcut(&entry.keys) {
                    chords.push((
                        keys,
                        CommandId::Plugin {
                            index,
                            kind: ActionKind::Hotkey,
                            action: entry.action_id,
                        },
                    ));
                }
            }
        }
        chords
    }

    /// Starts a plugin and registers its hotkeys. A plugin whose start fails
    /// stays off for this session but keeps its saved enabled flag, so the
    /// next start tries it again.
    pub(super) fn enable(&mut self, index: usize) {
        if self.slots[index].enabled {
            return;
        }
        let meta = self.slots[index].plugin.metadata();
        let settings = self.settings_of(meta.id);

        let init_result = {
            let ctx = HostContext {
                hwnd: self.hwnd,
                settings: &settings,
            };
            self.slots[index].plugin.init(&ctx)
        };
        if let Err(err) = init_result {
            log::error!("{}: init failed: {err}", meta.id);
            self.warnings
                .push(format!("{} failed to start: {err}", meta.name));
            return;
        }

        let actions: Vec<(u32, &'static str, &'static str, Hotkey)> = self.slots[index]
            .plugin
            .hotkey_actions()
            .iter()
            .map(|action| (action.id, action.name, action.label, action.default))
            .collect();

        for (action_id, name, label, default) in actions {
            let shortcut = self.resolve_shortcut(meta.id, name, default);
            let Some(hotkey) = shortcut
                .filter(|keys| !keys.is_chord())
                .map(|keys| keys.first)
            else {
                let keys = shortcut
                    .map(|keys| hotkeys::format_shortcut(&keys))
                    .unwrap_or_default();
                self.slots[index].registered.push(RegisteredHotkey {
                    action_id,
                    global_id: 0,
                    label,
                    hotkey: shortcut.map_or(default, |keys| keys.first),
                    keys,
                    ok: true,
                    active: false,
                });
                continue;
            };
            let global_id = self.next_hotkey_id;
            self.next_hotkey_id += 1;
            let keys = hotkeys::format(hotkey);
            let ok = self.keys.register(global_id, hotkey);
            if ok {
                log::info!("registered {keys} for {}.{name}", meta.id);
            } else {
                let taken = self.taken_message(hotkey);
                log::warn!("could not register {keys} for {}.{name}: {taken}", meta.id);
                self.warnings.push(taken);
            }
            self.slots[index].registered.push(RegisteredHotkey {
                action_id,
                global_id,
                label,
                hotkey,
                keys,
                ok,
                active: true,
            });
        }

        let bound: Vec<(u32, String)> = self.slots[index]
            .registered
            .iter()
            .map(|entry| (entry.action_id, entry.keys.clone()))
            .collect();
        self.slots[index].plugin.on_hotkeys_bound(&bound);

        self.slots[index].enabled = true;
        self.config_mut(meta.id).enabled = true;
        self.save(meta.id);
        log::info!("{} enabled", meta.id);
    }

    pub(super) fn disable(&mut self, index: usize) {
        if !self.stop(index) {
            return;
        }
        let id = self.slots[index].plugin.metadata().id;
        self.config_mut(id).enabled = false;
        self.save(id);
        log::info!("{id} disabled");
    }

    /// Frees the plugin's hotkeys and tears it down, leaving its saved
    /// enabled flag alone. False when it was not running.
    pub(super) fn stop(&mut self, index: usize) -> bool {
        if !self.slots[index].enabled {
            return false;
        }
        for entry in self.slots[index].registered.drain(..) {
            if entry.active && entry.ok {
                self.keys.unregister(entry.global_id);
            }
        }
        if self.slots[index].plugin.strip_look().is_some() {
            ARRANGE_FREE.with(|cell| cell.set(true));
        }
        self.slots[index].plugin.teardown();
        self.slots[index].enabled = false;
        let id = self.slots[index].plugin.metadata().id.to_string();
        WINDOWS_WANTED.with(|queue| queue.borrow_mut().push((id, None, false)));
        plugin_changed();
        true
    }

    /// Quitting stops every plugin but must not save it as disabled, or
    /// the next start would come up with every plugin switched off.
    pub(super) fn stop_all(&mut self) {
        for index in 0..self.slots.len() {
            if self.stop(index) {
                log::info!("{} stopped", self.slots[index].plugin.metadata().id);
            }
        }
        self.free_palette();
    }

    /// Stops a running plugin and starts it again, for a change it can only
    /// take at start.
    pub(super) fn restart(&mut self, index: usize) {
        if self.slots[index].enabled {
            self.disable(index);
            self.enable(index);
        }
    }

    pub(super) fn set_hotkey(&mut self, plugin_id: &str, action: &str, binding: &str) {
        let hotkey = match hotkeys::parse(binding) {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("{plugin_id}.{action}: {err}");
                return;
            }
        };
        self.config_mut(plugin_id)
            .hotkeys
            .insert(action.to_string(), hotkeys::format(hotkey));
        self.save(plugin_id);

        if let Some(index) = self.index_of(plugin_id) {
            self.restart(index);
        }
        log::info!(
            "{plugin_id}.{action} rebound to {}",
            hotkeys::format(hotkey)
        );
    }

    pub(super) fn set_setting(&mut self, plugin_id: &str, key: &str, value: Value) {
        {
            let plugin = self.config_mut(plugin_id);
            if !plugin.settings.is_object() {
                plugin.settings = serde_json::json!({});
            }
            if key.starts_with('/') {
                config::set_pointer(&mut plugin.settings, key, value);
            } else if let Some(object) = plugin.settings.as_object_mut() {
                object.insert(key.to_string(), value);
            }
        }
        self.save(plugin_id);

        let settings = self.settings_of(plugin_id);
        if let Some(index) = self.index_of(plugin_id) {
            if self.slots[index].enabled && !self.slots[index].plugin.on_settings_changed(&settings)
            {
                self.restart(index);
            }
        }
    }

    /// Deletes the plugin's settings file and starts it again from its
    /// defaults. False when the file could not be deleted.
    pub(super) fn reset(&mut self, plugin_id: &str) -> bool {
        if let Err(err) = PluginConfig::reset_in(&self.data, plugin_id) {
            log::error!("{err}");
            return false;
        }
        let Some(index) = self.index_of(plugin_id) else {
            return false;
        };
        // Stop without saving: `disable` would write the old in-memory
        // settings back over the file just deleted.
        let was_enabled = self.stop(index);
        self.configs.remove(plugin_id);
        self.load_config(index);
        // The defaults say enabled; a plugin that was off must stay off.
        self.config_mut(plugin_id).enabled = was_enabled;
        self.save(plugin_id);
        if was_enabled {
            self.enable(index);
        }
        log::info!("{plugin_id} reset to defaults");
        true
    }

    /// The plugin and action a global hotkey belongs to, while it is on.
    pub(super) fn hotkey_target(&self, global_id: i32) -> Option<(usize, u32)> {
        let target = self.slots.iter().enumerate().find_map(|(index, slot)| {
            slot.registered
                .iter()
                .find(|entry| entry.active && entry.global_id == global_id)
                .map(|entry| (index, entry.action_id))
        })?;
        log::debug!(
            "hotkey {global_id} goes to {}, action {}",
            self.slots[target.0].plugin.metadata().id,
            target.1
        );
        self.slots[target.0].enabled.then_some(target)
    }

    /// The registered hotkeys of the plugins that are on: (plugin name,
    /// action label, keys).
    pub(super) fn registered(&self) -> Vec<(String, String, Hotkey)> {
        let mut found = Vec::new();
        for slot in self.slots.iter().filter(|slot| slot.enabled) {
            let plugin = slot.plugin.metadata().name;
            for entry in slot
                .registered
                .iter()
                .filter(|entry| entry.active && entry.ok)
            {
                found.push((plugin.to_string(), entry.label.to_string(), entry.hotkey));
            }
        }
        found
    }

    pub(super) fn hotkey_infos(&self, index: usize) -> Vec<HotkeyInfo> {
        let slot = &self.slots[index];
        let meta = slot.plugin.metadata();
        let actions = slot.plugin.hotkey_actions();
        let default_of = |action_id: u32| {
            actions
                .iter()
                .find(|action| action.id == action_id)
                .map(|action| hotkeys::format(action.default))
                .unwrap_or_default()
        };
        if slot.enabled {
            return slot
                .registered
                .iter()
                .map(|entry| HotkeyInfo {
                    action: self.action_name(index, entry.action_id).to_string(),
                    label: entry.label.to_string(),
                    binding: entry.keys.clone(),
                    default_binding: default_of(entry.action_id),
                    registered: entry.ok,
                })
                .collect();
        }
        actions
            .iter()
            .map(|action| HotkeyInfo {
                action: action.name.to_string(),
                label: action.label.to_string(),
                binding: self
                    .resolve_shortcut(meta.id, action.name, action.default)
                    .map(|keys| hotkeys::format_shortcut(&keys))
                    .unwrap_or_default(),
                default_binding: hotkeys::format(action.default),
                registered: true,
            })
            .collect()
    }

    pub(super) fn action_name(&self, index: usize, action_id: u32) -> &'static str {
        self.slots[index]
            .plugin
            .hotkey_actions()
            .into_iter()
            .find(|action| action.id == action_id)
            .map(|action| action.name)
            .unwrap_or("")
    }

    pub(super) fn palette_entries(&self, plugins: &[PluginInfo]) -> Vec<PaletteEntry> {
        let mut entries = Vec::new();
        let host_group = "WinCraft".to_string();
        for (command, label) in [
            (HostCommand::OpenSettings, "Open settings"),
            (HostCommand::OpenStore, "Plugin store"),
            (HostCommand::OpenLayout, "Saved window layout"),
            (HostCommand::OpenAbout, "About WinCraft"),
            (HostCommand::OpenConfig, "Open the config file"),
            (HostCommand::OpenLog, "Open the log file"),
            (HostCommand::UpdateWinCraft, "Update WinCraft"),
            (
                HostCommand::DetailedLog,
                detailed_log::toggle_label(crate::core::detailed::on()),
            ),
            (
                HostCommand::DetailedLogMark,
                "Detailed log: Mark this moment",
            ),
            (
                HostCommand::DetailedLogDelete,
                "Detailed log: Delete the files",
            ),
            (HostCommand::Exit, "Quit WinCraft"),
        ] {
            entries.push(PaletteEntry {
                id: CommandId::Host(command),
                group: host_group.clone(),
                label: label.to_string(),
                hint: String::new(),
                words: String::new(),
                confirm: if command == HostCommand::DetailedLogDelete {
                    "Press Enter again to delete every detailed log file. This cannot be undone."
                        .to_string()
                } else {
                    String::new()
                },
                list: false,
                disabled: false,
                plugin: None,
                subtitle: detailed_log::subtitle(command).or_else(|| {
                    (command == HostCommand::UpdateWinCraft)
                        .then(|| "Checks now and installs a newer version at once".to_string())
                }),
                shortcut_id: String::new(),
                shortcut: String::new(),
                quiet: matches!(
                    command,
                    HostCommand::OpenSettings
                        | HostCommand::OpenStore
                        | HostCommand::UpdateWinCraft
                ),
                badge: None,
                toggle: None,
            });
        }
        for (index, slot) in self.slots.iter().enumerate() {
            let meta = slot.plugin.metadata();
            let group = meta.name.to_string();
            entries.push(PaletteEntry {
                id: CommandId::Host(HostCommand::TogglePlugin(index)),
                group: group.clone(),
                label: if slot.enabled {
                    format!("Turn {} off", meta.name)
                } else {
                    format!("Turn {} on", meta.name)
                },
                hint: String::new(),
                words: String::new(),
                confirm: String::new(),
                list: false,
                disabled: false,
                plugin: Some(meta.id.to_string()),
                subtitle: None,
                shortcut_id: String::new(),
                shortcut: String::new(),
                quiet: false,
                badge: None,
                toggle: None,
            });
            // A plugin that is off keeps its commands in the list, dimmed, so
            // its hotkey does not seem to vanish; running one opens its page.
            let disabled = !slot.enabled;
            let plugin = Some(meta.id.to_string());
            let bindings = plugins.get(index).map(|info| &info.hotkeys);
            for action in slot.plugin.hotkey_actions() {
                let hint = bindings
                    .and_then(|list| list.iter().find(|info| info.label == action.label))
                    .map(|info| info.binding.clone())
                    .unwrap_or_default();
                entries.push(PaletteEntry {
                    id: CommandId::Plugin {
                        index,
                        kind: ActionKind::Hotkey,
                        action: action.id,
                    },
                    group: group.clone(),
                    label: action.label.to_string(),
                    hint,
                    words: String::new(),
                    confirm: String::new(),
                    list: false,
                    disabled,
                    plugin: plugin.clone(),
                    subtitle: slot.plugin.palette_subtitle(ActionKind::Hotkey, action.id),
                    shortcut_id: String::new(),
                    shortcut: String::new(),
                    quiet: false,
                    badge: None,
                    toggle: None,
                });
            }
            for action in slot.plugin.tray_actions() {
                if entries
                    .iter()
                    .any(|entry| entry.group == group && entry.label == action.label)
                {
                    continue;
                }
                entries.push(PaletteEntry {
                    id: CommandId::Plugin {
                        index,
                        kind: ActionKind::Tray,
                        action: action.id,
                    },
                    group: group.clone(),
                    label: action.label.to_string(),
                    hint: String::new(),
                    words: String::new(),
                    confirm: String::new(),
                    list: false,
                    disabled,
                    plugin: plugin.clone(),
                    subtitle: slot.plugin.palette_subtitle(ActionKind::Tray, action.id),
                    shortcut_id: String::new(),
                    shortcut: String::new(),
                    quiet: false,
                    badge: None,
                    toggle: None,
                });
            }
            for command in slot.plugin.palette_commands() {
                entries.push(PaletteEntry {
                    id: CommandId::Plugin {
                        index,
                        kind: ActionKind::Palette,
                        action: command.id,
                    },
                    group: group.clone(),
                    label: command.label.to_string(),
                    hint: command.hint.to_string(),
                    words: command.words.to_string(),
                    confirm: command.confirm.to_string(),
                    list: command.list,
                    disabled,
                    plugin: plugin.clone(),
                    subtitle: slot
                        .plugin
                        .palette_subtitle(ActionKind::Palette, command.id),
                    shortcut_id: String::new(),
                    shortcut: String::new(),
                    quiet: false,
                    badge: None,
                    toggle: command.toggle,
                });
            }
        }
        for entry in &mut entries {
            entry.shortcut_id = self.shortcut_id(entry.id);
            if let CommandId::Plugin {
                kind: ActionKind::Hotkey,
                ..
            } = entry.id
            {
                entry.shortcut = entry.hint.clone();
            }
        }
        entries
    }

    /// The id a command has on the Hotkeys page and in
    /// config.json: the plugin's id and the action, never a place in a list.
    pub(super) fn shortcut_id(&self, id: CommandId) -> String {
        let plugin = |index: usize| {
            self.slots
                .get(index)
                .map_or("unknown", |slot| slot.plugin.metadata().id)
        };
        match id {
            CommandId::Host(HostCommand::TogglePlugin(index)) => {
                format!("{}.turn-on-off", plugin(index))
            }
            CommandId::Host(command) => format!("wincraft.{}", host_slug(command)),
            CommandId::Plugin {
                index,
                kind,
                action,
            } => match kind {
                ActionKind::Hotkey => {
                    format!("{}.{}", plugin(index), self.action_name(index, action))
                }
                ActionKind::Tray => format!("{}.tray-{action}", plugin(index)),
                ActionKind::Palette => format!("{}.command-{action}", plugin(index)),
            },
        }
    }

    /// The strip's windows: from an enabled plugin that keeps a window order
    /// (LayoutKeeper), else from the plugin that draws the strip
    /// (`look_plugin`, WindowDeck), which lists the open windows itself.
    pub(super) fn window_groups(
        &mut self,
        look_plugin: &str,
    ) -> Option<(&'static str, WindowGroups)> {
        let mut ask = |own: bool| {
            self.slots.iter_mut().find_map(|slot| {
                let id = slot.plugin.metadata().id;
                if !slot.enabled || (id == look_plugin) != own {
                    return None;
                }
                Some((id, slot.plugin.window_groups()?))
            })
        };
        ask(false).or_else(|| ask(true))
    }

    pub(super) fn index_of(&self, id: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.plugin.metadata().id == id)
    }

    /// Whether the plugin with this id is there and on.
    pub(super) fn is_on(&self, id: &str) -> bool {
        self.index_of(id)
            .is_some_and(|index| self.slots[index].enabled)
    }

    pub(super) fn save(&self, id: &str) {
        let Some(plugin) = self.configs.get(id) else {
            return;
        };
        if let Err(err) = plugin.save_in(&self.data, id) {
            log::error!("could not save the settings for {id}: {err}");
        }
    }
}
