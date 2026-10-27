mod explorer;
mod guard;
mod monitors;
// The look of the panel is shared with the host's update notice.
pub(crate) mod notice;
mod restore;
mod state;
pub mod thumbnail_probe;
mod verify;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, IsHungAppWindow, KillTimer, SendMessageTimeoutW, SetTimer,
    SetWindowPos, ENDSESSION_CLOSEAPP, HWND_TOP, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_SHOWNORMAL, WM_ENDSESSION, WM_NULL, WM_QUERYENDSESSION, WM_TIMER,
};

use crate::core::app_names::{self, app_label, friendly_key, with_start_menu_name};
use crate::core::live_windows::{self as windows, handles_in};
use crate::core::strip_groups;
use crate::core::taskbar_order as order;
use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    TrayAction, WinCraftPlugin, WindowGroups,
};
use crate::core::ui_bridge::{
    ActionKind, ArrangeAction, ArrangeDesktop, ArrangeGroup, LayoutAction, LayoutApp, LayoutEntry,
    LayoutRef, LayoutSnapshot,
};
use crate::core::virtual_desktops as desktops;
use crate::core::window_actions;
use crate::core::window_identity as identity;
use crate::core::{appid, clock, host, instance, session, wide, window_census, window_names};
use desktops::{Desktop, DesktopId};
use identity::{browser_name, Rect};
use order::{Handle, OrderModel};
use restore::Restore;
use state::{ProgramState, Programs, SavedMonitor, SavedWindow};

/// The host window is shared, so the timer id spells "LK" to stay clear of
/// any other plugin's.
const TIMER_ID: usize = 0x4C4B;
const TICK_MS: u32 = 1000;
/// The restore notice fades with its own short timer, only while it fades.
const FADE_TIMER_ID: usize = 0x4C4B_0001;
const FADE_STEP_MS: u32 = 16;
const FADE_MS: u64 = 250;

const ACTION_RESTORE: u32 = 1;
const ACTION_SAVE_NOW: u32 = 2;
const ACTION_MOVE_LEFT: u32 = 3;
const ACTION_MOVE_RIGHT: u32 = 4;

const VK_OPEN_BRACKET: u32 = 0xDB;
const VK_CLOSE_BRACKET: u32 = 0xDD;
const COMMAND_OPEN_STATE: u32 = 1;

/// Every app; the old default was "chrome.exe".
const DEFAULT_PROGRAMS: &str = "*";
const OLD_DEFAULT_PROGRAMS: &str = "chrome.exe";
const DEFAULT_SNAPSHOT_SECONDS: u64 = 30;
const DEFAULT_SETTLE_SECONDS: u64 = 3;
const DEFAULT_RESTORE_MINUTES: u64 = 30;
/// Until 0.11.1 these were the defaults, so a saved 5 or 3 is not a choice.
const OLD_DEFAULT_SETTLE_SECONDS: u64 = 5;
/// The default before windows were placed the moment they appear; the wait
/// now only decides when the taskbar order is put back.
const OLD_DEFAULT_SETTLE_SECONDS_2: u64 = 15;
const OLD_DEFAULT_RESTORE_MINUTES: u64 = 3;
/// How the crash guard times a hold. They do not follow the restore
/// settings: a wait of 30 minutes for apps to open is far too long to keep
/// saves held after a browser restart.
const GUARD_STABLE_SECONDS: u64 = 30;
const GUARD_REFILL_SECONDS: u64 = 180;

/// The second half of a restore, run one tick after the first.
struct Finish {
    moves: Vec<(HWND, DesktopId, String)>,
    stack: Vec<(usize, HWND)>,
    foreground: HWND,
    summary: String,
    /// Show the summary as a balloon too, not only in the log.
    announce: bool,
    /// Where each restored window should end up, for the check after it.
    targets: Vec<verify::Target>,
    /// Each restored group's windows in the taskbar order that was applied.
    orders: Vec<(String, Vec<Handle>)>,
}

/// How long after a restore, and after moving the stragglers again, the
/// windows are looked at. Re-added buttons pull windows to the current
/// desktop a moment late, more so while the PC is busy after sign-in.
const CHECK_SECONDS: u64 = 3;

/// A look at the windows of a restore that is done: were they all put where
/// they belong? Out of place windows are moved once more and looked at again.
struct Check {
    due: u64,
    targets: Vec<verify::Target>,
    orders: Vec<(String, Vec<Handle>)>,
    /// The places in `targets` of the windows moved a second time; set once
    /// the first look is over.
    moved_again: Option<Vec<usize>>,
}

