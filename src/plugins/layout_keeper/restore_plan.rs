use super::*;

/// The saved programs a restore of `due` goes through: the watched ones
/// with a saved window in a group that is due.
pub(super) fn restore_exes(saved: &Programs, due: &[restore::Due], watch: &Watch) -> Vec<String> {
    saved
        .iter()
        .filter(|(exe, program)| {
            watch.covers(exe)
                && program
                    .windows
                    .iter()
                    .any(|window| due.iter().any(|due| due.group == window.group))
        })
        .map(|(exe, _)| exe.clone())
        .collect()
}

/// What a restore reads about one program before it changes anything.
pub(super) struct ExeView<'a> {
    pub(super) exe: &'a str,
    /// Its saved windows, in taskbar order.
    pub(super) saved: Vec<SavedWindow>,
    /// Its open windows, top of the z-order first.
    pub(super) mine: Vec<&'a windows::LiveWindow>,
    /// The desktop each open window is on before its button is re-added.
    pub(super) before: Vec<Option<DesktopId>>,
    /// The monitor each open window is on, as a place in the monitor list.
    on_monitor: Vec<Option<usize>>,
}

impl<'a> ExeView<'a> {
    pub(super) fn new(
        exe: &'a str,
        program: &ProgramState,
        live: &'a [windows::LiveWindow],
        desktop_of: &dyn Fn(HWND) -> Option<DesktopId>,
        monitor_of: &dyn Fn(Rect) -> Option<usize>,
    ) -> Self {
        let mut saved = program.windows.clone();
        saved.sort_by_key(|window| window.taskbar_index);
        let mine: Vec<&windows::LiveWindow> = live
            .iter()
            .filter(|window| window.identity.exe == exe)
            .collect();
        let before = mine.iter().map(|window| desktop_of(window.hwnd)).collect();
        let on_monitor = mine
            .iter()
            .map(|window| monitor_of(window.identity.rect))
            .collect();
        Self {
            exe,
            saved,
            mine,
            before,
            on_monitor,
        }
    }
}

/// The rest of what a restore plan reads.
pub(super) struct RestoreContext<'a> {
    /// Every open window, for the groups' order models.
    pub(super) live: &'a [windows::LiveWindow],
    /// The desktops that exist now.
    pub(super) known: &'a [DesktopId],
    /// The connected monitors as the layout file names them.
    pub(super) monitors: &'a [SavedMonitor],
    /// The desktop mover works, so a re-added button's pull can be undone.
    pub(super) can_move: bool,
    pub(super) restore_monitors: bool,
    pub(super) ticks: u64,
    pub(super) snapshot_seconds: u64,
}

/// What a restore does about one window's monitor.
#[derive(Debug, PartialEq)]
pub(super) struct MonitorStep {
    pub(super) hwnd: HWND,
    pub(super) label: String,
    pub(super) saved: Option<SavedMonitor>,
    /// The monitor it is on, as a place in the monitor list.
    pub(super) now: Option<usize>,
    pub(super) target: monitors::Return,
}

/// One taskbar group's share of a restore, decided before anything moves.
#[derive(Debug, Default, PartialEq)]
pub(super) struct GroupPlan {
    pub(super) group: String,
    /// A second pass, for windows that came after the group's restore.
    pub(super) late: bool,
    /// The order once more after a sign-in restore; like a second pass.
    pub(super) again: bool,
    /// Done before the taskbar order: with buttons on the taskbar of the
    /// window's own monitor, a move to another monitor moves its button too,
    /// and the order is rebuilt after that.
    pub(super) monitors: Vec<MonitorStep>,
    /// Desktop moves for the second half of the restore.
    pub(super) moves: Vec<(HWND, DesktopId, String)>,
    /// Windows saved on a desktop that no longer exists, and that desktop.
    pub(super) gone: Vec<(String, DesktopId)>,
    /// In a second pass, the late windows that change desktop.
    pub(super) moved_late: usize,
    /// Saved windows matched to an open one, of `saved`.
    pub(super) matched: usize,
    pub(super) saved: usize,
    /// The saved windows no open window matched, in a first pass.
    pub(super) lost: Vec<String>,
}

