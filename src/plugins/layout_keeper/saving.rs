use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use super::*;
use crate::core::window_snapshot;

/// A timer read that has not come back in this long is given up, and the
/// next timer save asks again.
const TIMER_READ_GIVE_UP: Duration = Duration::from_secs(10);

/// What a save reads from Windows: the watched windows, top of the z-order
/// first, and the desktop of each one that has a known one.
pub(super) struct Seen {
    pub(super) live: Vec<windows::LiveWindow>,
    pub(super) on_desktop: BTreeMap<Handle, DesktopId>,
}

/// The windows a timer save is reading on a thread of its own.
pub(super) struct TimerRead {
    pub(super) coming: Receiver<window_snapshot::Snapshot>,
    pub(super) asked: Instant,
    /// `restores_started` when the read was asked for.
    pub(super) restores: u64,
}

impl LayoutKeeper {
    /// A fresh look at the windows, written to the state file (see
    /// `plan_save`). Returns whether anything was written and how many
    /// windows the file holds.
    fn write_layout(&mut self, reason: &str, force: bool) -> Result<(bool, usize), String> {
        let seen = self.seen_now();
        self.write_from(&seen, reason, force)
    }

    /// The watched windows and their desktops, read on the host now.
    pub(super) fn seen_now(&self) -> Seen {
        let live = self.live_windows();
        let on_desktop = live
            .iter()
            .filter_map(|window| {
                let desktop = self.reader.as_ref()?.read(window.hwnd)?;
                Some((window.hwnd as Handle, desktop))
            })
            .collect();
        Seen { live, on_desktop }
    }

    /// The watched windows and their desktops, from windows read on a
    /// thread of their own. Nothing is asked from Windows here.
    pub(super) fn seen_in(&self, snapshot: window_snapshot::Snapshot) -> Seen {
        let on_desktop = snapshot
            .windows
            .iter()
            .filter_map(|window| Some((window.hwnd as Handle, window.desktop?)))
            .collect();
        let watched = |exe: &str| self.watch.covers(exe);
        let live = window_snapshot::while_used(snapshot, || window_snapshot::live(&watched))
            .unwrap_or_default();
        Seen { live, on_desktop }
    }

    /// `write_layout` over windows already read.
    pub(super) fn write_from(
        &mut self,
        seen: &Seen,
        reason: &str,
        force: bool,
    ) -> Result<(bool, usize), String> {
        let desktops = desktops::list();
        let monitors = monitors::list();
        let connected = monitors::saved_names(&monitors);
        let world = World {
            desktops: &desktops,
            monitors: &connected,
            desktop_of: &|hwnd| seen.on_desktop.get(&(hwnd as Handle)).copied(),
            monitor_of: &|rect| monitors::index_of(rect, &monitors),
            today: clock::local_iso(SystemTime::now())
                .get(..10)
                .map(str::to_string),
        };
        self.write_seen(&seen.live, &world, reason, force)
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
        let wrote = self.writer.write_when(
            plan.programs,
            reason,
            clock::local_iso(SystemTime::now()),
            force,
            write_when(reason),
        )?;
        self.released.clear();
        if plan.backup {
            self.guard.wrote(reason == "timer");
        }
        Ok((wrote, plan.count))
    }

    /// The save every `snapshot_seconds`. The windows are read on a thread
    /// of their own and the layout is saved when they are back
    /// (`timer_read_arrived`), so the host does not wait for them. During a
    /// restore, and without a desktop reader or a host window, the host
    /// reads them itself as before.
    pub(super) fn timer_save(&mut self) {
        let can_read_later =
            self.reader.is_some() && self.host_hwnd.is_some() && !self.restore.is_running();
        let pending = self.timer_read.as_ref().map(|read| read.asked.elapsed());
        match timer_step(can_read_later, pending) {
            TimerStep::OnHost => {
                // A read still out is stale once a restore runs.
                self.timer_read = None;
                self.save("timer");
            }
            TimerStep::Wait => host::detailed(format_args!(
                "layout: the windows read {} ms ago are not back yet, so this timer save is skipped",
                pending.unwrap_or_default().as_millis()
            )),
            TimerStep::Read => {
                if let Some(waited) = pending {
                    log::info!(
                        "the windows read for the layout save did not come back in {} s; reading them again",
                        waited.as_secs()
                    );
                }
                let coming =
                    window_snapshot::read_later(|| host::post_to_plugins(TIMER_READ_ID));
                // A read given up is dropped with its receiver, so it can
                // never be saved.
                self.timer_read = Some(TimerRead {
                    coming,
                    asked: Instant::now(),
                    restores: self.restores_started,
                });
            }
        }
    }

