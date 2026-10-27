use super::*;

impl LayoutKeeper {
    /// Starts a restore run. `sign_in`: the run that follows signing in,
    /// whose groups get their taskbar order a second time.
    pub(super) fn start_restore(&mut self, sign_in: bool) {
        if self.refuses("restore") {
            return;
        }
        let groups = self.saved_groups();
        if groups.is_empty() {
            log::info!("no layout has been saved yet, so there is nothing to restore");
            host::notify(
                "LayoutKeeper",
                "Nothing to restore: no layout has been saved yet",
            );
            return;
        }
        log::info!(
            "restore watches {} saved taskbar groups for {} minutes; each window is placed as it opens, and each group gets its taskbar order once all its saved windows are open or none has come for {} s (at the latest {} minutes after its first window), and again for windows that come in the next {} minutes",
            groups.len(),
            self.restore.give_up_seconds / 60,
            self.restore.settle_seconds,
            restore::GROUP_LIMIT_SECONDS / 60,
            restore::SECOND_PASS_SECONDS / 60
        );
        self.restored_stack.clear();
        self.placed.clear();
        self.announced = false;
        self.expected = self.saved_counts();
        self.last_placed = None;
        self.bar_floor = 0;
        self.panel_closed = false;
        self.restore.start(self.ticks, groups);
        self.restore.expect(self.expected.clone());
        if sign_in {
            self.restore.after_sign_in();
        }
    }

    /// First half of restoring the groups in `due`: their taskbar order.
    /// Re-adding the buttons pulls every window onto the current desktop, a
    /// moment later and not at once, so the desktop moves and the
    /// front-to-back order wait for the next tick in `finish_restore`.
    /// Without the desktop mover only the windows already on this desktop
    /// are re-added. The current desktop and the foreground window are left
    /// as they are.
    ///
    /// A group in its second pass (`Pass::Late`) has windows that came after
    /// its first restore. Windows already placed by this run stay on the
    /// desktop they are on now, and nothing is raised, so a person who has
    /// started working is not disturbed.
    pub(super) fn restore_groups(&mut self, due: Vec<restore::Due>) {
        let Some(saved) = self.writer.last().cloned() else {
            return;
        };
        let live = self.refresh();
        let registry = desktops::list();
        let known: Vec<DesktopId> = registry.iter().map(|desktop| desktop.id).collect();
        let can_move = self.ensure_mover(&registry);
        let monitors = monitors::list();
        let connected = monitors::saved_names(&monitors);
        let foreground = unsafe { GetForegroundWindow() };
        let mut done = Restored::default();
        for exe in restore_exes(&saved, &due, &self.watch) {
            // Read before any button is re-added, which pulls windows here.
            let reader = self.reader.as_ref();
            let view = ExeView::new(
                &exe,
                &saved[&exe],
                &live,
                &|hwnd| reader.and_then(|reader| reader.read(hwnd)),
                &|rect| monitors::index_of(rect, &monitors),
            );
            let cx = RestoreContext {
                live: &live,
                known: &known,
                monitors: &connected,
                can_move,
                restore_monitors: self.restore_monitors,
                ticks: self.ticks,
                snapshot_seconds: self.snapshot_seconds,
            };
            let plans = plan_restore(
                &view,
                &due,
                &cx,
                &mut self.groups,
                &mut self.placed,
                &mut self.restored_stack,
            );
            self.carry_out(plans, can_move, &monitors, &mut done);
        }
        let Some(summary) = summary(&done) else {
            return;
        };
        self.last_placed = Some(self.ticks);
        let restored = !done.parts.is_empty();
        // With the notice on, its last line says it; the balloon would repeat it.
        let announce = restored && !self.announced && !self.show_progress;
        self.announced |= restored;
        self.finishing = Some(Finish {
            targets: done.targets,
            orders: done.orders,
            moves: done.moves,
            stack: if restored {
                self.restored_stack.clone()
            } else {
                Vec::new()
            },
            foreground,
            summary,
            announce,
        });
    }

