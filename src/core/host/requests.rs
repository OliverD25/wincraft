use super::*;

impl Host {
    pub(super) fn run_command(&mut self, id: CommandId) {
        match id {
            CommandId::Host(HostCommand::OpenSettings) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::General))
            }
            CommandId::Host(HostCommand::OpenStore) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::Store))
            }
            CommandId::Host(HostCommand::OpenLayout) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::Layout))
            }
            CommandId::Host(HostCommand::OpenAbout) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::About))
            }
            CommandId::Host(HostCommand::OpenConfig) => open_in_notepad(&config::config_path()),
            CommandId::Host(HostCommand::OpenLog) => open_in_notepad(&config::log_path()),
            CommandId::Host(HostCommand::UpdateWinCraft) => self.update_from_palette(),
            CommandId::Host(HostCommand::Exit) => self.shutdown(),
            CommandId::Host(HostCommand::TogglePlugin(index)) => {
                if index < self.plugins.slots.len() {
                    if self.plugins.slots[index].enabled {
                        self.plugins.disable(index);
                    } else {
                        self.enable_slot(index);
                    }
                    self.publish();
                }
            }
            CommandId::Plugin {
                index,
                kind,
                action,
            } => {
                let Some(slot) = self.plugins.slots.get_mut(index) else {
                    return;
                };
                if !slot.enabled {
                    return;
                }
                match kind {
                    ActionKind::Hotkey => {
                        // From the palette, not the keyboard: the action runs
                        // at once, without the hotkey's wait for a digit.
                        LAST_HOTKEY.with(|cell| cell.borrow_mut().take());
                        slot.plugin.on_hotkey(action)
                    }
                    ActionKind::Tray => slot.plugin.on_tray_action(action),
                    ActionKind::Palette => slot.plugin.on_palette_command(action),
                }
            }
        }
    }

    /// The rows a plugin offers for one of its commands.
    pub(super) fn command_list(
        &mut self,
        id: CommandId,
    ) -> Option<crate::core::ui_bridge::SubList> {
        let CommandId::Plugin {
            index,
            kind,
            action,
        } = id
        else {
            return None;
        };
        let slot = self
            .plugins
            .slots
            .get_mut(index)
            .filter(|slot| slot.enabled)?;
        slot.plugin.command_list(kind, action)
    }

    /// What a plugin says about one of its commands for the inspector.
    pub(super) fn command_details(
        &mut self,
        id: CommandId,
    ) -> Option<crate::core::ui_bridge::CommandDetails> {
        let CommandId::Plugin {
            index,
            kind,
            action,
        } = id
        else {
            return None;
        };
        let slot = self
            .plugins
            .slots
            .get_mut(index)
            .filter(|slot| slot.enabled)?;
        slot.plugin.command_details(kind, action)
    }

    /// Every enabled plugin's toggle commands and their state now.
    pub(super) fn toggle_states(&mut self) -> Vec<(CommandId, ToggleState)> {
        let mut states = Vec::new();
        for (index, slot) in self.plugins.slots.iter_mut().enumerate() {
            if !slot.enabled {
                continue;
            }
            for command in slot.plugin.palette_commands() {
                if command.toggle.is_none() {
                    continue;
                }
                let state = slot
                    .plugin
                    .toggle_state(ActionKind::Palette, command.id)
                    .unwrap_or_else(ToggleState::unknown);
                let id = CommandId::Plugin {
                    index,
                    kind: ActionKind::Palette,
                    action: command.id,
                };
                states.push((id, state));
            }
        }
        states
    }

    /// After the palette was told to show: the toggles' state, and an update
    /// check when the last one is old. Its answer shows from the next
    /// opening, since the palette keeps what it saw when it opened.
    pub(super) fn palette_opened(&mut self) {
        self.send_toggle_states();
        self.automatic_check(update_timing::Trigger::PaletteOpen);
    }

    /// Sends the toggles' state to the palette. Called after the palette
    /// was told to show, so reading does not hold up its first frame.
    pub(super) fn send_toggle_states(&mut self) {
        let states = self.toggle_states();
        self.to_ui.send(UiCommand::ToggleStates(states));
    }

    fn is_toggle(&self, id: CommandId) -> bool {
        self.palette_command(id)
            .is_some_and(|command| command.toggle.is_some())
    }

    /// A plugin's palette command by its id.
    fn palette_command(&self, id: CommandId) -> Option<crate::core::traits::PaletteCommand> {
        let CommandId::Plugin {
            index,
            kind: ActionKind::Palette,
            action,
        } = id
        else {
            return None;
        };
        let slot = self.plugins.slots.get(index)?;
        slot.plugin
            .palette_commands()
            .into_iter()
            .find(|command| command.id == action)
    }

    /// The text a palette command shows before its second Enter, when it
    /// asks for one.
    #[cfg(test)]
    pub(super) fn confirm_of(&self, id: CommandId) -> Option<(String, String)> {
        self.palette_command(id)
            .filter(|command| !command.confirm.is_empty())
            .map(|command| (command.label.to_string(), command.confirm.to_string()))
    }

    /// A hotkey's command. One that asks for Enter twice, such as emptying
    /// the Recycle Bin, or one that offers rows to pick from, is not run
    /// from a key: the palette opens on it, so the user sees what it will do.
    pub(super) fn run_from_key(&mut self, id: CommandId) {
        let Some(label) = self
            .palette_command(id)
            .filter(|command| !command.confirm.is_empty() || command.list)
            .map(|command| command.label.to_string())
        else {
            self.run_command(id);
            return;
        };
        log::info!("the hotkey for \"{label}\" opens the palette on it");
        search::actions::remember_front();
        self.to_ui.send(UiCommand::ShowPalette(cursor_monitor()));
        self.to_ui
            .send(UiCommand::TypeInPalette(format!("!{label}")));
        bring_to_front(self.palette_hwnd);
        self.palette_opened();
    }

    pub(super) fn handle_request(&mut self, request: HostRequest) {
        match request {
            HostRequest::UiReady { palette_hwnd } => {
                self.palette_hwnd = palette_hwnd as HWND;
                clear_system_frame(self.palette_hwnd);
                self.publish();
            }
            HostRequest::RunCommand(id) => {
                self.run_command(id);
                // A toggle runs with the palette left open: its row shows
                // the state Windows has now.
                if self.is_toggle(id) {
                    self.send_toggle_states();
                }
            }
            HostRequest::OpenList(id) => {
                let list = self.command_list(id);
                self.to_ui.send(UiCommand::CommandList { id, list });
            }
            HostRequest::ListPick { id, row } => {
                if let CommandId::Plugin {
                    index,
                    kind,
                    action,
                } = id
                {
                    match self.plugins.slots.get_mut(index) {
                        Some(slot) if slot.enabled => slot.plugin.on_list_pick(kind, action, &row),
                        _ => log::info!("the plugin is off; the pick was dropped"),
                    }
                }
            }
            HostRequest::CommandDetails(id) => {
                let details = self.command_details(id);
                self.to_ui.send(UiCommand::CommandDetails { id, details });
            }
            HostRequest::RunAction(search::Action::Command(id)) => self.run_command(id),
            HostRequest::RunAction(search::Action::Arrange(hwnd)) => {
                if self.send_arrange_for(true, false) {
                    self.to_ui.send(UiCommand::ArrangeSelectWindow(hwnd));
                }
            }
            HostRequest::RunAction(action) => search::actions::perform(&action, self.hwnd),
            HostRequest::PromptAnswer {
                plugin,
                id,
                target,
                text,
            } => match self.plugins.index_of(&plugin) {
                Some(index) if self.plugins.slots[index].enabled => self.plugins.slots[index]
                    .plugin
                    .on_prompt_answer(id, target, &text),
                _ => log::info!("{plugin} is off; the answer to its question was dropped"),
            },
            HostRequest::Arrange { plugin, action } => {
                if skipped_in_test_instance(&action, instance::is_test()) {
                    log::info!("read-only test instance: renaming a window skipped");
                    return;
                }
                let owner = if action.is_about_names() {
                    NAMER_ID
                } else {
                    plugin.as_str()
                };
                self.arrange_action(owner, &action);
                if !matches!(
                    action,
                    ArrangeAction::Activate(_) | ArrangeAction::NewWindow { .. }
                ) {
                    self.send_arrange(false);
                }
            }
            HostRequest::LayoutRefresh => self.send_layout(),
            HostRequest::Layout { plugin, action } => {
                match self.plugins.index_of(&plugin) {
                    Some(index) if self.plugins.slots[index].enabled => {
                        self.plugins.slots[index].plugin.on_layout_action(&action)
                    }
                    _ => log::info!("{plugin} is off; the Layout page's request was dropped"),
                }
                self.send_layout();
            }
            HostRequest::FocusWindow(hwnd) => bring_to_front(hwnd as HWND),
            HostRequest::ArrangeDigit(digit) => self.arrange_digit(digit),
            HostRequest::ClearPaletteUsage => self.to_ui.send(UiCommand::ClearPaletteUsage),
            HostRequest::SetPluginEnabled { id, enabled } => {
                if let Some(index) = self.plugins.index_of(&id) {
                    if enabled {
                        self.enable_slot(index);
                    } else {
                        self.plugins.disable(index);
                    }
                    self.publish();
                }
            }
            HostRequest::SetHotkey {
                plugin,
                action,
                binding,
            } => self.set_hotkey(&plugin, &action, &binding),
            HostRequest::SetSetting { plugin, key, value } => {
                self.set_setting(&plugin, &key, value)
            }
            HostRequest::SetShortcuts(changes) => self.set_shortcuts(changes),
            HostRequest::ResetPlugin(id) => self.reset_plugin(&id),
            HostRequest::SetHostSetting(setting) => self.set_host_setting(setting),
            HostRequest::HideUpdate(tag) => self.hide_update(tag),
            HostRequest::CheckForUpdates => self.check_for_updates(true),
            HostRequest::UpdateNow => {
                if !self.updates.busy && self.updates.blocked.is_none() {
                    self.updates.reopen = Some(update::Reopen::About);
                }
                self.update_now();
            }
            HostRequest::CancelUpdate => {
                if self.updates.busy && matches!(self.updates.status, UpdateStatus::Downloading(_))
                {
                    self.updates.cancel.store(true, Ordering::Relaxed);
                }
            }
            HostRequest::OpenPath(path) => open_in_notepad(&path),
            HostRequest::Exit => self.shutdown(),
        }
    }

    pub(super) fn arrange_action(&mut self, plugin: &str, action: &ArrangeAction) {
        match self.plugins.index_of(plugin) {
            Some(index) if self.plugins.slots[index].enabled => {
                self.plugins.slots[index].plugin.on_arrange_action(action)
            }
            _ => log::info!("{plugin} is off; the strip's request was dropped"),
        }
    }

    pub(super) fn set_hotkey(&mut self, plugin_id: &str, action: &str, binding: &str) {
        self.plugins.set_hotkey(plugin_id, action, binding);
        self.show_warnings();
        self.publish();
    }

    pub(super) fn set_setting(&mut self, plugin_id: &str, key: &str, value: Value) {
        self.plugins.set_setting(plugin_id, key, value);
        self.show_warnings();
        self.publish();
    }

    pub(super) fn reset_plugin(&mut self, plugin_id: &str) {
        if self.plugins.reset(plugin_id) {
            self.show_warnings();
            self.publish();
        }
    }

    pub(super) fn set_host_setting(&mut self, setting: HostSetting) {
        match setting {
            HostSetting::StartWithWindows(wanted) => match autostart::set(wanted) {
                Ok(()) => {
                    self.config.start_with_windows = wanted;
                    log::info!("start with Windows: {wanted}");
                }
                Err(err) => log::error!("could not change autostart: {err}"),
            },
            HostSetting::Search(search) => self.config.search = search,
            HostSetting::Tips(tips) => self.config.tips = tips,
            HostSetting::CheckUpdatesDaily(on) => {
                self.config.updates.check_daily = on;
                log::info!("check for updates automatically: {on}");
            }
            HostSetting::DownloadUpdatesAhead(on) => self.set_download_ahead(on),
            HostSetting::Theme(name) => {
                self.config.theme = name;
                theme::set_current(name, self.config.mode);
                log::info!("theme: {}", name.label());
                self.to_ui.send(UiCommand::ThemeChanged);
            }
            HostSetting::Mode(mode) => {
                self.config.mode = mode;
                theme::set_current(self.config.theme, mode);
                self.to_ui.send(UiCommand::ThemeChanged);
            }
        }
        self.save_config();
        self.publish();
    }

    pub(super) fn save_config(&self) {
        if let Err(err) = self.config.save_in(&self.plugins.data) {
            log::error!("could not save config.json: {err}");
        }
    }

    pub(super) fn shutdown(&mut self) {
        log::info!("WinCraft is shutting down");
        self.report_healthy(true);
        self.to_ui.send(UiCommand::Quit);
        self.plugins.stop_all();
        self.end_wait();
        for (id, _, ok) in self.shortcut_keys.drain(..) {
            if ok {
                self.plugins.keys.unregister(id);
            }
        }
        unsafe { DeregisterShellHookWindow(self.hwnd) };
        self.tray.remove();
        unsafe { PostQuitMessage(0) };
    }
}