    /// The windows a timer save read on a thread of its own may be back:
    /// the layout is built from them here and written on a thread of its
    /// own. A read that is stale is dropped (`stale_read`).
    pub(super) fn timer_read_arrived(&mut self) {
        let Some(read) = self.timer_read.take() else {
            return;
        };
        let snapshot = match read.coming.try_recv() {
            Ok(snapshot) => snapshot,
            Err(TryRecvError::Empty) => {
                self.timer_read = Some(read);
                return;
            }
            Err(TryRecvError::Disconnected) => {
                host::detailed(format_args!(
                    "layout: the windows could not be read on their own thread; the host reads them"
                ));
                self.save("timer");
                return;
            }
        };
        let waited = read.asked.elapsed().as_millis();
        let count = snapshot.windows.len();
        if let Some(why) = stale_read(
            self.host_hwnd.is_some(),
            read.restores,
            self.restores_started,
            self.restore.is_running() || self.restore_finishing(),
        ) {
            host::detailed(format_args!(
                "layout: {count} windows read on their own thread dropped, because {why}"
            ));
            return;
        }
        let started = Instant::now();
        let seen = self.seen_in(snapshot);
        let outcome = self.save_seen("timer", &seen);
        host::detailed(format_args!(
            "layout: {count} windows read on their own thread, back after {waited} ms; built on the host in {:.1} ms, {outcome}",
            started.elapsed().as_secs_f64() * 1000.0
        ));
    }

    pub(super) fn save(&mut self, reason: &str) {
        // Between the two halves of a restore the windows are half moved.
        if self.restore_finishing() {
            log::info!("a restore is under way, so the layout is not saved ({reason})");
            return;
        }
        let seen = self.seen_now();
        self.save_seen(reason, &seen);
    }

    /// `save` over windows already read. Returns a few words on what
    /// happened, for the detailed log.
    fn save_seen(&mut self, reason: &str, seen: &Seen) -> &'static str {
        match self.write_from(seen, reason, false) {
            Ok((true, count)) => {
                log::info!("saved {count} windows ({reason})");
                self.last_saved = Some(clock::now_hours_minutes());
                host::plugin_changed();
                match write_when(reason) {
                    state::When::Later => "changed, so the file is written on its own thread",
                    state::When::Now => "changed, so the file was written",
                }
            }
            Ok((false, _)) => {
                log::debug!("layout unchanged ({reason})");
                "unchanged"
            }
            Err(err) => {
                log::error!("could not save the layout: {err}");
                "the save failed"
            }
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

/// What a timer save does.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum TimerStep {
    /// Reads the windows on the host and saves now, as before: during a
    /// restore, or without a desktop reader or a host window.
    OnHost,
    /// Nothing: the windows asked for last time are still being read.
    Wait,
    /// Reads the windows on a thread of their own.
    Read,
}

/// `pending`: how long ago the read still out was asked for.
pub(super) fn timer_step(can_read_later: bool, pending: Option<Duration>) -> TimerStep {
    if !can_read_later {
        TimerStep::OnHost
    } else if pending.is_some_and(|waited| waited < TIMER_READ_GIVE_UP) {
        TimerStep::Wait
    } else {
        TimerStep::Read
    }
}

/// Why windows read on a thread of their own must not be saved, if they
/// must not: the plugin was switched off meanwhile, a restore started after
/// they were asked for, or one is under way, so they may show windows
/// half moved.
pub(super) fn stale_read(
    on: bool,
    restores_then: u64,
    restores_now: u64,
    restoring: bool,
) -> Option<&'static str> {
    if !on {
        Some("LayoutKeeper was switched off")
    } else if restores_now != restores_then {
        Some("a restore started after they were asked for")
    } else if restoring {
        Some("a restore is under way")
    } else {
        None
    }
}

/// The timer save writes on a thread of its own; every other save, the
/// end of a session among them, writes before it returns.
pub(super) fn write_when(reason: &str) -> state::When {
    if reason == "timer" {
        state::When::Later
    } else {
        state::When::Now
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
