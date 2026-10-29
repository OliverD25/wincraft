use super::*;

impl WinCraftPlugin for LayoutKeeper {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "layout_keeper",
            name: "LayoutKeeper",
            description: "Remembers which desktop and monitor each window is on, the taskbar thumbnail order and which window is in front, and restores them after a reboot.",
            author: "community",
            version: "1.2.1",
            readme: include_str!("README.md"),
            category: "Windows",
            icon: Some("layout-grid"),
            thumbnail: None,
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "programs": DEFAULT_PROGRAMS,
            "excluded_programs": "",
            "restore_on_start": true,
            "settle_seconds": DEFAULT_SETTLE_SECONDS,
            "restore_window_minutes": DEFAULT_RESTORE_MINUTES,
            "snapshot_interval_seconds": DEFAULT_SNAPSHOT_SECONDS,
            "restore_front_order": true,
            "restore_monitors": true,
            "show_restore_progress": true,
        })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: "programs",
                label: "Programs",
                help: "* for every app, or exe names separated by commas.",
                kind: FieldKind::Text,
            },
            SettingField {
                key: "excluded_programs",
                label: "Never touch",
                help: "Exe names to leave alone, separated by commas, such as telegram.exe. Windows helpers are always left alone: rundll32, dllhost, ShellExperienceHost, StartMenuExperienceHost, SearchHost, TextInputHost, LockApp and ShellHost.",
                kind: FieldKind::Text,
            },
            SettingField {
                key: "restore_on_start",
                label: "Restore on start",
                help: "After you sign in to Windows, put the saved layout back once the programs have opened their windows. A later start of WinCraft, such as after an update, restores nothing; Restore layout (Win+Alt+L) does it at any time.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: "settle_seconds",
                label: "Wait for windows (seconds)",
                help: "Each window is put on its monitor and desktop the moment it opens. An app's taskbar order is put back once all its saved windows are open, or once no new window has come for this long.",
                kind: FieldKind::Number {
                    min: 1.0,
                    max: 60.0,
                },
            },
            SettingField {
                key: "restore_window_minutes",
                label: "Wait for apps to open (minutes)",
                help: "An app that has opened no window by then is not restored. Its windows are placed as they open; its taskbar order is put back within 4 minutes, and again when more windows come in the 5 minutes after that.",
                kind: FieldKind::Number {
                    min: 1.0,
                    max: 120.0,
                },
            },
            SettingField {
                key: "snapshot_interval_seconds",
                label: "Save every (seconds)",
                help: "How often the layout is written to disk while nothing else triggers it.",
                kind: FieldKind::Number {
                    min: 5.0,
                    max: 600.0,
                },
            },
            SettingField {
                key: "restore_front_order",
                label: "Restore which window is in front",
                help: "Also put the windows back in their front-to-back order on each desktop.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: "restore_monitors",
                label: "Restore monitors",
                help: "Also move each window back to the monitor it was on, at the same size and place relative to the screen. A window whose monitor is not connected stays where it is.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: "show_restore_progress",
                label: "Show restore progress",
                help: "While windows are being put back, show a small please-wait panel at the bottom right and the progress in the tray icon's tooltip.",
                kind: FieldKind::Toggle,
            },
        ]
    }

    fn migrate_settings(&self, settings: &mut serde_json::Value) {
        if migrate_programs(settings) {
            log::info!(
                "programs was the old default \"{OLD_DEFAULT_PROGRAMS}\"; now \"*\", every app"
            );
        }
        for old in [OLD_DEFAULT_SETTLE_SECONDS, OLD_DEFAULT_SETTLE_SECONDS_2] {
            if migrate_default(settings, "settle_seconds", old, DEFAULT_SETTLE_SECONDS) {
                log::info!(
                    "settle_seconds was the old default {old}; now {DEFAULT_SETTLE_SECONDS}"
                );
            }
        }
        if migrate_default(
            settings,
            "restore_window_minutes",
            OLD_DEFAULT_RESTORE_MINUTES,
            DEFAULT_RESTORE_MINUTES,
        ) {
            log::info!(
                "restore_window_minutes was the old default {OLD_DEFAULT_RESTORE_MINUTES}; now {DEFAULT_RESTORE_MINUTES}"
            );
        }
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.apply_settings(settings);
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.apply_settings(ctx.settings);
        self.ticks = 0;
        self.reader = match desktops::Reader::new() {
            Ok(reader) => Some(reader),
            Err(err) => {
                log::warn!("{err}; desktops will not be recorded");
                None
            }
        };
        let file = start_layout(
            state::load(&self.state_path),
            state::load(&state::snapshot_path(&self.state_path)),
        );
        if let Some(file) = &file {
            let count: usize = file
                .programs
                .values()
                .map(|program| program.windows.len())
                .sum();
            match clock::parse_iso(&file.saved) {
                Some(secs) => log::info!(
                    "the saved layout has {count} windows, saved {} ({})",
                    clock::local_iso(UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64)),
                    file.reason
                ),
                None => log::info!(
                    "the saved layout has {count} windows; its save time \"{}\" cannot be read",
                    file.saved
                ),
            }
        }
        self.writer = state::Writer::new(self.state_path.clone(), file.map(|file| file.programs));
        self.groups = saved_models(self.writer.last());
        publish_census(&self.groups);
        if unsafe { SetTimer(ctx.hwnd, TIMER_ID, TICK_MS, None) } == 0 {
            return Err("could not start its timer".to_string());
        }
        self.host_hwnd = Some(ctx.hwnd);
        log::info!("watching {}", self.watch);

        // Switching the plugin off and on again is not a reboot.
        if !self.started_once && self.restore_on_start && start_follows_sign_in() {
            self.start_restore(true);
        }
        self.started_once = true;
        if host::demo_restore_panel() {
            self.show_demo_panel();
        }
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![
            HotkeyAction {
                id: ACTION_RESTORE,
                name: "restore_now",
                label: "Restore layout",
                default: win_alt(u32::from(b'L')),
            },
            HotkeyAction {
                id: ACTION_SAVE_NOW,
                name: "save_now",
                label: "Save layout now",
                default: win_alt(u32::from(b'J')),
            },
            HotkeyAction {
                id: ACTION_MOVE_LEFT,
                name: "move_left",
                label: "Move window left in taskbar",
                default: win_alt(VK_OPEN_BRACKET),
            },
            HotkeyAction {
                id: ACTION_MOVE_RIGHT,
                name: "move_right",
                label: "Move window right in taskbar",
                default: win_alt(VK_CLOSE_BRACKET),
            },
        ]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        match action_id {
            ACTION_RESTORE => self.start_restore(false),
            ACTION_SAVE_NOW => self.save_now(),
            ACTION_MOVE_LEFT => self.shift_front(-1),
            ACTION_MOVE_RIGHT => self.shift_front(1),
            _ => {}
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: ACTION_RESTORE,
            label: "Restore layout",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == ACTION_RESTORE {
            self.start_restore(false);
        }
    }

    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        let restore =
            matches!(kind, ActionKind::Hotkey | ActionKind::Tray) && action_id == ACTION_RESTORE;
        if !restore {
            return None;
        }
        Some(match &self.last_saved {
            Some(time) => format!("Last saved {time}"),
            None => "Not saved yet this session".to_string(),
        })
    }

    fn window_groups(&mut self) -> Option<WindowGroups> {
        let live = self.refresh();
        let registry = desktops::list();
        let current = desktops::current();
        let monitors = monitors::list();
        let connected = monitors::saved_names(&monitors);
        let mut disconnected: Vec<state::SavedMonitor> = Vec::new();
        let mut saved_monitor: BTreeMap<isize, usize> = BTreeMap::new();
        for (exe, program) in self.writer.last().into_iter().flatten() {
            let open = Self::open_matches(exe, program, &live);
            for (window, hwnd) in program.windows.iter().zip(open) {
                let (Some(saved), Some(hwnd)) = (&window.monitor, hwnd) else {
                    continue;
                };
                let place = match monitors::find(saved, &connected) {
                    Some(index) => index,
                    None => {
                        let known = disconnected
                            .iter()
                            .position(|m| m.device == saved.device && m.name == saved.name);
                        connected.len()
                            + known.unwrap_or_else(|| {
                                disconnected.push(saved.clone());
                                disconnected.len() - 1
                            })
                    }
                };
                saved_monitor.insert(hwnd, place);
            }
        }
        let reader = self.reader.as_ref();
        let desktop_of = |hwnd: Handle| strip_groups::desktop_of(reader, hwnd);
        let place = strip_groups::place_on(&monitors);
        let saved = |hwnd: Handle| saved_monitor.get(&hwnd).copied();
        let groups: Vec<ArrangeGroup> = self
            .groups
            .iter()
            .filter_map(|(key, model)| {
                strip_groups::group(
                    key,
                    self.label_of(key),
                    model.windows(),
                    &desktop_of,
                    &place,
                    &saved,
                )
            })
            .collect();

        // The strip opens on the front window's taskbar group when it is
        // watched, else on the busiest group.
        let front = unsafe { GetForegroundWindow() };
        let (_, front_key) = windows::describe(front);
        Some(strip_groups::window_groups(
            groups,
            &front_key,
            strip_groups::desktops(&registry, current),
            self.watch.to_string(),
            monitors::for_strip(&monitors, &disconnected),
        ))
    }

    fn on_arrange_action(&mut self, action: &ArrangeAction) {
        let what = match action {
            ArrangeAction::Activate(_) => "switching to a window",
            ArrangeAction::Reorder { .. } => "reordering from the strip",
            ArrangeAction::MoveToDesktop { .. } => "moving a window to another desktop",
            ArrangeAction::MoveToMonitor { .. } => "moving a window to another monitor",
            ArrangeAction::Close(_) => "closing a window",
            // The host sends names to WindowNamer and new windows to the
            // strip's own plugin; they never come here.
            ArrangeAction::Rename { .. }
            | ArrangeAction::AskName(_)
            | ArrangeAction::NewWindow { .. } => return,
        };
        if self.refuses(what) {
            return;
        }
        match action {
            ArrangeAction::Activate(hwnd) => window_actions::activate(*hwnd as HWND),
            ArrangeAction::Reorder { group, order } => {
                self.refresh();
                let Some(model) = self.groups.get_mut(group) else {
                    return;
                };
                model.set_order(order);
                self.apply_group(group, false);
                self.save("manual");
            }
            ArrangeAction::MoveToDesktop { hwnd, desktop } => {
                let Some(target) = DesktopId::parse(desktop) else {
                    return;
                };
                let registry = desktops::list();
                if !self.ensure_mover(&registry) {
                    let reason = self.mover.why_not();
                    host::notify(
                        "LayoutKeeper",
                        &format!("Windows cannot be moved to another desktop: {reason}"),
                    );
                    return;
                }
                let label = window_names::display_title(*hwnd as HWND);
                self.move_to_desktop(*hwnd as HWND, target, &registry, &label);
                let owner = self
                    .groups
                    .iter()
                    .find(|(_, group)| group.position_of(*hwnd).is_some())
                    .map(|(exe, _)| exe.clone());
                if let Some(exe) = owner {
                    if !self.reapply.contains(&exe) {
                        self.reapply.push(exe);
                    }
                }
                self.arrange_stale = true;
            }
            ArrangeAction::MoveToMonitor { hwnd, monitor } => {
                self.move_to_monitor(*hwnd as HWND, monitor);
                self.released.insert(*hwnd as Handle);
                self.save("manual");
                self.arrange_stale = true;
            }
            ArrangeAction::Close(hwnd) => {
                window_actions::close(*hwnd as HWND);
                self.arrange_stale = true;
            }
            ArrangeAction::Rename { .. }
            | ArrangeAction::AskName(_)
            | ArrangeAction::NewWindow { .. } => {}
        }
    }

    fn layout_snapshot(&mut self) -> Option<LayoutSnapshot> {
        let saved = self.writer.last().cloned().unwrap_or_default();
        let live = self.live_windows();
        let registry = desktops::list();
        let current = desktops::current();
        let monitors = monitors::list();

        let mut apps: BTreeMap<String, LayoutApp> = BTreeMap::new();
        for (exe, program) in &saved {
            if !self.watch.covers(exe) {
                continue;
            }
            let open = Self::open_matches(exe, program, &live);
            for (index, window) in program.windows.iter().enumerate() {
                let key = if window.group.is_empty() {
                    exe.clone()
                } else {
                    window.group.clone()
                };
                if !apps.contains_key(&key) {
                    let label = self.saved_label(exe, &window.group);
                    apps.insert(
                        key.clone(),
                        LayoutApp {
                            key: key.clone(),
                            label,
                            windows: Vec::new(),
                        },
                    );
                }
                let hwnd = open[index];
                let title = match hwnd {
                    Some(hwnd) => window_names::display_title(hwnd as HWND),
                    None => window.label().to_string(),
                };
                let desktop = window.desktop.clone().unwrap_or_default();
                let desktop_removed = DesktopId::parse(&desktop)
                    .is_some_and(|id| id != DesktopId::ALL && !registry.iter().any(|d| d.id == id));
                if let Some(app) = apps.get_mut(&key) {
                    app.windows.push(LayoutEntry {
                        entry: LayoutRef {
                            exe: exe.clone(),
                            index,
                            title: window.title.clone(),
                        },
                        title,
                        desktop,
                        desktop_removed,
                        monitor: monitors::strip_label(window.rect, &monitors),
                        hwnd,
                        closed_since: if hwnd.is_none() {
                            window.seen.clone()
                        } else {
                            None
                        },
                        same_title: false,
                    });
                }
            }
        }
        let mut apps: Vec<LayoutApp> = apps.into_values().collect();
        for app in &mut apps {
            let titles: Vec<&str> = app.windows.iter().map(|w| w.title.as_str()).collect();
            let same = identity::same_titles(&titles);
            for (window, same) in app.windows.iter_mut().zip(same) {
                window.same_title = same;
            }
        }
        apps.sort_by(|a, b| {
            b.windows
                .len()
                .cmp(&a.windows.len())
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });
        Some(LayoutSnapshot {
            plugin: "layout_keeper".to_string(),
            last_saved: self.last_saved.clone(),
            desktops: registry
                .iter()
                .map(|desktop| ArrangeDesktop {
                    id: desktop.id.to_string(),
                    name: desktop.name.clone(),
                    current: Some(desktop.id) == current,
                })
                .collect(),
            apps,
            can_rename: false,
        })
    }

    fn on_layout_action(&mut self, action: &LayoutAction) {
        match action {
            LayoutAction::RestoreNow => self.start_restore(false),
            LayoutAction::SaveNow => self.save_now(),
            LayoutAction::SetDesktop {
                entry,
                desktop,
                hwnd,
            } => {
                let Some(target) = DesktopId::parse(desktop) else {
                    return;
                };
                let registry = desktops::list();
                let name = desktops::name_of(&registry, target);
                let label = entry.title.clone();
                self.edit_saved(
                    entry,
                    &format!("saved \"{label}\" for desktop {desktop}"),
                    |programs, entry| {
                        if let Some(window) = programs
                            .get_mut(&entry.exe)
                            .and_then(|program| program.windows.get_mut(entry.index))
                        {
                            window.desktop = Some(target.to_string());
                            window.desktop_name = name;
                        }
                    },
                );
                // An open window goes there now, the way the strip moves it.
                let open = hwnd.filter(|hwnd| {
                    self.live_windows()
                        .iter()
                        .any(|window| window.hwnd as isize == *hwnd)
                });
                if let Some(hwnd) = open {
                    self.on_arrange_action(&ArrangeAction::MoveToDesktop {
                        hwnd,
                        desktop: target.to_string(),
                    });
                }
            }
            LayoutAction::Forget(entry) => {
                let live = self.live_windows();
                let open = self
                    .writer
                    .last()
                    .and_then(|programs| programs.get(&entry.exe))
                    .map(|program| Self::open_matches(&entry.exe, program, &live))
                    .and_then(|open| open.get(entry.index).copied().flatten());
                if open.is_some() {
                    log::info!(
                        "\"{}\" is open, so it is saved again at once; not forgotten",
                        entry.title
                    );
                    return;
                }
                self.edit_saved(
                    entry,
                    &format!("forgot the saved window \"{}\"", entry.title),
                    |programs, entry| {
                        if let Some(program) = programs.get_mut(&entry.exe) {
                            program.windows.remove(entry.index);
                            if program.windows.is_empty() {
                                programs.remove(&entry.exe);
                            }
                        }
                    },
                );
            }
        }
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: COMMAND_OPEN_STATE,
            label: "Open state file",
            hint: "",
            words: "",
            confirm: "",
            list: false,
            toggle: None,
        }]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id == COMMAND_OPEN_STATE {
            self.open_state_file();
        }
    }

    fn status(&self) -> Option<String> {
        let saved = match &self.last_saved {
            Some(time) => format!("Layout saved at {time}"),
            None => "Layout not saved yet this session".to_string(),
        };
        let moves = match &self.mover {
            Mover::Untried => "Desktop moves: checked at the first restore".to_string(),
            Mover::Ready(_) => "Desktop moves: available".to_string(),
            Mover::Disabled(reason) => format!("Desktop moves: disabled ({reason})"),
        };
        let mut parts = vec![saved];
        if self.read_only {
            parts.push(format!(
                "Read-only test instance: {} changes to real windows skipped",
                self.refused
            ));
        }
        if self.restore.waiting_count() > 0 {
            parts.push(format!(
                "Restore waiting for {} apps to open",
                self.restore.waiting_count()
            ));
        }
        if let Some(restore) = &self.last_restore {
            parts.push(restore.clone());
        }
        parts.push(moves);
        Some(parts.join(" \u{b7} "))
    }

    fn on_taskbar_created(&mut self) {
        if self.refuses("putting the taskbar order back after Explorer restarted") {
            return;
        }
        self.explorer.restarted(self.ticks);
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        // An installer or update closing apps is not the end of the session:
        // Windows keeps running, and the next start is no sign-in.
        let closes_app = (lparam as u32) & ENDSESSION_CLOSEAPP != 0;
        match msg {
            WM_TIMER if wparam == TIMER_ID => self.tick(),
            WM_TIMER if wparam == FADE_TIMER_ID => self.fade_step(),
            host::PLUGIN_POST if wparam == TIMER_READ_ID => self.timer_read_arrived(),
            WM_QUERYENDSESSION | WM_ENDSESSION if closes_app => self.save("shutdown"),
            WM_QUERYENDSESSION => self.save_at_end("shutdown"),
            WM_ENDSESSION if wparam != 0 => self.save_at_end("shutdown"),
            _ => {}
        }
    }

    fn teardown(&mut self) {
        // Windows still being read for a timer save are never saved now.
        self.timer_read = None;
        if let Some(hwnd) = self.host_hwnd.take() {
            unsafe { KillTimer(hwnd, TIMER_ID) };
            self.save_at_end("teardown");
        }
        self.restore = Restore::new(self.restore.settle_seconds, self.restore.give_up_seconds);
        self.hide_notice();
        if let Some(mut window) = self.notice.take() {
            window.destroy();
        }
        if self.tray_tip.take().is_some() {
            host::set_tray_tip(None);
        }
        self.finishing = None;
        self.checks.clear();
        self.restored_stack.clear();
        self.reader = None;
        self.taskbar = None;
        self.mover = Mover::Untried;
        self.guard = guard::Guard::default();
        self.groups.clear();
        window_census::clear();
        self.labels.clear();
    }
}
