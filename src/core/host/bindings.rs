use super::*;

impl Host {
    /// The commands with a shortcut the host registers itself, of WinCraft
    /// and of the plugins that are on.
    pub(super) fn bound_shortcuts(&self) -> Vec<(Shortcut, CommandId)> {
        let mut bound = self.plugins.plugin_chords();
        for entry in self.plugins.palette_entries(&[]) {
            let hotkey = matches!(
                entry.id,
                CommandId::Plugin {
                    kind: ActionKind::Hotkey,
                    ..
                }
            );
            if entry.disabled || hotkey {
                continue;
            }
            let keys = self
                .config
                .shortcuts
                .get(&entry.shortcut_id)
                .and_then(|text| shortcuts::parse(text));
            if let Some(keys) = keys {
                bound.push((keys, entry.id));
            }
        }
        bound
    }

    /// Registers the first presses the shortcuts need, when they changed.
    pub(super) fn register_shortcuts(&mut self) {
        let plan = shortcuts::plan(&self.bound_shortcuts());
        let same = plan.len() == self.shortcut_keys.len()
            && plan
                .iter()
                .zip(&self.shortcut_keys)
                .all(|(wanted, (_, have, _))| wanted == have);
        if same {
            return;
        }
        for (id, _, ok) in self.shortcut_keys.drain(..) {
            if ok {
                self.plugins.keys.unregister(id);
            }
        }
        for (offset, press) in plan.into_iter().enumerate() {
            let id = SHORTCUT_ID_BASE + offset as i32;
            let keys = hotkeys::format(press.hotkey);
            let ok = self.plugins.keys.register(id, press.hotkey);
            if ok {
                log::info!("registered {keys} for a shortcut");
            } else {
                let taken = self.plugins.taken_message(press.hotkey);
                log::warn!("could not register {keys} for a shortcut: {taken}");
            }
            self.shortcut_keys.push((id, press, ok));
        }
    }

    /// A shortcut's first press came: run its command, or wait three
    /// seconds for the second press of a chord.
    pub(super) fn shortcut_pressed(&mut self, global_id: i32) {
        let Some((_, press, _)) = self
            .shortcut_keys
            .iter()
            .find(|(id, _, _)| *id == global_id)
            .cloned()
        else {
            return;
        };
        self.end_wait();
        if press.seconds.is_empty() {
            if let Some(command) = press.plain {
                self.run_command(command);
            }
            return;
        }
        let keys = hotkeys::format(press.hotkey);
        if !chord::listen(
            self.hwnd as isize,
            WM_APP_WAIT_KEY,
            shortcuts::CHORD_WAIT,
            chord_rule(press.clone()),
        ) {
            return;
        }
        log::info!("{keys}: waiting 3 s for the second key of a chord");
        self.chord_wait = Some(press);
        self.wait_hint = Some(ui::taskbar_badges::WaitHint::line_only(
            &shortcuts::wait_line(&keys),
        ));
    }

    /// Applies the Hotkeys page's changes in order, so Replace
    /// frees the keys before the new command takes them.
    pub(super) fn set_shortcuts(&mut self, changes: Vec<ShortcutChange>) {
        let infos = self.shortcut_infos(&self.commands());
        for change in changes {
            let Some(info) = infos.iter().find(|info| info.id == change.id) else {
                log::warn!("no command {} for a shortcut", change.id);
                continue;
            };
            let text = change
                .binding
                .clone()
                .unwrap_or_else(|| info.default_binding.clone());
            let text = match shortcuts::parse(&text) {
                Some(keys) => hotkeys::format_shortcut(&keys),
                None => String::new(),
            };
            match info.command {
                None => {
                    // The palette always has a hotkey: removing it gives
                    // the default back.
                    let text = if text.is_empty() {
                        DEFAULT_PALETTE_HOTKEY.to_string()
                    } else {
                        text
                    };
                    match self.plugins.rebind_palette(&text) {
                        Ok(text) => self.config.palette_hotkey = text,
                        Err(err) => log::warn!("palette hotkey: {err}"),
                    }
                }
                Some(CommandId::Plugin {
                    index,
                    kind: ActionKind::Hotkey,
                    action,
                }) => {
                    let name = self.plugins.action_name(index, action);
                    if let Some(plugin) = &info.plugin {
                        self.plugins.set_shortcut_text(plugin, name, &text);
                    }
                }
                Some(_) => {
                    if text.is_empty() {
                        self.config.shortcuts.remove(&info.id);
                    } else {
                        self.config.shortcuts.insert(info.id.clone(), text.clone());
                    }
                    log::info!("{} shortcut set to \"{text}\"", info.id);
                }
            }
        }
        self.show_warnings();
        self.save_config();
        self.publish();
    }
}