    /// The first half of a restore for the groups of one program, as
    /// `plan_restore` decided it: monitors, then the taskbar order. The
    /// desktop moves are kept in `done` for the second half.
    pub(super) fn carry_out(
        &mut self,
        plans: Vec<GroupPlan>,
        can_move: bool,
        monitors: &[monitors::Monitor],
        done: &mut Restored,
    ) {
        for plan in plans {
            for step in &plan.monitors {
                self.return_to_monitor(step, monitors);
            }
            self.apply_group(&plan.group, can_move);
            done.targets.extend(check_targets(&plan, monitors));
            if let Some(model) = self.groups.get(&plan.group) {
                done.orders.push((plan.group.clone(), model.handles()));
            }
            for (label, gone) in &plan.gone {
                log::info!(
                    "\"{label}\" was saved on desktop {gone}, which no longer exists; it stays where it is"
                );
            }
            done.moves.extend(plan.moves);
            let label = self.label_of(&plan.group);
            if plan.again {
                log::info!(
                    "put the taskbar order of {label} back a second time, in case the first rebuild came too soon for some buttons"
                );
                continue;
            }
            if plan.late {
                let line = format!(
                    "second pass: moved {} late {label} windows",
                    plan.moved_late
                );
                log::info!("{line}");
                done.late_parts.push(line);
                continue;
            }
            log::info!(
                "restored {} of {} {label} windows",
                plan.matched,
                plan.saved
            );
            if !plan.lost.is_empty() {
                log::info!("not matched: {}", plan.lost.join(" | "));
            }
            done.missing += plan.lost.len();
            done.parts.push(format!(
                "{} of {} {label} windows",
                plan.matched, plan.saved
            ));
        }
    }

    /// Second half of a restore: desktops, then the front-to-back order.
    /// Right after the buttons were re-added Windows may still report a
    /// window's old desktop, so every window whose desktop is not the current
    /// one is moved without asking where it is.
    pub(super) fn finish_restore(&mut self, finish: Finish) {
        if self.refuses("finishing a restore") {
            return;
        }
        let registry = desktops::list();
        let (moves, raises) = finish_steps(&finish, desktops::current(), self.front_order);
        for (hwnd, target, label) in moves {
            self.move_to_desktop(*hwnd, *target, &registry, label);
        }
        for hwnd in raises {
            raise(hwnd);
        }
        if !finish.targets.is_empty() {
            self.checks.push(Check {
                due: self.ticks + CHECK_SECONDS,
                targets: finish.targets.clone(),
                orders: finish.orders.clone(),
                moved_again: None,
            });
        }
        log::info!("{}", finish.summary);
        if finish.announce {
            host::notify("LayoutKeeper", &finish.summary);
        }
        self.last_restore = Some(format!(
            "{} at {}",
            finish.summary,
            clock::now_hours_minutes()
        ));
    }

    /// Builds the desktop mover on first use; true when it is usable.
    pub(super) fn ensure_mover(&mut self, registry: &[Desktop]) -> bool {
        self.mover.ensure(registry)
    }

    pub(super) fn move_to_desktop(
        &mut self,
        hwnd: HWND,
        target: DesktopId,
        registry: &[Desktop],
        label: &str,
    ) {
        if self.refuses("desktop move") {
            return;
        }
        let Mover::Ready(mover) = &self.mover else {
            return;
        };
        window_actions::move_to_desktop(mover, self.reader.as_ref(), hwnd, target, registry, label);
    }

    /// Puts a restored window back on the monitor it was saved on.
    pub(super) fn return_to_monitor(&mut self, step: &MonitorStep, monitors: &[monitors::Monitor]) {
        let label = &step.label;
        match step.target {
            monitors::Return::Stay => {}
            monitors::Return::Unplugged => {
                if let Some(saved) = &step.saved {
                    log::info!(
                        "\"{label}\" was saved on monitor {} ({}), which is not connected; it stays where it is",
                        saved.device,
                        saved.name
                    );
                }
            }
            monitors::Return::To(target) => {
                let (Some(now), false) = (step.now, self.refuses("monitor move")) else {
                    return;
                };
                let device = &monitors[target].id().device;
                match monitors::move_window(step.hwnd, &monitors[now], &monitors[target], monitors)
                {
                    Ok(()) => log::info!("put \"{label}\" back on monitor {device}"),
                    Err(err) => {
                        log::warn!("could not put \"{label}\" back on monitor {device}: {err}")
                    }
                }
            }
        }
    }

