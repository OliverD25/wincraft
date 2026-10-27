use super::*;

impl LayoutKeeper {
    /// A fresh look at the windows, written to the state file (see
    /// `plan_save`). Returns whether anything was written and how many
    /// windows the file holds.
    fn write_layout(&mut self, reason: &str, force: bool) -> Result<(bool, usize), String> {
        let live = self.live_windows();
        let desktops = desktops::list();
        let monitors = monitors::list();
        let connected = monitors::saved_names(&monitors);
        let on_desktop: BTreeMap<Handle, DesktopId> = live
            .iter()
            .filter_map(|window| {
                let desktop = self.reader.as_ref()?.read(window.hwnd)?;
                Some((window.hwnd as Handle, desktop))
            })
            .collect();
        let world = World {
            desktops: &desktops,
            monitors: &connected,
            desktop_of: &|hwnd| on_desktop.get(&(hwnd as Handle)).copied(),
            monitor_of: &|rect| monitors::index_of(rect, &monitors),
            today: clock::local_iso(SystemTime::now())
                .get(..10)
                .map(str::to_string),
        };
        self.write_seen(&live, &world, reason, force)
    }

    /// `write_layout` over windows already looked at.
    pub(super) fn write_seen(
        &mut self,
        live: &[windows::LiveWindow],
        world: &World,
        reason: &str,
        force: bool,
    ) -> Result<(bool, usize), String> {
        self.track(live);
        self.note_shared_titles(live);
        self.watch_bursts(live);
        let fresh = capture(
            live,
            world,
            &self.groups,
            self.writer.last(),
            &self.released,
        );
        // A group whose windows are still being checked after its restore
        // may have a window that a late pull has just put on the wrong
        // desktop; its saved windows wait for the check.
        let mut pending = self.restore.pending();
        for check in &self.checks {
            for target in &check.targets {
                if !pending.contains(&target.group) {
                    pending.push(target.group.clone());
                }
            }
        }
        let plan = plan_save(
            self.writer.last(),
            fresh,
            reason,
            &self.watch,
            &self.guard,
            pending,
        );
        if plan.copy_aside && self.writer.keep_previous()? {
            log::info!(
                "kept the previous layout as {} before writing over it",
                self.writer.prev_path().display()
            );
        }
        let wrote = self.writer.write(
            plan.programs,
            reason,
            clock::local_iso(SystemTime::now()),
            force,
        )?;
        self.released.clear();
        if plan.backup {
            self.guard.wrote(reason == "timer");
        }
        Ok((wrote, plan.count))
    }

    pub(super) fn save(&mut self, reason: &str) {
        // Between the two halves of a restore the windows are half moved.
        if self.restore_finishing() {
            log::info!("a restore is under way, so the layout is not saved ({reason})");
            return;
        }
        match self.write_layout(reason, false) {
            Ok((true, count)) => {
                log::info!("saved {count} windows ({reason})");
                self.last_saved = Some(clock::now_hours_minutes());
                host::plugin_changed();
            }
            Ok((false, _)) => log::debug!("layout unchanged ({reason})"),
            Err(err) => log::error!("could not save the layout: {err}"),
        }
    }

    /// The save at the end of a session or a run: the state file as usual,
    /// then the shutdown snapshot that the next sign-in restores from.
    pub(super) fn save_at_end(&mut self, reason: &str) {
        if self.restore_finishing() {
            log::info!("a restore is under way, so the layout is not saved ({reason})");
            return;
        }
        self.save(reason);
        match self
            .writer
            .write_snapshot(reason, clock::local_iso(SystemTime::now()))
        {
            Ok(Some(count)) => log::info!(
                "kept the layout of {count} windows in {} for the next sign-in ({reason})",
                self.writer.snapshot_path().display()
            ),
            Ok(None) => {}
            Err(err) => log::error!("could not keep the layout for the next sign-in: {err}"),
        }
    }

    /// Save layout now: always a fresh look and a write, and always a word
    /// back, because an unchanged layout used to make the hotkey look dead.
    pub(super) fn save_now(&mut self) {
        if self.restore_finishing() {
            log::info!("a restore is under way, so the layout is not saved (manual)");
            host::notify(
                "LayoutKeeper",
                "A restore is under way; the layout is saved once it is done",
            );
            return;
        }
        match self.write_layout("manual", true) {
            Ok((_, count)) => {
                log::info!("saved {count} windows (manual)");
                self.last_saved = Some(clock::now_hours_minutes());
                host::notify("LayoutKeeper", &format!("Layout saved, {count} windows"));
            }
            Err(err) => {
                log::error!("could not save the layout: {err}");
                host::notify(
                    "LayoutKeeper",
                    &format!("The layout could not be saved: {err}"),
                );
            }
        }
    }
}

/// What a save reads from Windows besides the window list, as plain values
/// and lookups, so tests can give answers of their own.
pub(super) struct World<'a> {
    pub(super) desktops: &'a [Desktop],
    /// The connected monitors as the layout file names them, in the order
    /// of `monitors::list`.
    pub(super) monitors: &'a [SavedMonitor],
    pub(super) desktop_of: &'a dyn Fn(HWND) -> Option<DesktopId>,
    /// The place in `monitors` of the monitor a rectangle is mostly on.
    pub(super) monitor_of: &'a dyn Fn(Rect) -> Option<usize>,
    /// The day a window is seen on, as the file writes it: "2026-10-18".
    pub(super) today: Option<String>,
}

