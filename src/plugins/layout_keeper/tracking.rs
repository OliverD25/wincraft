use super::*;

impl LayoutKeeper {
    /// Brings every group's order model up to date with these windows.
    pub(super) fn track(&mut self, live: &[windows::LiveWindow]) {
        let mut keys: Vec<String> = Vec::new();
        for window in live {
            if !keys.contains(&window.group) {
                keys.push(window.group.clone());
            }
            self.remember_label(window);
        }
        let known: Vec<String> = self.groups.keys().cloned().collect();
        for key in known {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in keys {
            let mine = handles_in(live, &key);
            self.groups
                .entry(key)
                .or_default()
                .refresh(&mine, self.ticks, self.snapshot_seconds);
        }
        self.groups.retain(|_, group| !group.is_empty());
        publish_census(&self.groups);
    }

    /// Gives a group its name the first time one of its windows is seen.
    pub(super) fn remember_label(&mut self, window: &windows::LiveWindow) {
        if self.labels.contains_key(&window.group) && !self.unresolved.contains(&window.group) {
            return;
        }
        let (label, resolved) = app_names::window_label(window);
        if resolved {
            self.unresolved.remove(&window.group);
        } else {
            self.unresolved.insert(window.group.clone());
        }
        log::debug!("new taskbar group {} labelled \"{label}\"", window.group);
        self.labels.insert(window.group.clone(), label);
    }

    /// A name a person knows for a group, for words on screen: the label of
    /// a window seen, else one made from the program the saved layout names
    /// for it. Never a path or an app ID.
    pub(super) fn group_name(&mut self, group: &str) -> String {
        if let Some(label) = self.labels.get(group) {
            return label.clone();
        }
        let exe = self
            .writer
            .last()
            .into_iter()
            .flatten()
            .find_map(|(exe, program)| {
                program
                    .windows
                    .iter()
                    .any(|window| window.group == group)
                    .then(|| exe.clone())
            });
        match exe {
            Some(exe) => self.saved_label(&exe, group),
            None => friendly_key(group),
        }
    }

    pub(super) fn label_of(&self, key: &str) -> String {
        self.labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// What the Layout page calls a saved group. A group seen live this
    /// session has its label already; the others are named from the Start
    /// menu entry of their app ID, else from the program.
    pub(super) fn saved_label(&mut self, exe: &str, group: &str) -> String {
        if let Some(label) = self.labels.get(group) {
            return label.clone();
        }
        let has_id = !group.contains('\\');
        let name = has_id.then(|| appid::registered_name(group)).flatten();
        let mut resolved = true;
        let label = match browser_name(exe) {
            Some(browser) => appid::group_label(browser, name.as_deref(), group, ""),
            None => {
                let (name, done) = with_start_menu_name(name, group);
                resolved = done;
                app_label(name.as_deref(), None, exe)
            }
        };
        // Not remembered while the apps index is still being built: the next
        // ask may find it ready.
        if resolved {
            self.labels.insert(group.to_string(), label.clone());
        }
        label
    }

    /// For each saved window of `exe`, in the saved order, the handle of the
    /// open window it matches.
    pub(super) fn open_matches(
        exe: &str,
        program: &ProgramState,
        live: &[windows::LiveWindow],
    ) -> Vec<Option<isize>> {
        let saved_ids: Vec<identity::WindowIdentity> = program
            .windows
            .iter()
            .map(|window| window.identity(exe))
            .collect();
        let mine: Vec<&windows::LiveWindow> = live
            .iter()
            .filter(|window| window.identity.exe == exe)
            .collect();
        let live_ids: Vec<identity::WindowIdentity> =
            mine.iter().map(|window| window.identity.clone()).collect();
        let matching = identity::match_windows(&saved_ids, &live_ids);
        (0..program.windows.len())
            .map(|index| {
                matching
                    .pairs
                    .iter()
                    .find(|(saved, _)| *saved == index)
                    .map(|(_, l)| mine[*l].hwnd as isize)
            })
            .collect()
    }

    /// Changes one saved window and writes the file at once. `change` gets
    /// the window if `entry` still names it (same place, same title).
    pub(super) fn edit_saved(
        &mut self,
        entry: &LayoutRef,
        what: &str,
        change: impl FnOnce(&mut Programs, &LayoutRef),
    ) {
        let Some(mut programs) = self.writer.last().cloned() else {
            return;
        };
        let current = programs
            .get(&entry.exe)
            .and_then(|program| program.windows.get(entry.index))
            .is_some_and(|window| window.title == entry.title);
        if !current {
            log::info!("the saved list changed under the Layout page; {what} skipped");
            return;
        }
        change(&mut programs, entry);
        match self.writer.write(
            programs,
            "layout page",
            clock::local_iso(SystemTime::now()),
            true,
        ) {
            Ok(_) => log::info!("{what}"),
            Err(err) => log::error!("could not save the layout: {err}"),
        }
    }

    /// Tells the guard what each taskbar group holds now and logs any hold
    /// that starts or ends.
    pub(super) fn watch_bursts(&mut self, live: &[windows::LiveWindow]) {
        let mut groups: BTreeMap<String, Vec<Handle>> = BTreeMap::new();
        for window in live {
            groups
                .entry(window.group.clone())
                .or_default()
                .push(window.hwnd as Handle);
        }
        let timing = guard::Timing {
            stable_seconds: GUARD_STABLE_SECONDS,
            refill_seconds: GUARD_REFILL_SECONDS,
        };
        for event in self.guard.observe(self.ticks, &groups, timing) {
            match event {
                guard::Event::Held { group, why } => {
                    let what = match why {
                        guard::Why::Dropped { before, now } => {
                            format!("its windows went from {before} to {now}")
                        }
                        guard::Why::Replaced { replaced, before } => {
                            format!("{replaced} of its {before} windows were replaced")
                        }
                    };
                    log::info!(
                        "holding timer saves for \"{}\": {what}, which looks like a crash or restart",
                        self.label_of(&group)
                    );
                }
                guard::Event::Refilled { group, now } => log::info!(
                    "\"{}\" is back with {now} windows; saves stay held until the count is steady for {} s",
                    self.label_of(&group),
                    timing.stable_seconds
                ),
                guard::Event::Released {
                    group,
                    stable_seconds,
                } => log::info!(
                    "\"{}\" has been steady for {stable_seconds} s; timer saves resume",
                    self.label_of(&group)
                ),
                guard::Event::Closed { group } => log::info!(
                    "\"{}\" did not come back within {} s, so its windows were closed on purpose; timer saves resume",
                    self.label_of(&group),
                    timing.refill_seconds
                ),
            }
        }
    }

    /// Logs which windows of one app share a title, only when that set
    /// changes: a restore cannot tell such windows apart.
    pub(super) fn note_shared_titles(&mut self, live: &[windows::LiveWindow]) {
        let mut by_group: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
        for window in live {
            let title = window_names::custom(window.hwnd as isize)
                .unwrap_or_else(|| window.identity.label().to_string());
            by_group
                .entry(window.group.as_str())
                .or_default()
                .push((identity::title_key(&title), title));
        }
        let mut shared: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut shown: BTreeMap<(String, String), String> = BTreeMap::new();
        for (group, titles) in &by_group {
            let names: Vec<&str> = titles.iter().map(|(_, title)| title.as_str()).collect();
            for ((key, title), same) in titles.iter().zip(identity::same_titles(&names)) {
                if same {
                    let id = (group.to_string(), key.clone());
                    *shared.entry(id.clone()).or_default() += 1;
                    shown.entry(id).or_insert_with(|| title.clone());
                }
            }
        }
        if shared == self.shared_titles {
            return;
        }
        if shared.is_empty() {
            log::info!("no windows of one app share a title any more");
        } else {
            let list: Vec<String> = shared
                .iter()
                .map(|(id, count)| format!("{} x{count} in {}", shown[id], self.label_of(&id.0)))
                .collect();
            log::info!(
                "windows of one app share a title, so a restore cannot tell them apart; give one a name: {}",
                list.join(" | ")
            );
        }
        self.shared_titles = shared;
    }
}