/// Plans the restore of one program's groups in `due` and brings the run's
/// bookkeeping up to date: each group gets an order model built from its
/// saved windows, every matched window counts as placed, and a first pass
/// adds them to the front-to-back stack. In a second pass the windows this
/// run placed already stay on the monitor and desktop they are on now.
pub(super) fn plan_restore(
    view: &ExeView,
    due: &[restore::Due],
    cx: &RestoreContext,
    groups: &mut BTreeMap<String, OrderModel>,
    placed: &mut BTreeSet<Handle>,
    stack: &mut Vec<(usize, HWND)>,
) -> Vec<GroupPlan> {
    let windows = &view.saved;
    let mine = &view.mine;
    let saved_ids: Vec<identity::WindowIdentity> = windows
        .iter()
        .map(|window| window.identity(view.exe))
        .collect();
    let live_ids: Vec<identity::WindowIdentity> =
        mine.iter().map(|window| window.identity.clone()).collect();
    let matching = identity::match_windows(&saved_ids, &live_ids);
    let live_groups: Vec<String> = mine.iter().map(|window| window.group.clone()).collect();

    let mut plans = Vec::new();
    for entry in due
        .iter()
        .filter(|entry| live_groups.contains(&entry.group))
    {
        let key = &entry.group;
        let again = entry.pass == restore::Pass::Again;
        let late = entry.pass == restore::Pass::Late || again;
        let members = state::saved_for_group(windows, &matching, &live_groups, key);
        // The model takes the same pairs as the moves below, so a window
        // gets its taskbar place and its desktop from the same saved entry
        // even when two saved windows share a title.
        let mut model = OrderModel::from_matched(
            members
                .iter()
                .map(|s| {
                    let paired = matching
                        .pairs
                        .iter()
                        .find(|(saved, live)| saved == s && live_groups[*live] == *key)
                        .map(|(_, live)| mine[*live].hwnd as Handle);
                    (saved_ids[*s].clone(), paired)
                })
                .collect(),
        );
        model.refresh(&handles_in(cx.live, key), cx.ticks, cx.snapshot_seconds);
        groups.insert(key.clone(), model);
        let pairs: Vec<(usize, usize)> = matching
            .pairs
            .iter()
            .filter(|(s, l)| members.contains(s) && live_groups[*l] == *key)
            .copied()
            .collect();
        let mut plan = GroupPlan {
            group: key.clone(),
            late,
            again,
            matched: pairs.len(),
            saved: members.len(),
            ..GroupPlan::default()
        };
        if cx.restore_monitors {
            for (s, l) in &pairs {
                let window = mine[*l];
                if late && placed.contains(&(window.hwnd as Handle)) {
                    continue;
                }
                let saved = windows[*s].monitor.clone();
                let now = view.on_monitor[*l];
                plan.monitors.push(MonitorStep {
                    hwnd: window.hwnd,
                    label: window.identity.label().to_string(),
                    target: monitors::restore_target(saved.as_ref(), now, cx.monitors),
                    saved,
                    now,
                });
            }
        }
        if cx.can_move {
            for (l, window) in mine.iter().enumerate() {
                if live_groups[l] != *key {
                    continue;
                }
                let kept = late && placed.contains(&(window.hwnd as Handle));
                let saved_desktop = pairs
                    .iter()
                    .find(|(_, live)| *live == l && !kept)
                    .and_then(|(s, _)| windows[*s].desktop.as_deref());
                if !late {
                    let gone = saved_desktop
                        .and_then(DesktopId::parse)
                        .filter(|id| *id != DesktopId::ALL && !cx.known.contains(id));
                    if let Some(gone) = gone {
                        plan.gone.push((window.identity.label().to_string(), gone));
                    }
                }
                let before = view.before[l];
                if let Some(target) = desktop_target(before, saved_desktop, cx.known) {
                    if late && !kept && Some(target) != before {
                        plan.moved_late += 1;
                    }
                    plan.moves
                        .push((window.hwnd, target, window.identity.label().to_string()));
                }
            }
        }
        for (s, l) in &pairs {
            placed.insert(mine[*l].hwnd as Handle);
            if !late {
                stack.push((windows[*s].z_index, mine[*l].hwnd));
            }
        }
        if !late {
            plan.lost = members
                .iter()
                .filter(|s| !pairs.iter().any(|(paired, _)| paired == *s))
                .map(|s| windows[*s].label().to_string())
                .collect();
        }
        plans.push(plan);
    }
    plans
}

