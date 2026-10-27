use super::*;

impl LayoutKeeper {
    /// Makes one taskbar group match its model, if it does not already. Only
    /// that group's buttons are rebuilt. Re-adding a button pulls a window on
    /// another desktop over to this one, so those windows are only included
    /// when `pull` is set and the caller will move them back.
    pub(super) fn apply_group(&mut self, key: &str, pull: bool) {
        if self.refuses("taskbar reorder") {
            return;
        }
        let label = self.label_of(key);
        let Some(pending) = self.groups.get(key).and_then(OrderModel::pending) else {
            return;
        };
        let send: Vec<Handle> = pending
            .iter()
            .copied()
            .filter(|hwnd| pull || !windows::on_other_desktop(*hwnd as HWND))
            .collect();
        if send.is_empty() {
            if let Some(group) = self.groups.get_mut(key) {
                group.mark_applied(pending);
            }
            return;
        }
        if log::log_enabled!(log::Level::Debug) {
            let labels: Vec<&str> = self
                .groups
                .get(key)
                .map(|group| {
                    group
                        .windows()
                        .filter(|(hwnd, _)| send.contains(hwnd))
                        .map(|(_, identity)| identity.label())
                        .collect()
                })
                .unwrap_or_default();
            log::debug!(
                "re-adding {label} buttons in this order: {}",
                labels.join(" | ")
            );
        }
        let Some(took) = order::apply_logged(&mut self.taskbar, &send) else {
            return;
        };
        log::info!(
            "applied the order of {} {label} windows in {} ms ({} on other desktops left alone)",
            send.len(),
            took.as_millis(),
            pending.len() - send.len()
        );
        if let Some(group) = self.groups.get_mut(key) {
            group.mark_applied(pending);
        }
    }

    /// Explorer rebuilt the taskbar, so each group's buttons are in its order,
    /// not ours. Once the taskbar is back, sends each group's order again:
    /// only the windows on this desktop, and nothing else is restored. A
    /// group the crash guard holds is left alone, because its windows were
    /// just replaced and its order is not to be trusted.
    pub(super) fn put_back_after_explorer(&mut self) {
        let live = self.refresh();
        let taskbar = unsafe { FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null()) };
        match self
            .explorer
            .step(self.ticks, !taskbar.is_null(), live.len())
        {
            explorer::Step::Wait => {}
            explorer::Step::GiveUp => {
                log::warn!("Explorer restarted, but the taskbar did not come back in time");
            }
            explorer::Step::Apply => {
                if self.restore.is_running() || self.finishing.is_some() {
                    log::info!(
                        "Explorer restarted while a restore is under way; the restore puts the taskbar order back"
                    );
                    return;
                }
                let held = self.guard.held();
                let keys: Vec<String> = self
                    .groups
                    .iter()
                    .filter(|(key, group)| group.handles().len() > 1 && !held.contains(*key))
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in &keys {
                    if let Some(group) = self.groups.get_mut(key) {
                        group.invalidate();
                    }
                    self.apply_group(key, false);
                }
                if !keys.is_empty() {
                    log::info!(
                        "Explorer restarted: put the taskbar order of {} apps back",
                        keys.len()
                    );
                }
            }
        }
    }

    /// Moves the front window one place along its taskbar group.
    pub(super) fn shift_front(&mut self, step: isize) {
        if self.refuses("moving the front window in its group") {
            return;
        }
        let front = unsafe { GetForegroundWindow() };
        let (exe, key) = windows::describe(front);
        if !self.watch.covers(&exe) {
            log::info!("the front window belongs to \"{exe}\", which is not watched");
            host::notify(
                "LayoutKeeper",
                &format!("Moving in the taskbar works on windows of {}", self.watch),
            );
            return;
        }
        let live = self.refresh();
        let visible: Vec<Handle> = live
            .iter()
            .filter(|window| window.group == key && !windows::on_other_desktop(window.hwnd))
            .map(|window| window.hwnd as Handle)
            .collect();
        let moved = self
            .groups
            .get_mut(&key)
            .is_some_and(|group| group.shift(front as Handle, step, &visible));
        if !moved {
            log::info!("the front window is already at that end of its group");
            host::notify(
                "LayoutKeeper",
                if step < 0 {
                    "This window is already first in its taskbar group"
                } else {
                    "This window is already last in its taskbar group"
                },
            );
            return;
        }
        self.apply_group(&key, false);
        self.save("manual");
    }

    pub(super) fn restore_finishing(&self) -> bool {
        self.finishing.is_some()
    }

    /// The groups a restore waits for: every watched group with saved
    /// windows. Windows saved before groups were recorded have none and are
    /// left to the groups of the windows they match.
    pub(super) fn saved_groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = Vec::new();
        for (exe, program) in self.writer.last().into_iter().flatten() {
            if !self.watch.covers(exe) {
                continue;
            }
            for window in &program.windows {
                if !window.group.is_empty() && !groups.contains(&window.group) {
                    groups.push(window.group.clone());
                }
            }
        }
        groups
    }

    /// How many windows are saved for each taskbar group.
    pub(super) fn saved_counts(&self) -> BTreeMap<String, usize> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for (exe, program) in self.writer.last().into_iter().flatten() {
            if !self.watch.covers(exe) {
                continue;
            }
            for window in program.windows.iter().filter(|w| !w.group.is_empty()) {
                *counts.entry(window.group.clone()).or_default() += 1;
            }
        }
        counts
    }
}
