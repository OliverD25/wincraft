mod appid;
mod desktops;
mod guard;
mod identity;
mod monitors;
mod order;
mod restore;
mod state;
mod windows;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsIconic, KillTimer, PostMessageW, SetTimer, SetWindowPos, ShowWindow,
    HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOWNORMAL, WM_CLOSE,
    WM_ENDSESSION, WM_QUERYENDSESSION, WM_TIMER,
};

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    TrayAction, WinCraftPlugin, WindowGroups,
};
use crate::core::ui_bridge::{
    ActionKind, ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeWindow,
};
use crate::core::{clock, host, instance, wide};
use desktops::{Desktop, DesktopId};
use order::{Handle, OrderModel};
use restore::{Restore, Step};
use state::{ProgramState, Programs, SavedWindow};

/// The host window is shared, so the timer id spells "LK" to stay clear of
/// any other plugin's.
const TIMER_ID: usize = 0x4C4B;
const TICK_MS: u32 = 1000;

const ACTION_RESTORE: u32 = 1;
const ACTION_SAVE_NOW: u32 = 2;
const ACTION_MOVE_LEFT: u32 = 3;
const ACTION_MOVE_RIGHT: u32 = 4;
const ACTION_ARRANGE: u32 = 5;

const VK_OPEN_BRACKET: u32 = 0xDB;
const VK_CLOSE_BRACKET: u32 = 0xDD;
const COMMAND_OPEN_STATE: u32 = 1;

/// Every app; the old default was "chrome.exe".
const DEFAULT_PROGRAMS: &str = "*";
const OLD_DEFAULT_PROGRAMS: &str = "chrome.exe";
/// WinCraft's own windows are never arranged: the strip would arrange itself.
const OWN_EXE: &str = "wincraft.exe";
/// Windows' own helper processes. They can own visible windows with titles
/// (rundll32 hosts the Sound control panel, the shell hosts draw Start,
/// search, the touch keyboard and the lock screen), but they are not apps a
/// person arranges, and a restore must never move them. Always skipped,
/// whatever the settings say. ApplicationFrameHost is not here: it owns the
/// windows of Settings and other Store apps.
const SYSTEM_HELPERS: &[&str] = &[
    "rundll32.exe",
    "dllhost.exe",
    "ShellExperienceHost.exe",
    "StartMenuExperienceHost.exe",
    "SearchHost.exe",
    "TextInputHost.exe",
    "LockApp.exe",
    "ShellHost.exe",
];
const DEFAULT_SNAPSHOT_SECONDS: u64 = 30;
const DEFAULT_SETTLE_SECONDS: u64 = 5;
const DEFAULT_RESTORE_MINUTES: u64 = 3;

/// The second half of a restore, run one tick after the first.
struct Finish {
    moves: Vec<(HWND, DesktopId, String)>,
    stack: Vec<(usize, HWND)>,
    foreground: HWND,
    summary: String,
}

/// The undocumented move chain is built on first use and kept for the
/// session, or given up on for the session after one failure.
#[derive(Default)]
enum Mover {
    #[default]
    Untried,
    Ready(desktops::Mover),
    Disabled(String),
}

pub struct LayoutKeeper {
    host_hwnd: Option<HWND>,
    watch: Watch,
    snapshot_seconds: u64,
    restore_on_start: bool,
    front_order: bool,
    preview_on_hover: bool,
    ticks: u64,
    started_once: bool,
    reader: Option<desktops::Reader>,
    writer: state::Writer,
    /// One order model per taskbar group, keyed like Windows keys the group:
    /// by AppUserModelID, or by program path when a window names none.
    groups: BTreeMap<String, OrderModel>,
    labels: BTreeMap<String, String>,
    taskbar: Option<order::Taskbar>,
    mover: Mover,
    restore: Restore,
    finishing: Option<Finish>,
    /// Every window this restore run has put back, with its saved place in
    /// the front-to-back order, so each later group raises them all again.
    restored_stack: Vec<(usize, HWND)>,
    /// Groups to rebuild again on the next tick, after a desktop move that
    /// Windows only reports a moment later.
    reapply: Vec<String>,
    /// The strip needs fresh data on the next tick.
    arrange_stale: bool,
    guard: guard::Guard,
    last_saved: Option<String>,
    last_restore: Option<String>,
    /// A test instance (WINCRAFT_INSTANCE) watches, saves and shows the
    /// strip, but never restores, reorders, moves, raises or closes the
    /// user's real windows.
    read_only: bool,
    /// Changes a read-only instance turned down, for its status line.
    refused: u32,
}

impl Default for LayoutKeeper {
    fn default() -> Self {
        Self {
            host_hwnd: None,
            watch: Watch::default(),
            snapshot_seconds: DEFAULT_SNAPSHOT_SECONDS,
            restore_on_start: true,
            front_order: true,
            preview_on_hover: true,
            ticks: 0,
            started_once: false,
            reader: None,
            writer: state::Writer::default(),
            groups: BTreeMap::new(),
            labels: BTreeMap::new(),
            taskbar: None,
            mover: Mover::Untried,
            restore: Restore::new(DEFAULT_SETTLE_SECONDS, DEFAULT_RESTORE_MINUTES * 60),
            finishing: None,
            restored_stack: Vec::new(),
            reapply: Vec::new(),
            arrange_stale: false,
            guard: guard::Guard::default(),
            last_saved: None,
            last_restore: None,
            read_only: instance::is_test(),
            refused: 0,
        }
    }
}

impl LayoutKeeper {
    pub fn new() -> Self {
        Self::default()
    }

    /// True, after logging it, when this is a read-only test instance and
    /// `what` would change a real window; the caller then does nothing.
    fn refuses(&mut self, what: &str) -> bool {
        if !self.read_only {
            return false;
        }
        self.refused += 1;
        log::info!("read-only test instance: {what} skipped");
        true
    }