use desktops::LazyMover as Mover;

pub struct LayoutKeeper {
    host_hwnd: Option<HWND>,
    watch: Watch,
    snapshot_seconds: u64,
    restore_on_start: bool,
    front_order: bool,
    /// "Restore monitors": a restore moves windows back to their monitor.
    restore_monitors: bool,
    /// Windows the user put on a monitor in the strip since the last save,
    /// which therefore no longer belong on an unplugged one.
    released: BTreeSet<Handle>,
    ticks: u64,
    started_once: bool,
    reader: Option<desktops::Reader>,
    /// The layout file `init` reads and the writer writes.
    state_path: std::path::PathBuf,
    writer: state::Writer,
    /// One order model per taskbar group, keyed like Windows keys the group:
    /// by AppUserModelID, or by program path when a window names none.
    groups: BTreeMap<String, OrderModel>,
    labels: BTreeMap<String, String>,
    /// Groups labelled before the apps index was ready, from the program
    /// file's own words; they get their Start menu name once it is.
    unresolved: BTreeSet<String>,
    taskbar: Option<order::Taskbar>,
    mover: Mover,
    restore: Restore,
    finishing: Option<Finish>,
    /// Every window this restore run has put back, with its saved place in
    /// the front-to-back order, so each later group raises them all again.
    restored_stack: Vec<(usize, HWND)>,
    /// The windows this restore run has already placed. A second pass leaves
    /// them where they are now, in case the person has moved them since.
    placed: BTreeSet<Handle>,
    /// This run has already shown its one balloon.
    announced: bool,
    /// Windows saved for each group when the restore started, to tell how
    /// far along an app is.
    expected: BTreeMap<String, usize>,
    /// "Show restore progress": the please-wait notice and the tray tooltip.
    show_progress: bool,
    notice: Option<notice::Notice>,
    notice_ctl: notice::Controller,
    fading_since: Option<u64>,
    /// The tooltip last handed to the host.
    tray_tip: Option<String>,
    /// The titles shared by windows of one app at the last save, to log
    /// only when the set changes.
    shared_titles: BTreeMap<(String, String), usize>,
    /// Groups to rebuild again on the next tick, after a desktop move that
    /// Windows only reports a moment later.
    reapply: Vec<String>,
    /// Explorer restarted, so its taskbar buttons are in its own order.
    explorer: explorer::Rebuild,
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
    /// Restores whose windows are still to be looked at.
    checks: Vec<Check>,
    /// When this restore run last placed a window, for the panel's cap.
    last_placed: Option<u64>,
    /// The progress bar's fill never drops below this in a run.
    bar_floor: u16,
    /// The panel has shown and gone in this run; it does not come back for
    /// windows the user opens later.
    panel_closed: bool,
}