    /// Moves a window to the monitor with that device name, from the one it
    /// is on now; nothing when it is there already.
    pub(super) fn move_to_monitor(&mut self, hwnd: HWND, device: &str) {
        let rect = self
            .refresh()
            .iter()
            .find(|window| window.hwnd == hwnd)
            .map(|window| window.identity.rect);
        window_actions::move_to_monitor(hwnd, device, rect);
    }

    /// Puts every window of a group still in this restore on its saved
    /// monitor and desktop the tick it appears, without waiting for the rest
    /// of its app. Its taskbar order follows when the group is restored.
    pub(super) fn place_new(&mut self, live: &[windows::LiveWindow]) {
        if self.read_only {
            return;
        }
        let pending = self.restore.pending();
        let fresh = |window: &windows::LiveWindow| {
            pending.contains(&window.group) && !self.placed.contains(&(window.hwnd as Handle))
        };
        if !live.iter().any(fresh) {
            return;
        }
        let Some(saved) = self.writer.last().cloned() else {
            return;
        };
        let mut exes: Vec<String> = Vec::new();
        for window in live.iter().filter(|window| fresh(window)) {
            let exe = &window.identity.exe;
            if saved.contains_key(exe) && self.watch.covers(exe) && !exes.contains(exe) {
                exes.push(exe.clone());
            }
        }
        if exes.is_empty() {
            return;
        }
        let registry = desktops::list();
        let known: Vec<DesktopId> = registry.iter().map(|desktop| desktop.id).collect();
        let can_move = self.ensure_mover(&registry);
        let monitors = monitors::list();
        let connected = monitors::saved_names(&monitors);
        let mut placed_in: BTreeMap<String, usize> = BTreeMap::new();
        for exe in exes {
            let reader = self.reader.as_ref();
            let view = ExeView::new(
                &exe,
                &saved[&exe],
                live,
                &|hwnd| reader.and_then(|reader| reader.read(hwnd)),
                &|rect| monitors::index_of(rect, &monitors),
            );
            let cx = RestoreContext {
                live,
                known: &known,
                monitors: &connected,
                can_move,
                restore_monitors: self.restore_monitors,
                ticks: self.ticks,
                snapshot_seconds: self.snapshot_seconds,
            };
            for placement in plan_placements(&view, &pending, &self.placed, &cx) {
                self.placed.insert(placement.hwnd);
                *placed_in.entry(placement.group.clone()).or_default() += 1;
                self.place(&placement, &monitors);
            }
        }
        if placed_in.is_empty() {
            return;
        }
        self.last_placed = Some(self.ticks);
        for (group, count) in placed_in {
            log::info!(
                "placed {count} {} windows on their monitors and desktops as they opened",
                self.label_of(&group)
            );
        }
    }

    /// One placement of `place_new`; each step is logged only in debug,
    /// because the group's restore logs every window again.
    pub(super) fn place(&self, placement: &Placement, monitors: &[monitors::Monitor]) {
        let hwnd = placement.hwnd as HWND;
        let label = &placement.label;
        if let Some((from, to)) = placement.monitor {
            match monitors::move_window(hwnd, &monitors[from], &monitors[to], monitors) {
                Ok(()) => log::debug!(
                    "put \"{label}\" on monitor {} as it opened",
                    monitors[to].id().device
                ),
                Err(err) => {
                    log::warn!("could not put \"{label}\" on its monitor as it opened: {err}")
                }
            }
        }
        if let (Some(target), Mover::Ready(mover)) = (placement.desktop, &self.mover) {
            match mover.move_to(hwnd, target) {
                Ok(()) => log::debug!("moved \"{label}\" to desktop {target} as it opened"),
                Err(err) => {
                    log::warn!("could not move \"{label}\" to desktop {target} as it opened: {err}")
                }
            }
        }
    }

