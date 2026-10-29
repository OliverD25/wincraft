use super::*;

impl LayoutKeeper {
    pub(super) fn tick(&mut self) {
        self.ticks += 1;
        if !self.reapply.is_empty() {
            self.refresh();
            for exe in std::mem::take(&mut self.reapply) {
                if let Some(group) = self.groups.get_mut(&exe) {
                    group.invalidate();
                }
                self.apply_group(&exe, false);
            }
        }
        if std::mem::take(&mut self.arrange_stale) {
            host::refresh_arrange();
        }
        if self.explorer.due(self.ticks) {
            self.put_back_after_explorer();
        }
        self.run_checks();
        if let Some(finish) = self.finishing.take() {
            self.finish_restore(finish);
            return;
        }
        if self.restore.is_running() || self.notice_ctl.is_visible() || self.tray_tip.is_some() {
            let live = self.live_windows();
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for window in &live {
                self.remember_label(window);
                *counts.entry(window.group.clone()).or_default() += 1;
            }
            if self.restore.is_running() {
                self.place_new(&live);
            }
            let deciding = self.restore.deciding(&counts);
            let busy: BTreeSet<String> = deciding
                .into_iter()
                .filter(|group| {
                    live.iter()
                        .any(|window| window.group == *group && !answers(window.hwnd))
                })
                .collect();
            let step = self.restore.step_busy(self.ticks, &counts, &busy);
            for due in &step.due {
                if due.pass == restore::Pass::Limit {
                    log::info!(
                        "\"{}\" never held still for {} s; restoring what is open {} minutes after its first window",
                        self.label_of(&due.group),
                        self.restore.settle_seconds,
                        restore::GROUP_LIMIT_SECONDS / 60
                    );
                }
            }
            if !step.due.is_empty() {
                self.restore_groups(step.due);
            }
            if let Some(timed_out) = step.timed_out {
                let names = |groups: &[String]| {
                    groups
                        .iter()
                        .map(|group| self.label_of(group))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                log::info!(
                    "stopped waiting after {} minutes; never opened a window: {}",
                    self.restore.give_up_seconds / 60,
                    names(&timed_out.never_opened)
                );
                if !timed_out.by_limit.is_empty() {
                    log::info!(
                        "opened windows that kept changing, so restored after {} minutes anyway: {}",
                        restore::GROUP_LIMIT_SECONDS / 60,
                        names(&timed_out.by_limit)
                    );
                }
                if let Some(summary) = nothing_restored(&self.restore) {
                    host::notify("LayoutKeeper", &summary);
                    self.last_restore = Some(summary);
                }
            }
            self.update_progress(&counts);
        }
        // A read whose message the host has not delivered yet.
        self.timer_read_arrived();
        if save_due(self.ticks, self.snapshot_seconds) {
            self.timer_save();
        }
    }

    /// For `--show-restore-panel` in a test instance: the panel with sample
    /// progress, so it can be looked at without restoring anything.
    pub(super) fn show_demo_panel(&mut self) {
        let progress = notice::sample();
        if let Some(content) = notice::active_content(&progress, &notice::same_label) {
            self.show_notice(&content);
        }
        let tip = notice::tooltip(&progress, &notice::same_label);
        host::set_tray_tip(tip.clone());
        self.tray_tip = tip;
    }

    /// Brings the please-wait notice and the tray tooltip up to date.
    fn update_progress(&mut self, counts: &BTreeMap<String, usize>) {
        let progress = self.restore.progress(self.ticks, counts, &self.expected);
        let names: BTreeMap<String, String> = progress
            .iter()
            .map(|entry| (entry.group.clone(), self.group_name(&entry.group)))
            .collect();
        let (tip, active, last) = {
            let label = |group: &str| {
                names
                    .get(group)
                    .cloned()
                    .unwrap_or_else(|| friendly_key(group))
            };
            // Once the run is over the tooltip goes back to the plain name,
            // however many apps never opened.
            let showing =
                self.show_progress && (self.restore.is_running() || self.notice_ctl.is_visible());
            (
                showing
                    .then(|| notice::tooltip(&progress, &label))
                    .flatten(),
                notice::panel_open(self.ticks, self.last_placed, self.panel_closed)
                    .then(|| notice::active_content(&progress, &label))
                    .flatten(),
                notice::final_line(&progress, &label),
            )
        };
        if tip != self.tray_tip {
            host::set_tray_tip(tip.clone());
            self.tray_tip = tip;
        }
        if !self.show_progress {
            return;
        }
        let active = active.map(|mut content| {
            content.bar = notice::steady(content.bar, &mut self.bar_floor);
            content
        });
        let was_visible = self.notice_ctl.is_visible();
        match self.notice_ctl.update(self.ticks, active, last) {
            notice::Command::Nothing => {}
            notice::Command::Show(content) => self.show_notice(&content),
            notice::Command::Fade => self.start_fade(),
        }
        if was_visible && !self.notice_ctl.is_visible() {
            self.panel_closed = true;
        }
    }

    fn show_notice(&mut self, content: &notice::Content) {
        if let Some(hwnd) = self.host_hwnd {
            self.fading_since = None;
            unsafe { KillTimer(hwnd, FADE_TIMER_ID) };
        }
        if self.notice.is_none() {
            match notice::Notice::create() {
                Ok(window) => self.notice = Some(window),
                Err(err) => {
                    log::warn!("{err}");
                    return;
                }
            }
        }
        if let Some(window) = self.notice.as_mut() {
            window.show(content);
        }
    }

    fn start_fade(&mut self) {
        let Some(hwnd) = self.host_hwnd else {
            return;
        };
        if self.notice.is_none() {
            return;
        }
        unsafe { SetTimer(hwnd, FADE_TIMER_ID, FADE_STEP_MS, None) };
        self.fading_since = Some(unsafe { GetTickCount64() });
    }

    pub(super) fn fade_step(&mut self) {
        let (Some(since), Some(hwnd)) = (self.fading_since, self.host_hwnd) else {
            return;
        };
        let elapsed = unsafe { GetTickCount64() }.saturating_sub(since);
        let Some(window) = self.notice.as_mut() else {
            return;
        };
        if elapsed >= FADE_MS {
            window.hide();
            self.fading_since = None;
            unsafe { KillTimer(hwnd, FADE_TIMER_ID) };
            return;
        }
        window.set_alpha((255 * (FADE_MS - elapsed) / FADE_MS) as u8);
    }

    pub(super) fn hide_notice(&mut self) {
        if let Some(hwnd) = self.host_hwnd {
            unsafe { KillTimer(hwnd, FADE_TIMER_ID) };
        }
        self.fading_since = None;
        self.notice_ctl.reset();
        if let Some(window) = self.notice.as_mut() {
            window.hide();
        }
    }

    pub(super) fn open_state_file(&mut self) {
        if !self.state_path.exists() {
            self.save("manual");
        }
        let args = wide(&format!("\"{}\"", self.state_path.display()));
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide("notepad.exe").as_ptr(),
                args.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
    }
}