/// A window to put in place the moment it appears.
#[derive(Debug, PartialEq)]
pub(super) struct Placement {
    pub(super) hwnd: Handle,
    pub(super) group: String,
    pub(super) label: String,
    /// From and to, as places in the monitor list.
    pub(super) monitor: Option<(usize, usize)>,
    pub(super) desktop: Option<DesktopId>,
}

/// The open windows of one program that belong to a group still in the
/// restore, are not placed yet, and match a saved window of that group: the
/// monitor and desktop moves each needs (None for one already in place; it
/// is still listed, so it counts as placed).
pub(super) fn plan_placements(
    view: &ExeView,
    pending: &[String],
    placed: &BTreeSet<Handle>,
    cx: &RestoreContext,
) -> Vec<Placement> {
    let saved_ids: Vec<identity::WindowIdentity> = view
        .saved
        .iter()
        .map(|window| window.identity(view.exe))
        .collect();
    let live_ids: Vec<identity::WindowIdentity> = view
        .mine
        .iter()
        .map(|window| window.identity.clone())
        .collect();
    let matching = identity::match_windows(&saved_ids, &live_ids);
    let mut placements = Vec::new();
    for (s, l) in matching.pairs {
        let window = view.mine[l];
        let saved = &view.saved[s];
        let same_group = saved.group.is_empty() || saved.group == window.group;
        if !same_group
            || !pending.contains(&window.group)
            || placed.contains(&(window.hwnd as Handle))
        {
            continue;
        }
        let now = view.on_monitor[l];
        let monitor = match (cx.restore_monitors, now) {
            (true, Some(now)) => {
                match monitors::restore_target(saved.monitor.as_ref(), Some(now), cx.monitors) {
                    monitors::Return::To(to) if to != now => Some((now, to)),
                    _ => None,
                }
            }
            _ => None,
        };
        let before = view.before[l];
        let desktop = cx
            .can_move
            .then(|| desktop_target(before, saved.desktop.as_deref(), cx.known))
            .flatten()
            .filter(|target| Some(*target) != before && *target != DesktopId::ALL);
        placements.push(Placement {
            hwnd: window.hwnd as Handle,
            group: window.group.clone(),
            label: window.identity.label().to_string(),
            monitor,
            desktop,
        });
    }
    placements
}

/// What the first half of a restore run did, for its summary.
#[derive(Default)]
pub(super) struct Restored {
    pub(super) moves: Vec<(HWND, DesktopId, String)>,
    pub(super) targets: Vec<verify::Target>,
    pub(super) orders: Vec<(String, Vec<Handle>)>,
    /// "2 of 3 Chrome windows", one per group restored.
    pub(super) parts: Vec<String>,
    pub(super) late_parts: Vec<String>,
    /// Saved windows no open window matched.
    pub(super) missing: usize,
}

/// The line the log, the status and perhaps a balloon show for a restore,
/// or None when it restored nothing.
pub(super) fn summary(done: &Restored) -> Option<String> {
    if done.parts.is_empty() && done.late_parts.is_empty() {
        return None;
    }
    let mut sentences = Vec::new();
    if !done.parts.is_empty() {
        let mut summary = format!("Restored {}", done.parts.join(", "));
        if done.missing > 0 {
            summary.push_str(&format!(" ({} not found)", done.missing));
        }
        sentences.push(summary);
    }
    sentences.extend(done.late_parts.iter().cloned());
    Some(sentences.join("; "))
}