    /// Looks at the windows of every restore whose check is due.
    pub(super) fn run_checks(&mut self) {
        if !self.checks.iter().any(|check| check.due <= self.ticks) {
            return;
        }
        let (due, later): (Vec<Check>, Vec<Check>) = std::mem::take(&mut self.checks)
            .into_iter()
            .partition(|check| check.due <= self.ticks);
        self.checks = later;
        let live = self.live_windows();
        let monitors = monitors::list();
        let registry = desktops::list();
        for check in due {
            self.check_restore(check, &live, &monitors, &registry);
        }
    }

    /// Where each window is now; None for one no longer open.
    fn where_now(
        &self,
        hwnds: &[Handle],
        live: &[windows::LiveWindow],
        monitors: &[monitors::Monitor],
    ) -> Vec<Option<verify::Seen>> {
        hwnds
            .iter()
            .map(|hwnd| {
                let window = live.iter().find(|window| window.hwnd as Handle == *hwnd)?;
                Some(verify::Seen {
                    desktop: self.reader.as_ref().and_then(|r| r.read(window.hwnd)),
                    monitor: monitors::index_of(window.identity.rect, monitors)
                        .map(|index| monitors[index].id().device.clone()),
                })
            })
            .collect()
    }

    /// The first look moves the windows out of place once more and looks
    /// again later; the second look logs how each group came out, each
    /// window still out of place, and the taskbar order on each desktop.
    pub(super) fn check_restore(
        &mut self,
        mut check: Check,
        live: &[windows::LiveWindow],
        monitors: &[monitors::Monitor],
        registry: &[Desktop],
    ) {
        let hwnds: Vec<Handle> = check.targets.iter().map(|target| target.hwnd).collect();
        let seen = self.where_now(&hwnds, live, monitors);
        let wrong = verify::wrong(&check.targets, &seen);
        if check.moved_again.is_none() && !wrong.is_empty() {
            log::info!(
                "check after the restore: {} windows are not where the restore put them; moving them again",
                wrong.len()
            );
            for out in &wrong {
                let target = check.targets[out.index].clone();
                if let Some((device, _)) = &out.monitor {
                    self.move_to_monitor(target.hwnd as HWND, device);
                }
                if let Some((desktop, _)) = out.desktop {
                    self.move_to_desktop(target.hwnd as HWND, desktop, registry, &target.label);
                }
            }
            check.moved_again = Some(wrong.iter().map(|out| out.index).collect());
            check.due = self.ticks + CHECK_SECONDS;
            self.checks.push(check);
            return;
        }
        let moved_again = check.moved_again.unwrap_or_default();
        let desktop_name = |id: DesktopId| {
            desktops::name_of(registry, id).unwrap_or_else(|| "an unknown desktop".to_string())
        };
        for (group, tally) in verify::tallies(&check.targets, &seen, &wrong) {
            let label = self.label_of(&group);
            let again = moved_again
                .iter()
                .filter(|index| check.targets[**index].group == group)
                .count();
            log::info!("{}", verify::group_line(&label, &tally, again));
            for out in wrong
                .iter()
                .filter(|out| check.targets[out.index].group == group)
            {
                log::warn!(
                    "{}",
                    verify::window_line(&check.targets[out.index], out, &desktop_name)
                );
            }
        }
        for (group, order) in &check.orders {
            let label = self.label_of(group);
            let now = self.where_now(order, live, monitors);
            let applied: Vec<verify::Placed> = order
                .iter()
                .zip(now)
                .filter_map(|(hwnd, now)| {
                    let now = now?;
                    let target = check.targets.iter().find(|target| target.hwnd == *hwnd);
                    let name = target
                        .map(|target| target.label.clone())
                        .unwrap_or_else(|| window_names::display_title(*hwnd as HWND));
                    Some((name, target.and_then(|target| target.desktop), now.desktop))
                })
                .collect();
            for (desktop, wanted, now) in verify::desktop_orders(&applied) {
                log::info!(
                    "{}",
                    verify::order_line(&label, &desktop_name(desktop), &wanted, &now)
                );
            }
        }
    }
}