    fn apply_settings(&mut self, settings: &serde_json::Value) {
        let text = |key: &str, default: &'static str| {
            settings
                .get(key)
                .and_then(|value| value.as_str())
                .unwrap_or(default)
                .to_string()
        };
        self.watch = Watch::new(
            &text("programs", DEFAULT_PROGRAMS),
            &text("excluded_programs", ""),
        );
        self.snapshot_seconds = number(settings, "snapshot_interval_seconds", 5, 600)
            .unwrap_or(DEFAULT_SNAPSHOT_SECONDS);
        self.restore_on_start = flag(settings, "restore_on_start");
        self.front_order = flag(settings, "restore_front_order");
        self.preview_on_hover = flag(settings, "preview_on_hover");
        self.restore.settle_seconds =
            number(settings, "settle_seconds", 1, 60).unwrap_or(DEFAULT_SETTLE_SECONDS);
        self.restore.give_up_seconds = number(settings, "restore_window_minutes", 1, 30)
            .unwrap_or(DEFAULT_RESTORE_MINUTES)
            * 60;
    }

    /// The watched programs' app windows, top of the z-order first.
    fn live_windows(&self) -> Vec<windows::LiveWindow> {
        let registry = desktops::list();
        // Without the desktop reader a cloaked window cannot be placed, and
        // it is kept as it was before the reader existed.
        let on_known_desktop = |hwnd: HWND| match &self.reader {
            Some(reader) => reader
                .read(hwnd)
                .is_some_and(|id| registry.iter().any(|desktop| desktop.id == id)),
            None => true,
        };
        windows::enumerate(&|exe| self.watch.covers(exe), &on_known_desktop)
    }

    /// Looks at the windows and brings every group's order model up to date.
    fn refresh(&mut self) -> Vec<windows::LiveWindow> {
        let live = self.live_windows();
        let mut keys: Vec<String> = Vec::new();
        for window in &live {
            if !keys.contains(&window.group) {
                keys.push(window.group.clone());
            }
            if !self.labels.contains_key(&window.group) {
                let has_id = window.group != window.exe_path.to_lowercase();
                let name = window.app_name.clone().or_else(|| {
                    has_id
                        .then(|| appid::registered_name(&window.group))
                        .flatten()
                });
                let exe = &window.identity.exe;
                let label = match browser_name(exe) {
                    Some(browser) => appid::group_label(
                        browser,
                        name.as_deref(),
                        &window.group,
                        &window.exe_path,
                    ),
                    None => app_label(
                        name.as_deref(),
                        windows::file_description(&window.exe_path).as_deref(),
                        exe,
                    ),
                };
                log::debug!("new taskbar group {} labelled \"{label}\"", window.group);
                self.labels.insert(window.group.clone(), label);
            }
        }
        let known: Vec<String> = self.groups.keys().cloned().collect();
        for key in known {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in keys {
            let mine = handles_in(&live, &key);
            self.groups
                .entry(key)
                .or_default()
                .refresh(&mine, self.ticks, self.snapshot_seconds);
        }
        self.groups.retain(|_, group| !group.is_empty());
        live
    }

    fn label_of(&self, key: &str) -> String {
        self.labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// The watched programs' windows as they are now, each program's list in
    /// taskbar order. `z_index` counts across all programs, so the order
    /// between apps can be put back too.
    fn capture(&self, live: &[windows::LiveWindow]) -> Programs {
        let desktops = desktops::list();
        let monitors = monitors::list();
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
                    let desktop = self
                        .reader
                        .as_ref()
                        .and_then(|reader| reader.read(window.hwnd));
                    SavedWindow {
                        name: window.identity.name.clone(),
                        title: window.identity.title.clone(),
                        rect: window.identity.rect,
                        maximized: window.identity.maximized,
                        desktop: desktop.map(|id| id.to_string()),
                        desktop_name: desktop.and_then(|id| desktops::name_of(&desktops, id)),
                        group: window.group.clone(),
                        taskbar_index: self
                            .groups
                            .get(&window.group)
                            .and_then(|group| group.position_of(window.hwnd as Handle))
                            .unwrap_or(z_index),
                        z_index,
                        monitor: monitors::of_rect(window.identity.rect, &monitors),
                    }
                })
                .collect();
            saved.sort_by_key(|window| window.taskbar_index);
            programs.insert(exe.to_string(), ProgramState { windows: saved });
        }
        programs
    }

    /// Tells the guard what each taskbar group holds now and logs any hold
    /// that starts or ends.
    fn watch_bursts(&mut self, live: &[windows::LiveWindow]) {
        let mut groups: BTreeMap<String, Vec<Handle>> = BTreeMap::new();
        for window in live {
            groups
                .entry(window.group.clone())
                .or_default()
                .push(window.hwnd as Handle);
        }
        let timing = guard::Timing {
            stable_seconds: self.restore.settle_seconds * 6,
            refill_seconds: self.restore.give_up_seconds,
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

    /// A fresh look at the windows, written to the state file. A timer save
    /// keeps the saved windows of any group the guard holds, and the first
    /// write that puts a held group's new windows in the file copies the old
    /// file aside first. Returns whether anything was written and how many
    /// windows the file holds.
    fn write_layout(&mut self, reason: &str, force: bool) -> Result<(bool, usize), String> {
        let timer = reason == "timer";
        let live = self.refresh();
        self.watch_bursts(&live);
        let mut merged = state::merge(
            self.writer.last(),
            self.capture(&live),
            reason == "shutdown",
            &|exe| self.watch.covers(exe),
        );
        // Groups in a crash burst keep their saved windows for timer saves;
        // groups still waiting for their restore keep them for every save,
        // because what is on screen is the scrambled order the file fixes.
        let mut held = if timer {
            self.guard.held()
        } else {
            BTreeSet::new()
        };
        held.extend(self.restore.pending());
        merged = state::keep_groups(self.writer.last(), merged, &held);
        let count: usize = merged.values().map(|program| program.windows.len()).sum();
        let backup = self.guard.backup_needed(timer);
        // Only windows that differ from the file can lose anything; a burst
        // that came back exactly as saved needs no copy.
        let changes = self.writer.last() != Some(&merged);
        if backup && changes && self.writer.keep_previous()? {
            log::info!(
                "kept the previous layout as {} before writing over it",
                self.writer.prev_path().display()
            );
        }
        let wrote =
            self.writer
                .write(merged, reason, clock::local_iso(SystemTime::now()), force)?;
        if backup {
            self.guard.wrote(timer);
        }
        Ok((wrote, count))
    }

    fn save(&mut self, reason: &str) {
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

    /// Save layout now: always a fresh look and a write, and always a word
    /// back, because an unchanged layout used to make the hotkey look dead.
    fn save_now(&mut self) {
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

    /// Makes one taskbar group match its model, if it does not already. Only
    /// that group's buttons are rebuilt. Re-adding a button pulls a window on
    /// another desktop over to this one, so those windows are only included
    /// when `pull` is set and the caller will move them back.
    fn apply_group(&mut self, key: &str, pull: bool) {
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
        if self.taskbar.is_none() {
            match order::Taskbar::new() {
                Ok(taskbar) => self.taskbar = Some(taskbar),
                Err(err) => {
                    log::error!("{err}; the taskbar order cannot be changed");
                    return;
                }
            }
        }
        let Some(taskbar) = &self.taskbar else { return };
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
        let took = taskbar.apply(&send);
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

    /// Moves the front window one place along its taskbar group.
    fn shift_front(&mut self, step: isize) {
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

    fn restore_finishing(&self) -> bool {
        self.finishing.is_some()
    }

    /// The groups a restore waits for: every watched group with saved
    /// windows. Windows saved before groups were recorded have none and are
    /// left to the groups of the windows they match.
    fn saved_groups(&self) -> Vec<String> {
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

    fn start_restore(&mut self) {
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
            "restore watches {} saved taskbar groups for {} minutes; each is put back once its windows have held still for {} s",
            groups.len(),
            self.restore.give_up_seconds / 60,
            self.restore.settle_seconds
        );
        self.restored_stack.clear();
        self.restore.start(self.ticks, groups);
    }

    /// First half of restoring the groups in `due`: their taskbar order.
    /// Re-adding the buttons pulls every window onto the current desktop, a
    /// moment later and not at once, so the desktop moves and the
    /// front-to-back order wait for the next tick in `finish_restore`.
    /// Without the desktop mover only the windows already on this desktop
    /// are re-added. The current desktop and the foreground window are left
    /// as they are.
    fn restore_groups(&mut self, due: Vec<String>) {
        let Some(saved) = self.writer.last().cloned() else {
            return;
        };
        let live = self.refresh();
        let registry = desktops::list();
        let known: Vec<DesktopId> = registry.iter().map(|desktop| desktop.id).collect();
        let can_move = self.ensure_mover(&registry);
        let foreground = unsafe { GetForegroundWindow() };
        let mut moves: Vec<(HWND, DesktopId, String)> = Vec::new();
        let mut parts: Vec<String> = Vec::new();
        let mut missing = 0;

        let exes: Vec<String> = saved
            .iter()
            .filter(|(exe, program)| {
                self.watch.covers(exe)
                    && program
                        .windows
                        .iter()
                        .any(|window| due.contains(&window.group))
            })
            .map(|(exe, _)| exe.clone())
            .collect();
        for exe in exes {
            let mut windows = saved[&exe].windows.clone();
            windows.sort_by_key(|window| window.taskbar_index);
            let saved_ids: Vec<identity::WindowIdentity> =
                windows.iter().map(|window| window.identity(&exe)).collect();
            let mine: Vec<&windows::LiveWindow> = live
                .iter()
                .filter(|window| window.identity.exe == exe)
                .collect();
            let live_ids: Vec<identity::WindowIdentity> =
                mine.iter().map(|window| window.identity.clone()).collect();
            let matching = identity::match_windows(&saved_ids, &live_ids);
            // Read before any button is re-added, which pulls windows here.
            let before: Vec<Option<DesktopId>> = mine
                .iter()
                .map(|window| self.reader.as_ref().and_then(|r| r.read(window.hwnd)))
                .collect();
            let live_groups: Vec<String> = mine.iter().map(|window| window.group.clone()).collect();

            for key in due.iter().filter(|key| live_groups.contains(key)) {
                let members = state::saved_for_group(&windows, &matching, &live_groups, key);
                let ids = members.iter().map(|s| saved_ids[*s].clone()).collect();
                let mut model = OrderModel::from_saved(ids);
                model.refresh(&handles_in(&live, key), self.ticks, self.snapshot_seconds);
                self.groups.insert(key.clone(), model);
                self.apply_group(key, can_move);

                let pairs: Vec<(usize, usize)> = matching
                    .pairs
                    .iter()
                    .filter(|(s, l)| members.contains(s) && live_groups[*l] == *key)
                    .copied()
                    .collect();
                if can_move {
                    for (l, window) in mine.iter().enumerate() {
                        if live_groups[l] != *key {
                            continue;
                        }
                        let saved_desktop = pairs
                            .iter()
                            .find(|(_, live)| *live == l)
                            .and_then(|(s, _)| windows[*s].desktop.as_deref());
                        if let Some(target) = desktop_target(before[l], saved_desktop, &known) {
                            moves.push((window.hwnd, target, window.identity.label().to_string()));
                        }
                    }
                }
                for (s, l) in &pairs {
                    self.restored_stack
                        .push((windows[*s].z_index, mine[*l].hwnd));
                }

                let label = self.label_of(key);
                let lost: Vec<&str> = members
                    .iter()
                    .filter(|s| !pairs.iter().any(|(paired, _)| paired == *s))
                    .map(|s| windows[*s].label())
                    .collect();
                log::info!(
                    "restored {} of {} {label} windows",
                    pairs.len(),
                    members.len()
                );
                if !lost.is_empty() {
                    log::info!("not matched: {}", lost.join(" | "));
                }
                missing += lost.len();
                parts.push(format!(
                    "{} of {} {label} windows",
                    pairs.len(),
                    members.len()
                ));
            }
        }

        if parts.is_empty() {
            return;
        }
        let mut summary = format!("Restored {}", parts.join(", "));
        if missing > 0 {
            summary.push_str(&format!(" ({missing} not found)"));
        }
        self.finishing = Some(Finish {
            moves,
            stack: self.restored_stack.clone(),
            foreground,
            summary,
        });
    }

    /// Second half of a restore: desktops, then the front-to-back order.
    /// Right after the buttons were re-added Windows may still report a
    /// window's old desktop, so every window whose desktop is not the current
    /// one is moved without asking where it is.
    fn finish_restore(&mut self, finish: Finish) {
        if self.refuses("finishing a restore") {
            return;
        }
        let registry = desktops::list();
        let current = desktops::current();
        for (hwnd, target, label) in &finish.moves {
            if Some(*target) != current {
                self.move_to_desktop(*hwnd, *target, &registry, label);
            }
        }
        let mut stack = finish.stack;
        if self.front_order && !stack.is_empty() {
            stack.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, hwnd) in &stack {
                raise(*hwnd);
            }
            if !finish.foreground.is_null() {
                raise(finish.foreground);
            }
        }
        log::info!("{}", finish.summary);
        host::notify("LayoutKeeper", &finish.summary);
        self.last_restore = Some(format!(
            "{} at {}",
            finish.summary,
            clock::now_hours_minutes()
        ));
    }

    /// Builds the desktop mover on first use; true when it is usable.
    fn ensure_mover(&mut self, registry: &[Desktop]) -> bool {
        if matches!(self.mover, Mover::Untried) {
            self.mover = match desktops::Mover::new(registry) {
                Ok(mover) => {
                    log::info!("desktop moves are available");
                    Mover::Ready(mover)
                }
                Err(err) => {
                    log::warn!("desktop moves are disabled for this session: {err}");
                    Mover::Disabled(err)
                }
            };
        }
        matches!(self.mover, Mover::Ready(_))
    }

    fn move_to_desktop(
        &mut self,
        hwnd: HWND,
        target: DesktopId,
        registry: &[Desktop],
        label: &str,
    ) {
        if self.refuses("desktop move") {
            return;
        }
        if target == DesktopId::ALL {
            return;
        }
        let Some(name) = desktops::name_of(registry, target) else {
            log::info!("\"{label}\" was on desktop {target}, which no longer exists");
            return;
        };
        if self.reader.as_ref().and_then(|reader| reader.read(hwnd)) == Some(DesktopId::ALL) {
            return;
        }
        let Mover::Ready(mover) = &self.mover else {
            return;
        };
        match mover.move_to(hwnd, target) {
            Ok(()) => log::info!("moved \"{label}\" to {name}"),
            Err(err) => log::warn!("could not move \"{label}\" to {name}: {err}"),
        }
    }

    fn tick(&mut self) {
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
        if let Some(finish) = self.finishing.take() {
            self.finish_restore(finish);
            return;
        }
        if self.restore.is_running() {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for window in self.live_windows() {
                *counts.entry(window.group).or_default() += 1;
            }
            match self.restore.step(self.ticks, &counts) {
                Step::Apply(groups) => self.restore_groups(groups),
                Step::TimedOut(left) => {
                    let labels: Vec<String> =
                        left.iter().map(|group| self.label_of(group)).collect();
                    log::info!(
                        "stopped waiting after {} minutes; not restored because no windows settled: {}",
                        self.restore.give_up_seconds / 60,
                        labels.join(", ")
                    );
                    if self.restore.restored() == 0 {
                        let summary = format!(
                            "None of the saved apps settled within {} minutes, so nothing was restored",
                            self.restore.give_up_seconds / 60
                        );
                        host::notify("LayoutKeeper", &summary);
                        self.last_restore = Some(summary);
                    }
                }
                Step::Wait | Step::Nothing => {}
            }
        }
        if self.ticks % self.snapshot_seconds.max(1) == 0 {
            self.save("timer");
        }
    }

    fn open_state_file(&mut self) {
        if !state::path().exists() {
            self.save("manual");
        }
        let args = wide(&format!("\"{}\"", state::path().display()));
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

fn handles_in(live: &[windows::LiveWindow], key: &str) -> Vec<(Handle, identity::WindowIdentity)> {
    live.iter()
        .filter(|window| window.group == key)
        .map(|window| (window.hwnd as Handle, window.identity.clone()))
        .collect()
}

/// Puts a window on top of the z-order without activating it.
/// Where a restored window goes, given the desktop it is on before its
/// button is re-added (which pulls it to the current one) and the desktop it
/// was saved on. A window on no desktop, or pinned to all of them, is never
/// moved; a saved desktop that is pinned or gone sends the window back where
/// it was, only undoing the pull.
fn desktop_target(
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

fn raise(hwnd: HWND) {
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

/// Which programs LayoutKeeper looks after: every app ("*") or a list, less
/// the excluded ones, and never WinCraft itself.
#[derive(Clone, Debug, Default, PartialEq)]
struct Watch {
    all: bool,
    programs: Vec<String>,
    excluded: Vec<String>,
}

impl Watch {
    fn new(programs: &str, excluded: &str) -> Self {
        let programs = parse_programs(programs);
        Self {
            all: programs.iter().any(|program| program == "*"),
            programs: programs
                .into_iter()
                .filter(|program| program != "*")
                .collect(),
            excluded: parse_programs(excluded),
        }
    }

    fn covers(&self, exe: &str) -> bool {
        let exe = exe.to_lowercase();
        !exe.is_empty()
            && exe != OWN_EXE
            && !is_system_helper(&exe)
            && !self.excluded.contains(&exe)
            && (self.all || self.programs.contains(&exe))
    }
}

/// Matches the file name, in any case, whether given alone or as a path.
fn is_system_helper(exe: &str) -> bool {
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    SYSTEM_HELPERS
        .iter()
        .any(|helper| helper.eq_ignore_ascii_case(name))
}

/// "any app", "any app except telegram.exe", "chrome.exe, msedge.exe".
impl std::fmt::Display for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.all {
            f.write_str("any app")?;
            if !self.excluded.is_empty() {
                write!(f, " except {}", self.excluded.join(", "))?;
            }
            return Ok(());
        }
        f.write_str(&self.programs.join(", "))
    }
}

/// Settings from before 0.8 name only Chrome, which was the default then,
/// not a choice: that one value becomes "every app". A list the user wrote
/// is kept.
fn migrate_programs(settings: &mut serde_json::Value) -> bool {
    let old = settings.get("programs").and_then(|value| value.as_str());
    if old.map(str::trim) != Some(OLD_DEFAULT_PROGRAMS) {
        return false;
    }
    settings["programs"] = serde_json::Value::from(DEFAULT_PROGRAMS);
    true
}

/// The browsers keep their product name in front of each web app's name:
/// "Chrome · Gemini".
fn browser_name(exe: &str) -> Option<&'static str> {
    match exe {
        "chrome.exe" => Some("Chrome"),
        "msedge.exe" => Some("Edge"),
        "firefox.exe" => Some("Firefox"),
        "brave.exe" => Some("Brave"),
        _ => None,
    }
}

/// A name for a program known only by its file: "telegram.exe" -> "Telegram".
fn program_name(exe: &str) -> String {
    if let Some(browser) = browser_name(exe) {
        return browser.to_string();
    }
    let stem = exe.strip_suffix(".exe").unwrap_or(exe);
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => exe.to_string(),
    }
}

/// What the strip calls any other app's group: the name the app gives its
/// windows or its Start menu entry, else the description in its program
/// file, else its file name.
fn app_label(name: Option<&str>, description: Option<&str>, exe: &str) -> String {
    name.or(description)
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| program_name(exe))
}

fn parse_programs(text: &str) -> Vec<String> {
    let mut programs: Vec<String> = Vec::new();
    for name in text.split(',').map(|name| name.trim().to_lowercase()) {
        if !name.is_empty() && !programs.contains(&name) {
            programs.push(name);
        }
    }
    programs
}

fn number(settings: &serde_json::Value, key: &str, min: u64, max: u64) -> Option<u64> {
    let value = settings.get(key)?.as_f64()?;
    Some((value.round().max(0.0) as u64).clamp(min, max))
}

fn flag(settings: &serde_json::Value, key: &str) -> bool {
    settings
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

fn win_alt(vk: u32) -> Hotkey {
    Hotkey {
        modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
        vk,
    }
}

impl WinCraftPlugin for LayoutKeeper {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "layout_keeper",
            name: "LayoutKeeper",
            description: "Remembers which desktop each window is on, the taskbar thumbnail order and which window is in front, and restores them after a reboot.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
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
            "preview_on_hover": true,
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
                label: "Restore when WinCraft starts",
                help: "Put the saved layout back once the programs have opened their windows.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: "settle_seconds",
                label: "Wait for windows (seconds)",
                help:
                    "The restore starts when the number of windows has not changed for this long.",
                kind: FieldKind::Number {
                    min: 1.0,
                    max: 60.0,
                },
            },
            SettingField {
                key: "restore_window_minutes",
                label: "Give up after (minutes)",
                help: "Stop waiting if the windows never settle.",
                kind: FieldKind::Number {
                    min: 1.0,
                    max: 30.0,
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
                key: "preview_on_hover",
                label: "Preview window on hover",
                help: "Resting the mouse on a picture in the Arrange strip shows that window on its monitor.",
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
        let file = state::load();
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
        self.writer = state::Writer::new(state::path(), file.map(|file| file.programs));
        // Windows from a version 1 file have no group yet; their groups are
        // built when the windows are first seen.
        self.groups = BTreeMap::new();
        for (exe, program) in self.writer.last().cloned().unwrap_or_default() {
            let mut windows = program.windows;
            windows.sort_by_key(|window| window.taskbar_index);
            let mut by_group: BTreeMap<String, Vec<identity::WindowIdentity>> = BTreeMap::new();
            for window in windows.iter().filter(|window| !window.group.is_empty()) {
                by_group
                    .entry(window.group.clone())
                    .or_default()
                    .push(window.identity(&exe));
            }
            for (key, identities) in by_group {
                self.groups.insert(key, OrderModel::from_saved(identities));
            }
        }
        if unsafe { SetTimer(ctx.hwnd, TIMER_ID, TICK_MS, None) } == 0 {
            return Err("could not start its timer".to_string());
        }
        self.host_hwnd = Some(ctx.hwnd);
        log::info!("watching {}", self.watch);

        // Switching the plugin off and on again is not a reboot.
        if !self.started_once && self.restore_on_start {
            self.start_restore();
        }
        self.started_once = true;
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
            HotkeyAction {
                id: ACTION_ARRANGE,
                name: "arrange",
                label: "Arrange windows",
                default: win_alt(u32::from(b'A')),
            },
        ]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        match action_id {
            ACTION_RESTORE => self.start_restore(),
            ACTION_SAVE_NOW => self.save_now(),
            ACTION_MOVE_LEFT => self.shift_front(-1),
            ACTION_MOVE_RIGHT => self.shift_front(1),
            ACTION_ARRANGE => host::open_arrange(),
            _ => {}
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![
            TrayAction {
                id: ACTION_RESTORE,
                label: "Restore layout",
            },
            TrayAction {
                id: ACTION_ARRANGE,
                label: "Arrange windows",
            },
        ]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        match action_id {
            ACTION_RESTORE => self.start_restore(),
            ACTION_ARRANGE => host::open_arrange(),
            _ => {}
        }
    }

    fn page_action(&self) -> Option<u32> {
        Some(ACTION_ARRANGE)
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
        self.refresh();
        let registry = desktops::list();
        let current = desktops::current();
        let monitors = monitors::list();
        let mut groups: Vec<ArrangeGroup> = self
            .groups
            .iter()
            .filter_map(|(key, model)| {
                let windows: Vec<ArrangeWindow> = model
                    .windows()
                    .map(|(hwnd, identity)| {
                        let desktop = self
                            .reader
                            .as_ref()
                            .and_then(|reader| reader.read(hwnd as HWND))
                            .filter(|id| *id != DesktopId::ALL)
                            .map(|id| id.to_string())
                            .unwrap_or_default();
                        ArrangeWindow {
                            hwnd,
                            label: identity.label().to_string(),
                            desktop,
                            monitor: monitors::strip_label(identity.rect, &monitors),
                        }
                    })
                    .collect();
                (!windows.is_empty()).then(|| ArrangeGroup {
                    key: key.clone(),
                    label: self.label_of(key),
                    windows,
                })
            })
            .collect();

        // The switcher lists the apps with the most windows first.
        groups.sort_by(|a, b| {
            b.windows
                .len()
                .cmp(&a.windows.len())
                .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
        });

        // The strip opens on the front window's taskbar group when it is
        // watched, else on the busiest group.
        let front = unsafe { GetForegroundWindow() };
        let (_, front_key) = windows::describe(front);
        let focus = groups
            .iter()
            .position(|group| group.key == front_key)
            .unwrap_or(0);

        Some(WindowGroups {
            groups,
            desktops: registry
                .iter()
                .map(|desktop| ArrangeDesktop {
                    id: desktop.id.to_string(),
                    name: desktop.name.clone(),
                    current: Some(desktop.id) == current,
                })
                .collect(),
            focus,
            watched: vec![self.watch.to_string()],
            preview: self.preview_on_hover,
        })
    }

    fn on_arrange_action(&mut self, action: &ArrangeAction) {
        let what = match action {
            ArrangeAction::Activate(_) => "switching to a window",
            ArrangeAction::Reorder { .. } => "reordering from the strip",
            ArrangeAction::MoveToDesktop { .. } => "moving a window to another desktop",
            ArrangeAction::Close(_) => "closing a window",
        };
        if self.refuses(what) {
            return;
        }
        match action {
            ArrangeAction::Activate(hwnd) => {
                let hwnd = *hwnd as HWND;
                if unsafe { IsIconic(hwnd) } != 0 {
                    unsafe { ShowWindow(hwnd, SW_RESTORE) };
                }
                host::bring_to_front(hwnd);
            }
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
                    let reason = match &self.mover {
                        Mover::Disabled(reason) => reason.clone(),
                        _ => "not available".to_string(),
                    };
                    host::notify(
                        "LayoutKeeper",
                        &format!("Windows cannot be moved to another desktop: {reason}"),
                    );
                    return;
                }
                let label = windows::window_text(*hwnd as HWND);
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
            ArrangeAction::Close(hwnd) => {
                unsafe { PostMessageW(*hwnd as HWND, WM_CLOSE, 0, 0) };
                self.arrange_stale = true;
            }
        }
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: COMMAND_OPEN_STATE,
            label: "Open state file",
            hint: "",
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
        if self.restore.is_running() {
            parts.push(format!(
                "Restore waiting for {} apps to settle",
                self.restore.pending().len()
            ));
        }
        if let Some(restore) = &self.last_restore {
            parts.push(restore.clone());
        }
        parts.push(moves);
        Some(parts.join(" \u{b7} "))
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, _lparam: LPARAM) {
        match msg {
            WM_TIMER if wparam == TIMER_ID => self.tick(),
            WM_QUERYENDSESSION => self.save("shutdown"),
            WM_ENDSESSION if wparam != 0 => self.save("shutdown"),
            _ => {}
        }
    }

    fn teardown(&mut self) {
        if let Some(hwnd) = self.host_hwnd.take() {
            unsafe { KillTimer(hwnd, TIMER_ID) };
            self.save("teardown");
        }
        self.restore = Restore::new(self.restore.settle_seconds, self.restore.give_up_seconds);
        self.finishing = None;
        self.restored_stack.clear();
        self.reader = None;
        self.taskbar = None;
        self.mover = Mover::Untried;
        self.guard = guard::Guard::default();
        self.groups.clear();
        self.labels.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_is_watched_except_the_excluded_ones_and_wincraft() {
        let watch = Watch::new("*", " Telegram.exe, claude.exe");
        assert!(watch.covers("chrome.exe"));
        assert!(watch.covers("CHARMAP.EXE"));
        assert!(!watch.covers("telegram.exe"));
        assert!(!watch.covers("claude.exe"));
        assert!(!watch.covers("wincraft.exe"));
        assert!(!watch.covers(""));
        assert_eq!(watch.to_string(), "any app except telegram.exe, claude.exe");

        let listed = Watch::new("chrome.exe, charmap.exe", "");
        assert!(listed.covers("charmap.exe"));
        assert!(!listed.covers("telegram.exe"));
        assert_eq!(listed.to_string(), "chrome.exe, charmap.exe");
        // The exclude list wins over a list that names the same program.
        assert!(!Watch::new("chrome.exe", "chrome.exe").covers("chrome.exe"));
    }

    #[test]
    fn windows_helpers_are_matched_by_file_name_in_any_case() {
        for helper in SYSTEM_HELPERS {
            assert!(is_system_helper(helper), "{helper}");
            assert!(is_system_helper(&helper.to_lowercase()), "{helper}");
            assert!(is_system_helper(&helper.to_uppercase()), "{helper}");
        }
        assert!(is_system_helper(r"C:\Windows\System32\rundll32.exe"));
        assert!(is_system_helper(
            r"c:\windows\systemapps\shellexperiencehost_cw5n1h2txyewy\ShellExperienceHost.exe"
        ));
        assert!(is_system_helper("C:/Windows/System32/DllHost.exe"));
        // Only the whole file name counts.
        assert!(!is_system_helper("rundll32.exe.bak"));
        assert!(!is_system_helper("myrundll32.exe"));
        assert!(!is_system_helper(r"C:\Tools\rundll32\app.exe"));
        assert!(!is_system_helper("rundll32"));
        assert!(!is_system_helper(""));
    }

    #[test]
    fn real_apps_are_not_helpers() {
        for app in [
            "SystemSettings.exe",
            "ApplicationFrameHost.exe",
            "explorer.exe",
            "WindowsTerminal.exe",
            "chrome.exe",
            "notepad.exe",
            "Taskmgr.exe",
        ] {
            assert!(!is_system_helper(app), "{app}");
            assert!(Watch::new("*", "").covers(app), "{app}");
        }
    }

    #[test]
    fn helpers_are_skipped_whatever_the_program_list_says() {
        let every = Watch::new("*", "");
        let listed = Watch::new("rundll32.exe, dllhost.exe, chrome.exe", "");
        for watch in [&every, &listed] {
            assert!(!watch.covers("rundll32.exe"));
            assert!(!watch.covers("RUNDLL32.EXE"));
            assert!(!watch.covers("dllhost.exe"));
            assert!(!watch.covers("SearchHost.exe"));
            assert!(watch.covers("chrome.exe"));
        }
        assert!(!listed.covers("notepad.exe"));
    }

    #[test]
    fn the_users_own_exclude_list_still_applies_next_to_the_helpers() {
        let watch = Watch::new("*", "telegram.exe");
        assert!(!watch.covers("telegram.exe"));
        assert!(!watch.covers("rundll32.exe"));
        assert!(watch.covers("viber.exe"));
        // Naming a helper in the exclude list changes nothing either way.
        let both = Watch::new("*", "rundll32.exe");
        assert!(!both.covers("rundll32.exe"));
        assert!(both.covers("chrome.exe"));
        assert_eq!(watch.to_string(), "any app except telegram.exe");
    }

    fn saved(group: &str) -> state::SavedWindow {
        state::SavedWindow {
            name: Some(group.to_string()),
            title: group.to_string(),
            rect: [0, 0, 800, 600],
            maximized: false,
            desktop: None,
            desktop_name: None,
            group: group.to_string(),
            taskbar_index: 0,
            z_index: 0,
            monitor: None,
        }
    }

    fn old_file() -> Programs {
        let mut programs = Programs::new();
        for (exe, group) in [
            ("rundll32.exe", r"c:\windows\system32\rundll32.exe"),
            ("chrome.exe", "Chrome.UserData.Profile3"),
            ("dllhost.exe", r"c:\windows\system32\dllhost.exe"),
        ] {
            programs.insert(
                exe.to_string(),
                state::ProgramState {
                    windows: vec![saved(group)],
                },
            );
        }
        programs
    }

    #[test]
    fn a_helper_in_an_old_state_file_is_left_out_of_the_restore() {
        let mut keeper = LayoutKeeper {
            watch: Watch::new("*", ""),
            ..LayoutKeeper::default()
        };
        keeper.writer = state::Writer::new(
            std::env::temp_dir().join("wincraft-test-never-written.json"),
            Some(old_file()),
        );
        assert_eq!(keeper.saved_groups(), ["Chrome.UserData.Profile3"]);
    }

    #[test]
    fn a_helper_in_an_old_state_file_is_dropped_at_the_next_save() {
        let watch = Watch::new("*", "");
        let merged = state::merge(Some(&old_file()), Programs::new(), false, &|exe| {
            watch.covers(exe)
        });
        assert_eq!(merged.keys().collect::<Vec<_>>(), ["chrome.exe"]);
    }

    fn read_only() -> LayoutKeeper {
        LayoutKeeper {
            read_only: true,
            watch: Watch::new("*", ""),
            ..LayoutKeeper::default()
        }
    }

    // A handle no window has: if a guard were missing, the call would reach
    // Win32 with it and the refusal count would not move.
    const NO_WINDOW: HWND = 0x7fff_fff0 as HWND;

    #[test]
    fn a_normal_instance_is_not_read_only() {
        // cargo test runs without WINCRAFT_INSTANCE.
        assert!(!instance::is_test());
        let mut keeper = LayoutKeeper::default();
        assert!(!keeper.read_only);
        assert!(!keeper.refuses("anything"));
        assert_eq!(keeper.refused, 0);
    }

    #[test]
    fn a_read_only_instance_starts_no_restore() {
        let mut keeper = read_only();
        keeper.writer = state::Writer::new(
            std::env::temp_dir().join("wincraft-test-never-written.json"),
            Some(old_file()),
        );
        keeper.start_restore();
        assert_eq!(keeper.refused, 1);
        assert!(!keeper.restore.is_running());
    }

    #[test]
    fn a_read_only_instance_never_touches_the_taskbar_or_desktops() {
        let mut keeper = read_only();
        keeper.apply_group("Chrome.UserData.Profile3", true);
        assert!(keeper.taskbar.is_none(), "no ITaskbarList was made");
        keeper.move_to_desktop(NO_WINDOW, DesktopId(7), &[], "w");
        assert!(matches!(keeper.mover, Mover::Untried));
        keeper.shift_front(1);
        keeper.shift_front(-1);
        assert_eq!(keeper.refused, 4);
    }

    #[test]
    fn a_read_only_instance_does_not_finish_a_restore() {
        let mut keeper = read_only();
        keeper.finish_restore(Finish {
            moves: vec![(NO_WINDOW, DesktopId(7), "w".to_string())],
            stack: vec![(0, NO_WINDOW)],
            foreground: NO_WINDOW,
            summary: "Restored 1 of 1 test windows".to_string(),
        });
        assert_eq!(keeper.refused, 1);
        assert!(keeper.last_restore.is_none());
    }

    #[test]
    fn every_strip_action_is_turned_down_in_a_read_only_instance() {
        let mut keeper = read_only();
        let hwnd = NO_WINDOW as isize;
        let actions = [
            ArrangeAction::Activate(hwnd),
            ArrangeAction::Reorder {
                group: "Chrome.UserData.Profile3".to_string(),
                order: vec![hwnd],
            },
            ArrangeAction::MoveToDesktop {
                hwnd,
                desktop: "{00000000-0000-0000-0000-000000000007}".to_string(),
            },
            ArrangeAction::Close(hwnd),
        ];
        for (done, action) in actions.iter().enumerate() {
            keeper.on_arrange_action(action);
            assert_eq!(keeper.refused as usize, done + 1, "{action:?}");
        }
        assert!(keeper.reapply.is_empty());
        assert!(!keeper.arrange_stale);
    }

    #[test]
    fn the_hotkeys_that_move_windows_are_turned_down_too() {
        let mut keeper = read_only();
        keeper.on_hotkey(ACTION_RESTORE);
        keeper.on_hotkey(ACTION_MOVE_LEFT);
        keeper.on_hotkey(ACTION_MOVE_RIGHT);
        keeper.on_tray_action(ACTION_RESTORE);
        assert_eq!(keeper.refused, 4);
        let status = keeper.status().unwrap();
        assert!(
            status.contains("Read-only test instance: 4 changes"),
            "{status}"
        );
    }

    #[test]
    fn only_the_old_default_becomes_every_app() {
        let mut old = serde_json::json!({ "programs": "chrome.exe", "settle_seconds": 5 });
        assert!(migrate_programs(&mut old));
        assert_eq!(old["programs"], "*");
        assert_eq!(old["settle_seconds"], 5);

        for kept in ["chrome.exe, msedge.exe", "msedge.exe", "*"] {
            let mut own = serde_json::json!({ "programs": kept });
            assert!(!migrate_programs(&mut own), "{kept}");
            assert_eq!(own["programs"], kept);
        }
    }

    #[test]
    fn apps_are_named_by_what_they_call_themselves() {
        assert_eq!(
            app_label(Some("Telegram"), Some("Telegram Desktop"), "telegram.exe"),
            "Telegram"
        );
        assert_eq!(
            app_label(None, Some("Character Map"), "charmap.exe"),
            "Character Map"
        );
        assert_eq!(app_label(None, None, "claude.exe"), "Claude");
        assert_eq!(app_label(Some("  "), None, "code.exe"), "Code");
        assert_eq!(program_name("msedge.exe"), "Edge");
    }

    #[test]
    fn pinned_and_untracked_windows_are_never_moved() {
        let one = DesktopId(1);
        let two = DesktopId(2);
        let known = [one, two];
        let saved_two = two.to_string();
        // A window on desktop 1 that was saved on desktop 2 goes back there.
        assert_eq!(
            desktop_target(Some(one), Some(&saved_two), &known),
            Some(two)
        );
        // No desktop at all ("not tracked"), or pinned to every desktop.
        assert_eq!(desktop_target(None, Some(&saved_two), &known), None);
        assert_eq!(
            desktop_target(Some(DesktopId::ALL), Some(&saved_two), &known),
            None
        );
        assert_eq!(
            desktop_target(Some(DesktopId(9)), Some(&saved_two), &known),
            None
        );
        // Saved as pinned, or on a desktop that is gone: only undo the pull.
        let gone = DesktopId(9).to_string();
        assert_eq!(desktop_target(Some(one), Some(&gone), &known), Some(one));
        assert_eq!(desktop_target(Some(one), None, &known), Some(one));
        assert_eq!(desktop_target(Some(one), Some(""), &known), Some(one));
    }

    #[test]
    fn the_program_list_is_trimmed_lowercased_and_deduplicated() {
        assert_eq!(
            parse_programs(" Chrome.exe, ,msedge.exe,chrome.exe"),
            ["chrome.exe", "msedge.exe"]
        );
    }

    #[test]
    fn numbers_are_rounded_and_clamped() {
        let settings = serde_json::json!({ "a": 29.6, "b": 1.0, "c": "x" });
        assert_eq!(number(&settings, "a", 5, 600), Some(30));
        assert_eq!(number(&settings, "b", 5, 600), Some(5));
        assert_eq!(number(&settings, "c", 5, 600), None);
        assert_eq!(number(&settings, "missing", 5, 600), None);
    }

    #[test]
    fn switches_default_to_on() {
        let settings = serde_json::json!({ "off": false });
        assert!(!flag(&settings, "off"));
        assert!(flag(&settings, "missing"));
    }
}