/// The second half's work: the desktop moves that still change something,
/// then the windows to raise in that order, so the front window ends on
/// top and the window that was in front before the restore above them all.
pub(super) fn finish_steps(
    finish: &Finish,
    current: Option<DesktopId>,
    front_order: bool,
) -> (Vec<&(HWND, DesktopId, String)>, Vec<HWND>) {
    let moves = finish
        .moves
        .iter()
        .filter(|(_, target, _)| Some(*target) != current)
        .collect();
    let mut raises = Vec::new();
    if front_order && !finish.stack.is_empty() {
        let mut stack = finish.stack.clone();
        stack.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        raises.extend(stack.into_iter().map(|(_, hwnd)| hwnd));
        if !finish.foreground.is_null() {
            raises.push(finish.foreground);
        }
    }
    (moves, raises)
}

/// Where one group's restore wants each window, for the check after it: the
/// desktop of its move (which only undoes the pull for a window that stays)
/// and the monitor its monitor step leaves it on.
pub(super) fn check_targets(
    plan: &GroupPlan,
    monitors: &[monitors::Monitor],
) -> Vec<verify::Target> {
    check_targets_with(plan, &|index| {
        monitors
            .get(index)
            .map(|monitor| monitor.id().device.clone())
    })
}

/// `check_targets` with the device name of each monitor place given.
pub(super) fn check_targets_with(
    plan: &GroupPlan,
    device: &dyn Fn(usize) -> Option<String>,
) -> Vec<verify::Target> {
    let mut targets: Vec<verify::Target> = Vec::new();
    for step in &plan.monitors {
        let monitor = match step.target {
            monitors::Return::To(index) => device(index),
            monitors::Return::Stay => step.now.and_then(device),
            monitors::Return::Unplugged => None,
        };
        targets.push(verify::Target {
            hwnd: step.hwnd as Handle,
            group: plan.group.clone(),
            label: step.label.clone(),
            desktop: None,
            monitor,
        });
    }
    for (hwnd, desktop, label) in &plan.moves {
        let desktop = Some(*desktop).filter(|id| *id != DesktopId::ALL);
        match targets
            .iter_mut()
            .find(|target| target.hwnd == *hwnd as Handle)
        {
            Some(target) => target.desktop = desktop,
            None => targets.push(verify::Target {
                hwnd: *hwnd as Handle,
                group: plan.group.clone(),
                label: label.clone(),
                desktop,
                monitor: None,
            }),
        }
    }
    targets.retain(|target| target.desktop.is_some() || target.monitor.is_some());
    targets
}

/// What a run that timed out says when none of the saved apps opened a
/// window in time, so it restored nothing and waits for nothing.
pub(super) fn nothing_restored(run: &Restore) -> Option<String> {
    (run.restored() == 0 && run.waiting_count() == 0).then(|| {
        format!(
            "None of the saved apps opened a window within {} minutes, so nothing was restored",
            run.give_up_seconds / 60
        )
    })
}

/// Puts a window on top of the z-order without activating it.
/// Where a restored window goes, given the desktop it is on before its
/// button is re-added (which pulls it to the current one) and the desktop it
/// was saved on. A window on no desktop, or pinned to all of them, is never
/// moved; a saved desktop that is pinned or gone sends the window back where
/// it was, only undoing the pull.
pub(super) fn desktop_target(
    now: Option<DesktopId>,
    saved: Option<&str>,
    known: &[DesktopId],
) -> Option<DesktopId> {
    let now = now.filter(|id| known.contains(id))?;
    let saved = saved
        .and_then(DesktopId::parse)
        .filter(|id| known.contains(id));
    Some(saved.unwrap_or(now))
}

/// Whether a window's program answers messages now. A window that is still
/// busy being made (Chrome loading a restored window) may not have its
/// taskbar button in place yet, so its group's order waits a little.
pub(super) fn answers(hwnd: HWND) -> bool {
    if unsafe { IsHungAppWindow(hwnd) } != 0 {
        return false;
    }
    let mut result = 0usize;
    unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_NULL,
            0,
            0,
            SMTO_ABORTIFHUNG,
            ANSWER_TIMEOUT_MS,
            &mut result,
        ) != 0
    }
}

/// How long a window may take to answer before it counts as busy.
const ANSWER_TIMEOUT_MS: u32 = 50;

pub(super) fn raise(hwnd: HWND) {
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}