/// The watched programs' windows as they are now, each program's list in
/// taskbar order. `z_index` counts across all programs, so the order
/// between apps can be put back too. A window saved on a monitor that is
/// unplugged now keeps that monitor, unless the user has put it on another
/// one in the strip since (`released`).
fn capture(
    live: &[windows::LiveWindow],
    world: &World,
    groups: &BTreeMap<String, OrderModel>,
    previous: Option<&Programs>,
    released: &BTreeSet<Handle>,
) -> Programs {
    let mut programs = Programs::new();
    let mut exes: Vec<&str> = Vec::new();
    for window in live {
        if !exes.contains(&window.identity.exe.as_str()) {
            exes.push(&window.identity.exe);
        }
    }
    for exe in exes {
        let mine: Vec<(usize, &windows::LiveWindow)> = live
            .iter()
            .enumerate()
            .filter(|(_, window)| window.identity.exe == exe)
            .collect();
        let mut saved: Vec<SavedWindow> = mine
            .iter()
            .map(|(z_index, window)| {
                let z_index = *z_index;
                let desktop = (world.desktop_of)(window.hwnd);
                SavedWindow {
                    name: window.identity.name.clone(),
                    title: window.identity.title.clone(),
                    rect: window.identity.rect,
                    maximized: window.identity.maximized,
                    desktop: desktop.map(|id| id.to_string()),
                    desktop_name: desktop.and_then(|id| desktops::name_of(world.desktops, id)),
                    group: window.group.clone(),
                    taskbar_index: groups
                        .get(&window.group)
                        .and_then(|group| group.position_of(window.hwnd as Handle))
                        .unwrap_or(z_index),
                    z_index,
                    monitor: (world.monitor_of)(window.identity.rect)
                        .and_then(|index| world.monitors.get(index).cloned()),
                    seen: world.today.clone(),
                }
            })
            .collect();
        if let Some(before) = previous.and_then(|programs| programs.get(exe)) {
            let released: Vec<bool> = mine
                .iter()
                .map(|(_, window)| released.contains(&(window.hwnd as Handle)))
                .collect();
            monitors::keep_unplugged(exe, &before.windows, &mut saved, &released, world.monitors);
        }
        saved.sort_by_key(|window| window.taskbar_index);
        programs.insert(exe.to_string(), ProgramState { windows: saved });
    }
    programs
}

/// What a save writes, decided before anything is written.
#[derive(Debug, PartialEq)]
pub(super) struct SavePlan {
    pub(super) programs: Programs,
    /// How many windows the file holds afterwards.
    pub(super) count: usize,
    /// The guard wants the old file kept before a held group's new windows
    /// go in; it hears when the write is done.
    pub(super) backup: bool,
    /// Copy the old file aside before this write.
    pub(super) copy_aside: bool,
}

/// Folds a fresh look into the saved layout. Groups in a crash burst keep
/// their saved windows for timer saves; groups still waiting for their
/// restore (`pending`) keep them for every save, because what is on screen
/// is the scrambled order the file fixes. The old file is copied aside only
/// when the guard asks for it and this write changes the file: a burst that
/// came back exactly as saved has nothing to lose.
pub(super) fn plan_save(
    previous: Option<&Programs>,
    fresh: Programs,
    reason: &str,
    watch: &Watch,
    guard: &guard::Guard,
    pending: Vec<String>,
) -> SavePlan {
    let timer = reason == "timer";
    let merged = state::merge(previous, fresh, reason == "shutdown", &|exe| {
        watch.covers(exe)
    });
    let mut held = if timer { guard.held() } else { BTreeSet::new() };
    held.extend(pending);
    let programs = state::keep_groups(previous, merged, &held);
    let count = programs.values().map(|program| program.windows.len()).sum();
    let backup = guard.backup_needed(timer);
    let copy_aside = backup && previous != Some(&programs);
    SavePlan {
        programs,
        count,
        backup,
        copy_aside,
    }
}

/// A timer save is due on every `snapshot_seconds`-th tick.
pub(super) fn save_due(ticks: u64, snapshot_seconds: u64) -> bool {
    ticks.is_multiple_of(snapshot_seconds.max(1))
}

/// One order model per saved taskbar group, in the saved taskbar order.
/// Windows from a version 1 file have no group yet; their groups are built
/// when the windows are first seen.
pub(super) fn saved_models(saved: Option<&Programs>) -> BTreeMap<String, OrderModel> {
    let mut groups = BTreeMap::new();
    for (exe, program) in saved.into_iter().flatten() {
        let mut windows = program.windows.clone();
        windows.sort_by_key(|window| window.taskbar_index);
        let mut by_group: BTreeMap<String, Vec<identity::WindowIdentity>> = BTreeMap::new();
        for window in windows.iter().filter(|window| !window.group.is_empty()) {
            by_group
                .entry(window.group.clone())
                .or_default()
                .push(window.identity(exe));
        }
        for (key, identities) in by_group {
            groups.insert(key, OrderModel::from_saved(identities));
        }
    }
    groups
}

/// What the hover strip counts an app's windows from (`core::window_census`).
pub(super) fn publish_census(groups: &BTreeMap<String, OrderModel>) {
    window_census::publish(census(groups));
}

pub(super) fn census(groups: &BTreeMap<String, OrderModel>) -> Vec<window_census::CensusWindow> {
    groups
        .iter()
        .flat_map(|(key, model)| {
            model
                .windows()
                .map(move |(_, identity)| window_census::CensusWindow {
                    group: key.clone(),
                    rect: identity.rect,
                })
        })
        .collect()
}