impl Default for LayoutKeeper {
    fn default() -> Self {
        Self {
            host_hwnd: None,
            watch: Watch::default(),
            snapshot_seconds: DEFAULT_SNAPSHOT_SECONDS,
            restore_on_start: true,
            front_order: true,
            restore_monitors: true,
            released: BTreeSet::new(),
            ticks: 0,
            started_once: false,
            reader: None,
            state_path: state::path(),
            writer: state::Writer::default(),
            groups: BTreeMap::new(),
            labels: BTreeMap::new(),
            unresolved: BTreeSet::new(),
            taskbar: None,
            mover: Mover::Untried,
            restore: Restore::new(DEFAULT_SETTLE_SECONDS, DEFAULT_RESTORE_MINUTES * 60),
            finishing: None,
            restored_stack: Vec::new(),
            placed: BTreeSet::new(),
            announced: false,
            expected: BTreeMap::new(),
            show_progress: true,
            notice: None,
            notice_ctl: notice::Controller::default(),
            fading_since: None,
            tray_tip: None,
            shared_titles: BTreeMap::new(),
            reapply: Vec::new(),
            explorer: explorer::Rebuild::default(),
            arrange_stale: false,
            guard: guard::Guard::default(),
            last_saved: None,
            last_restore: None,
            read_only: instance::is_test(),
            refused: 0,
            checks: Vec::new(),
            last_placed: None,
            bar_floor: 0,
            panel_closed: false,
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
        self.restore_monitors = flag(settings, "restore_monitors");
        self.show_progress = flag(settings, "show_restore_progress");
        if !self.show_progress {
            self.hide_notice();
        }
        self.restore.settle_seconds =
            number(settings, "settle_seconds", 1, 60).unwrap_or(DEFAULT_SETTLE_SECONDS);
        self.restore.give_up_seconds = number(settings, "restore_window_minutes", 1, 120)
            .unwrap_or(DEFAULT_RESTORE_MINUTES)
            * 60;
    }

    /// The watched programs' app windows, top of the z-order first.
    fn live_windows(&self) -> Vec<windows::LiveWindow> {
        windows::on_desktops(&|exe| self.watch.covers(exe), self.reader.as_ref())
    }

    /// Looks at the windows and brings every group's order model up to date.
    fn refresh(&mut self) -> Vec<windows::LiveWindow> {
        let live = self.live_windows();
        self.track(&live);
        live
    }

    /// Brings every group's order model up to date with these windows.
    fn track(&mut self, live: &[windows::LiveWindow]) {
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
    fn remember_label(&mut self, window: &windows::LiveWindow) {
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
    fn group_name(&mut self, group: &str) -> String {
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

    fn label_of(&self, key: &str) -> String {
        self.labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// What the Layout page calls a saved group. A group seen live this
    /// session has its label already; the others are named from the Start
    /// menu entry of their app ID, else from the program.
    fn saved_label(&mut self, exe: &str, group: &str) -> String {
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
    fn open_matches(
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
    fn edit_saved(
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
    fn watch_bursts(&mut self, live: &[windows::LiveWindow]) {
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
    fn note_shared_titles(&mut self, live: &[windows::LiveWindow]) {
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
    fn write_seen(
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

    /// The save at the end of a session or a run: the state file as usual,
    /// then the shutdown snapshot that the next sign-in restores from.
    fn save_at_end(&mut self, reason: &str) {
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
    fn put_back_after_explorer(&mut self) {
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

    /// How many windows are saved for each taskbar group.
    fn saved_counts(&self) -> BTreeMap<String, usize> {
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

    /// Starts a restore run. `sign_in`: the run that follows signing in,
    /// whose groups get their taskbar order a second time.
    fn start_restore(&mut self, sign_in: bool) {
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
    fn restore_groups(&mut self, due: Vec<restore::Due>) {
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
    fn carry_out(
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
    fn finish_restore(&mut self, finish: Finish) {
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
    fn ensure_mover(&mut self, registry: &[Desktop]) -> bool {
        self.mover.ensure(registry)
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
        let Mover::Ready(mover) = &self.mover else {
            return;
        };
        window_actions::move_to_desktop(mover, self.reader.as_ref(), hwnd, target, registry, label);
    }

    /// Puts a restored window back on the monitor it was saved on.
    fn return_to_monitor(&mut self, step: &MonitorStep, monitors: &[monitors::Monitor]) {
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
    fn move_to_monitor(&mut self, hwnd: HWND, device: &str) {
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
    fn place_new(&mut self, live: &[windows::LiveWindow]) {
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
    fn place(&self, placement: &Placement, monitors: &[monitors::Monitor]) {
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
    fn run_checks(&mut self) {
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
    fn check_restore(
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
        if save_due(self.ticks, self.snapshot_seconds) {
            self.save("timer");
        }
    }

    /// For `--show-restore-panel` in a test instance: the panel with sample
    /// progress, so it can be looked at without restoring anything.
    fn show_demo_panel(&mut self) {
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

    fn fade_step(&mut self) {
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

    fn hide_notice(&mut self) {
        if let Some(hwnd) = self.host_hwnd {
            unsafe { KillTimer(hwnd, FADE_TIMER_ID) };
        }
        self.fading_since = None;
        self.notice_ctl.reset();
        if let Some(window) = self.notice.as_mut() {
            window.hide();
        }
    }

    fn open_state_file(&mut self) {
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

/// Whether this start is the one that follows the user signing in. Any
/// other start (a manual restart, an update, Windows starting WinCraft again
/// after an installer closed it) restores nothing, because the windows are
/// already where the user left them.
fn start_follows_sign_in() -> bool {
    let (age, source) = session::sign_in_age();
    let again = host::started_again();
    let restores = session::restores_at_start(age, again);
    if restores {
        log::info!(
            "restore on start: WinCraft started {} after sign-in",
            session::describe(age)
        );
    } else if again {
        log::info!(
            "no restore: WinCraft was started again, not by signing in ({} after sign-in)",
            session::describe(age)
        );
    } else {
        log::info!(
            "no restore: WinCraft started {} after sign-in{}",
            session::describe(age),
            if source == session::Source::Boot {
                " (counted from the start of the PC)"
            } else {
                ""
            }
        );
    }
    restores
}

/// The layout `init` starts from: the state file, or the shutdown snapshot
/// when nothing was saved after it (see `state::pick`).
fn start_layout(
    file: Option<state::StateFile>,
    snapshot: Option<state::StateFile>,
) -> Option<state::StateFile> {
    let time =
        |file: &Option<state::StateFile>| file.as_ref().map(|file| clock::parse_iso(&file.saved));
    match state::pick(time(&file), time(&snapshot)) {
        state::Source::State => {
            if let (Some(file), Some(snapshot)) = (&file, &snapshot) {
                log::info!(
                    "the state file (saved {}) is newer than the layout kept at the end of the last session (saved {}, {}), so it is used",
                    file.saved,
                    snapshot.saved,
                    snapshot.reason
                );
            }
            file
        }
        state::Source::Snapshot => {
            if let Some(snapshot) = &snapshot {
                log::info!(
                    "using the layout kept at the end of the last session (saved {}, {}); nothing was saved after it",
                    snapshot.saved,
                    snapshot.reason
                );
            }
            snapshot
        }
    }
}

/// What a save reads from Windows besides the window list, as plain values
/// and lookups, so tests can give answers of their own.
struct World<'a> {
    desktops: &'a [Desktop],
    /// The connected monitors as the layout file names them, in the order
    /// of `monitors::list`.
    monitors: &'a [SavedMonitor],
    desktop_of: &'a dyn Fn(HWND) -> Option<DesktopId>,
    /// The place in `monitors` of the monitor a rectangle is mostly on.
    monitor_of: &'a dyn Fn(Rect) -> Option<usize>,
    /// The day a window is seen on, as the file writes it: "2026-10-18".
    today: Option<String>,
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
struct SavePlan {
    programs: Programs,
    /// How many windows the file holds afterwards.
    count: usize,
    /// The guard wants the old file kept before a held group's new windows
    /// go in; it hears when the write is done.
    backup: bool,
    /// Copy the old file aside before this write.
    copy_aside: bool,
}

/// Folds a fresh look into the saved layout. Groups in a crash burst keep
/// their saved windows for timer saves; groups still waiting for their
/// restore (`pending`) keep them for every save, because what is on screen
/// is the scrambled order the file fixes. The old file is copied aside only
/// when the guard asks for it and this write changes the file: a burst that
/// came back exactly as saved has nothing to lose.
fn plan_save(
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

/// The saved programs a restore of `due` goes through: the watched ones
/// with a saved window in a group that is due.
fn restore_exes(saved: &Programs, due: &[restore::Due], watch: &Watch) -> Vec<String> {
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
struct ExeView<'a> {
    exe: &'a str,
    /// Its saved windows, in taskbar order.
    saved: Vec<SavedWindow>,
    /// Its open windows, top of the z-order first.
    mine: Vec<&'a windows::LiveWindow>,
    /// The desktop each open window is on before its button is re-added.
    before: Vec<Option<DesktopId>>,
    /// The monitor each open window is on, as a place in the monitor list.
    on_monitor: Vec<Option<usize>>,
}

impl<'a> ExeView<'a> {
    fn new(
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
struct RestoreContext<'a> {
    /// Every open window, for the groups' order models.
    live: &'a [windows::LiveWindow],
    /// The desktops that exist now.
    known: &'a [DesktopId],
    /// The connected monitors as the layout file names them.
    monitors: &'a [SavedMonitor],
    /// The desktop mover works, so a re-added button's pull can be undone.
    can_move: bool,
    restore_monitors: bool,
    ticks: u64,
    snapshot_seconds: u64,
}

/// What a restore does about one window's monitor.
#[derive(Debug, PartialEq)]
struct MonitorStep {
    hwnd: HWND,
    label: String,
    saved: Option<SavedMonitor>,
    /// The monitor it is on, as a place in the monitor list.
    now: Option<usize>,
    target: monitors::Return,
}

/// One taskbar group's share of a restore, decided before anything moves.
#[derive(Debug, Default, PartialEq)]
struct GroupPlan {
    group: String,
    /// A second pass, for windows that came after the group's restore.
    late: bool,
    /// The order once more after a sign-in restore; like a second pass.
    again: bool,
    /// Done before the taskbar order: with buttons on the taskbar of the
    /// window's own monitor, a move to another monitor moves its button too,
    /// and the order is rebuilt after that.
    monitors: Vec<MonitorStep>,
    /// Desktop moves for the second half of the restore.
    moves: Vec<(HWND, DesktopId, String)>,
    /// Windows saved on a desktop that no longer exists, and that desktop.
    gone: Vec<(String, DesktopId)>,
    /// In a second pass, the late windows that change desktop.
    moved_late: usize,
    /// Saved windows matched to an open one, of `saved`.
    matched: usize,
    saved: usize,
    /// The saved windows no open window matched, in a first pass.
    lost: Vec<String>,
}

/// Plans the restore of one program's groups in `due` and brings the run's
/// bookkeeping up to date: each group gets an order model built from its
/// saved windows, every matched window counts as placed, and a first pass
/// adds them to the front-to-back stack. In a second pass the windows this
/// run placed already stay on the monitor and desktop they are on now.
fn plan_restore(
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
struct Placement {
    hwnd: Handle,
    group: String,
    label: String,
    /// From and to, as places in the monitor list.
    monitor: Option<(usize, usize)>,
    desktop: Option<DesktopId>,
}

/// The open windows of one program that belong to a group still in the
/// restore, are not placed yet, and match a saved window of that group: the
/// monitor and desktop moves each needs (None for one already in place; it
/// is still listed, so it counts as placed).
fn plan_placements(
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
struct Restored {
    moves: Vec<(HWND, DesktopId, String)>,
    targets: Vec<verify::Target>,
    orders: Vec<(String, Vec<Handle>)>,
    /// "2 of 3 Chrome windows", one per group restored.
    parts: Vec<String>,
    late_parts: Vec<String>,
    /// Saved windows no open window matched.
    missing: usize,
}

/// The line the log, the status and perhaps a balloon show for a restore,
/// or None when it restored nothing.
fn summary(done: &Restored) -> Option<String> {
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
fn finish_steps(
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
fn check_targets(plan: &GroupPlan, monitors: &[monitors::Monitor]) -> Vec<verify::Target> {
    check_targets_with(plan, &|index| {
        monitors
            .get(index)
            .map(|monitor| monitor.id().device.clone())
    })
}

/// `check_targets` with the device name of each monitor place given.
fn check_targets_with(
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

/// A timer save is due on every `snapshot_seconds`-th tick.
fn save_due(ticks: u64, snapshot_seconds: u64) -> bool {
    ticks.is_multiple_of(snapshot_seconds.max(1))
}

/// What a run that timed out says when none of the saved apps opened a
/// window in time, so it restored nothing and waits for nothing.
fn nothing_restored(run: &Restore) -> Option<String> {
    (run.restored() == 0 && run.waiting_count() == 0).then(|| {
        format!(
            "None of the saved apps opened a window within {} minutes, so nothing was restored",
            run.give_up_seconds / 60
        )
    })
}

/// One order model per saved taskbar group, in the saved taskbar order.
/// Windows from a version 1 file have no group yet; their groups are built
/// when the windows are first seen.
fn saved_models(saved: Option<&Programs>) -> BTreeMap<String, OrderModel> {
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

/// Whether a window's program answers messages now. A window that is still
/// busy being made (Chrome loading a restored window) may not have its
/// taskbar button in place yet, so its group's order waits a little.
fn answers(hwnd: HWND) -> bool {
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
        windows::is_app(&exe)
            && !self.excluded.contains(&exe)
            && (self.all || self.programs.contains(&exe))
    }
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

/// A number setting that still holds the old default was never a choice, so
/// it takes the new default. Any other number is kept.
fn migrate_default(settings: &mut serde_json::Value, key: &str, old: u64, new: u64) -> bool {
    if settings.get(key).and_then(|value| value.as_f64()) != Some(old as f64) {
        return false;
    }
    settings[key] = serde_json::Value::from(new);
    true
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

/// How many windows the taskbar button's app has, on every desktop.
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
            WM_QUERYENDSESSION | WM_ENDSESSION if closes_app => self.save("shutdown"),
            WM_QUERYENDSESSION => self.save_at_end("shutdown"),
            WM_ENDSESSION if wparam != 0 => self.save_at_end("shutdown"),
            _ => {}
        }
    }

    fn teardown(&mut self) {
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

/// What the hover strip counts an app's windows from (`core::window_census`).
fn publish_census(groups: &BTreeMap<String, OrderModel>) {
    window_census::publish(census(groups));
}

fn census(groups: &BTreeMap<String, OrderModel>) -> Vec<window_census::CensusWindow> {
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

#[cfg(test)]
mod tests;
