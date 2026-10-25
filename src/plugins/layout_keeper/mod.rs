mod desktops;
mod explorer;
mod guard;
pub mod hover_preview;
mod identity;
mod monitors;
// The look of the panel is shared with the host's update notice.
pub(crate) mod notice;
mod order;
mod restore;
mod state;
pub mod thumbnail_probe;
mod verify;
mod windows;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, IsHungAppWindow, IsIconic, KillTimer, PostMessageW,
    SendMessageTimeoutW, SetTimer, SetWindowPos, ShowWindow, ENDSESSION_CLOSEAPP, HWND_TOP,
    SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOWNORMAL, WM_CLOSE,
    WM_ENDSESSION, WM_NULL, WM_QUERYENDSESSION, WM_TIMER,
};

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    TrayAction, TrayToggle, WinCraftPlugin, WindowGroups,
};
use crate::core::ui_bridge::{
    ActionKind, ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeView, ArrangeWindow,
    LayoutAction, LayoutApp, LayoutEntry, LayoutRef, LayoutSnapshot,
};
use crate::core::{
    appid, clock, host, instance, session, taskbar, taskbar_scope, wide, window_names,
};
use crate::search::providers::apps;
use desktops::{Desktop, DesktopId};
use identity::Rect;
use order::{Handle, OrderModel};
use restore::Restore;
use state::{ProgramState, Programs, SavedMonitor, SavedWindow};

/// WindowNamer tells a browser's page title from its product name the same way.
pub use identity::PRODUCT_SUFFIXES;

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
const ACTION_ARRANGE: u32 = 5;

/// A tray toggle; tray ids share their space with the actions above.
const TRAY_HOVER_STRIP: u32 = 6;

const VK_OPEN_BRACKET: u32 = 0xDB;
const VK_CLOSE_BRACKET: u32 = 0xDD;
const COMMAND_OPEN_STATE: u32 = 1;
const COMMAND_HOVER_STRIP: u32 = 2;

const HOVER_STRIP_KEY: &str = "taskbar_hover_strip";
const HOVER_DELAY_KEY: &str = "hover_delay_seconds";
const HOVER_STRIP_LABEL: &str = "Arrange strip on taskbar hover";

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
    /// "Restore monitors": a restore moves windows back to their monitor.
    restore_monitors: bool,
    /// Windows the user put on a monitor in the strip since the last save,
    /// which therefore no longer belong on an unplugged one.
    released: BTreeSet<Handle>,
    preview_on_hover: bool,
    /// The strip's view, as the user last picked it there.
    arrange_view: ArrangeView,
    /// "Show the Arrange strip instead of Windows' taskbar preview".
    hover_strip: bool,
    hover_delay_ms: u64,
    hover: hover_preview::HoverPreview,
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
            preview_on_hover: true,
            arrange_view: ArrangeView::Auto,
            hover_strip: false,
            hover_delay_ms: hover_preview::DEFAULT_DELAY_MS,
            hover: hover_preview::HoverPreview::default(),
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
        self.preview_on_hover = flag(settings, "preview_on_hover");
        self.arrange_view = ArrangeView::from_setting(&text("arrange_view", "auto"));
        self.hover_strip = settings
            .get(HOVER_STRIP_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        self.hover_delay_ms =
            hover_preview::delay_ms(settings.get(HOVER_DELAY_KEY).and_then(|v| v.as_f64()));
        self.apply_hover();
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

    /// Starts or stops the hover strip to match the settings, at once.
    fn apply_hover(&mut self) {
        let commands = self
            .hover
            .set(self.host_hwnd, self.hover_strip, self.hover_delay_ms);
        run_hover(commands);
    }

    fn hover_tick(&mut self) {
        let groups = &self.groups;
        let commands = self
            .hover
            .tick(&|app, button| windows_on_taskbar(groups, app, button));
        run_hover(commands);
    }

    /// The tray toggle and the palette command: flips the switch now and
    /// saves it, as the switch on the settings page would.
    fn toggle_hover(&mut self) {
        self.hover_strip = !self.hover_strip;
        self.apply_hover();
        host::save_setting(
            "layout_keeper",
            HOVER_STRIP_KEY,
            serde_json::Value::Bool(self.hover_strip),
        );
    }

    /// The watched programs' app windows, top of the z-order first.
    fn live_windows(&self) -> Vec<windows::LiveWindow> {
        let registry = desktops::list();
        // Without the desktop reader a cloaked window cannot be placed, and
        // it is kept as it was before the reader existed.
        let on_known_desktop = |hwnd: HWND| match &self.reader {
            Some(reader) => reader
                .read(hwnd)
                .is_some_and(|id| known_desktop(id, &registry)),
            None => true,
        };
        windows::enumerate(&|exe| self.watch.covers(exe), &on_known_desktop)
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
    }

    /// Gives a group its name the first time one of its windows is seen.
    fn remember_label(&mut self, window: &windows::LiveWindow) {
        if self.labels.contains_key(&window.group) && !self.unresolved.contains(&window.group) {
            return;
        }
        let has_id = window.group != window.exe_path.to_lowercase();
        let name = window.app_name.clone().or_else(|| {
            has_id
                .then(|| appid::registered_name(&window.group))
                .flatten()
        });
        let exe = &window.identity.exe;
        let browser = browser_name(exe);
        let (name, resolved) = if browser.is_some() {
            (name, true)
        } else {
            with_start_menu_name(name, &window.group)
        };
        if resolved {
            self.unresolved.remove(&window.group);
        } else {
            self.unresolved.insert(window.group.clone());
        }
        let label = match browser {
            Some(browser) => {
                appid::group_label(browser, name.as_deref(), &window.group, &window.exe_path)
            }
            None => app_label(
                name.as_deref(),
                windows::file_description(&window.exe_path).as_deref(),
                exe,
            ),
        };
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
        let (took, refused) = taskbar.apply(&send);
        for (call, hr) in &refused {
            let (what, hwnd) = match call {
                order::TabCall::Delete(hwnd) => ("remove", *hwnd),
                order::TabCall::Add(hwnd) => ("add", *hwnd),
                order::TabCall::Activate(hwnd) => ("highlight", *hwnd),
            };
            log::warn!(
                "the taskbar refused to {what} the button of \"{}\" ({}), so it may keep its old place",
                window_names::display_title(hwnd as HWND),
                crate::core::com::hex(*hr)
            );
        }
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
                let device = &monitors[target].saved().device;
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
        let monitors = monitors::list();
        let Some(to) = monitors.iter().position(|m| m.saved().device == device) else {
            log::info!("monitor {device} is not connected; the window stays where it is");
            return;
        };
        let rect = self
            .refresh()
            .iter()
            .find(|window| window.hwnd == hwnd)
            .map(|window| window.identity.rect);
        let Some(from) = rect.and_then(|rect| monitors::index_of(rect, &monitors)) else {
            log::info!("the window to move to {device} is no longer open");
            return;
        };
        if from == to {
            return;
        }
        let label = window_names::display_title(hwnd);
        match monitors::move_window(hwnd, &monitors[from], &monitors[to], &monitors) {
            Ok(()) => log::info!("moved \"{label}\" to monitor {device}"),
            Err(err) => log::warn!("could not move \"{label}\" to monitor {device}: {err}"),
        }
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
                    monitors[to].saved().device
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
                        .map(|index| monitors[index].saved().device.clone()),
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

/// A readable name for a group key when nothing better is known: the program
/// file of a path key, else the last word of an app ID. Never the key whole.
fn friendly_key(key: &str) -> String {
    if key.contains('\\') {
        return program_name(&windows::file_name(key));
    }
    key.rsplit(['.', '!', '_'])
        .find(|part| !part.is_empty())
        .unwrap_or(key)
        .to_string()
}

/// The name an app gets when it gives none of its own: the name of its Start
/// menu entry or app list entry, found by app ID or program file in the apps
/// index the palette keeps. Without a match the label stays what the
/// program file says ("Windows Terminal Host"). The flag says whether the
/// answer is final: it is not while the index is still being built.
fn with_start_menu_name(name: Option<String>, group: &str) -> (Option<String>, bool) {
    match apps::published() {
        Some(index) => start_menu_name(name, group, Some(&|key: &str| apps::name_in(&index, key))),
        None => start_menu_name(name, group, None),
    }
}

type NameLookup<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// `lookup` is None while the index is not built yet.
fn start_menu_name(
    name: Option<String>,
    group: &str,
    lookup: Option<&NameLookup<'_>>,
) -> (Option<String>, bool) {
    if name.is_some() {
        return (name, true);
    }
    match lookup {
        Some(lookup) => (lookup(group), true),
        None => (None, false),
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
            .map(|monitor| monitor.saved().device.clone())
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

/// A number setting that still holds the old default was never a choice, so
/// it takes the new default. Any other number is kept.
fn migrate_default(settings: &mut serde_json::Value, key: &str, old: u64, new: u64) -> bool {
    if settings.get(key).and_then(|value| value.as_f64()) != Some(old as f64) {
        return false;
    }
    settings[key] = serde_json::Value::from(new);
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

/// A desktop in Task View, or "every desktop" for a window shown on all of
/// them, which counts as on this one too.
fn known_desktop(id: DesktopId, registry: &[Desktop]) -> bool {
    id == DesktopId::ALL || registry.iter().any(|desktop| desktop.id == id)
}

/// How many windows the taskbar button's app has, on every desktop.
fn windows_of(groups: &BTreeMap<String, OrderModel>, app: &str) -> usize {
    groups
        .iter()
        .filter(|(key, _)| taskbar::same_app(app, key))
        .map(|(_, model)| model.windows().count())
        .sum()
}

/// How many windows the taskbar button at `button` lists for the app. A
/// secondary taskbar may list only the windows on its own monitor (see
/// `taskbar_scope`); the main one lists them all, and so does a taskbar
/// whose monitor cannot be found.
fn windows_on_taskbar(groups: &BTreeMap<String, OrderModel>, app: &str, button: Rect) -> usize {
    let monitors = monitors::list();
    let Some(hovered) = monitors::index_of(button, &monitors) else {
        return windows_of(groups, app);
    };
    let own_only =
        taskbar_scope::MultiMonitor::read().shows_only_own(monitors[hovered].is_primary());
    if !own_only {
        return windows_of(groups, app);
    }
    let primary = monitors.iter().position(|m| m.is_primary()).unwrap_or(0);
    let on: Vec<usize> = groups
        .iter()
        .filter(|(key, _)| taskbar::same_app(app, key))
        .flat_map(|(_, model)| model.windows())
        .map(|(_, identity)| monitors::index_of(identity.rect, &monitors).unwrap_or(primary))
        .collect();
    taskbar_scope::count_shown(&on, hovered, true)
}

/// The palette entry's second line: what the switch is now.
fn hover_subtitle(on: bool) -> &'static str {
    if on {
        "Now on: apps with 2 or more windows show the Arrange strip"
    } else {
        "Now off: Windows' own preview shows"
    }
}

fn run_hover(commands: Vec<hover_preview::Command>) {
    for command in commands {
        match command {
            hover_preview::Command::Open { app, button } => host::open_arrange_at(app, button),
            hover_preview::Command::Close => host::close_arrange(),
            hover_preview::Command::Lookup(_) => {}
        }
    }
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
            description: "Remembers which desktop and monitor each window is on, the taskbar thumbnail order and which window is in front, and restores them after a reboot.",
            author: "community",
            version: "1.2.1",
            readme: include_str!("README.md"),
            category: "Windows",
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
            "preview_on_hover": true,
            "arrange_view": ArrangeView::Auto.setting(),
            HOVER_STRIP_KEY: false,
            HOVER_DELAY_KEY: hover_preview::DEFAULT_DELAY_MS as f64 / 1000.0,
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
            SettingField {
                key: "preview_on_hover",
                label: "Preview window on hover",
                help: "Resting the mouse on a picture in the Arrange strip shows that window on its monitor.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: HOVER_STRIP_KEY,
                label: "Show the Arrange strip instead of Windows' taskbar preview",
                help: "Resting the mouse on the taskbar button of an app with 2 or more windows opens the Arrange strip above it, where you can drag the windows into order. Apps with one window keep Windows' own preview. Win+number and Alt+Tab work as before.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: HOVER_DELAY_KEY,
                label: "Hover delay (seconds)",
                help: "How long the mouse rests on a taskbar button before the Arrange strip opens.",
                kind: FieldKind::Slider {
                    min: 0.2,
                    max: 1.5,
                    step: 0.1,
                },
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
        self.hover.may_hide = !self.read_only || host::allow_hide_popup();
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
        if unsafe { SetTimer(ctx.hwnd, TIMER_ID, TICK_MS, None) } == 0 {
            return Err("could not start its timer".to_string());
        }
        self.host_hwnd = Some(ctx.hwnd);
        log::info!("watching {}", self.watch);
        self.apply_hover();

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
            ACTION_RESTORE => self.start_restore(false),
            ACTION_SAVE_NOW => self.save_now(),
            ACTION_MOVE_LEFT => self.shift_front(-1),
            ACTION_MOVE_RIGHT => self.shift_front(1),
            ACTION_ARRANGE => host::arrange_hotkey(),
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
            ACTION_RESTORE => self.start_restore(false),
            ACTION_ARRANGE => host::open_arrange(),
            TRAY_HOVER_STRIP => self.toggle_hover(),
            _ => {}
        }
    }

    fn tray_toggles(&self) -> Vec<TrayToggle> {
        vec![TrayToggle {
            id: TRAY_HOVER_STRIP,
            label: HOVER_STRIP_LABEL,
            on: self.hover_strip,
        }]
    }

    fn page_action(&self) -> Option<u32> {
        Some(ACTION_ARRANGE)
    }

    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        if kind == ActionKind::Palette && action_id == COMMAND_HOVER_STRIP {
            return Some(hover_subtitle(self.hover_strip).to_string());
        }
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
                        let name = window_names::custom(hwnd);
                        ArrangeWindow {
                            hwnd,
                            renamed: name.is_some(),
                            label: name.unwrap_or_else(|| identity.label().to_string()),
                            desktop,
                            monitor: monitors::strip_label(identity.rect, &monitors),
                            on_monitor: monitors::index_of(identity.rect, &monitors),
                            saved_monitor: saved_monitor.get(&hwnd).copied(),
                            same_title: false,
                            attention: false,
                        }
                    })
                    .collect();
                let mut windows = windows;
                let titles: Vec<&str> = windows.iter().map(|w| w.label.as_str()).collect();
                let same = identity::same_titles(&titles);
                for (window, same) in windows.iter_mut().zip(same) {
                    window.same_title = same;
                }
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
            monitors: monitors::for_strip(&monitors, &disconnected),
            view: self.arrange_view,
        })
    }

    fn on_arrange_action(&mut self, action: &ArrangeAction) {
        let what = match action {
            ArrangeAction::Activate(_) => "switching to a window",
            ArrangeAction::Reorder { .. } => "reordering from the strip",
            ArrangeAction::MoveToDesktop { .. } => "moving a window to another desktop",
            ArrangeAction::MoveToMonitor { .. } => "moving a window to another monitor",
            ArrangeAction::Close(_) => "closing a window",
            // The host sends names to WindowNamer; they never come here.
            ArrangeAction::Rename { .. } | ArrangeAction::AskName(_) => return,
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
                unsafe { PostMessageW(*hwnd as HWND, WM_CLOSE, 0, 0) };
                self.arrange_stale = true;
            }
            ArrangeAction::Rename { .. } | ArrangeAction::AskName(_) => {}
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
        vec![
            PaletteCommand {
                id: COMMAND_OPEN_STATE,
                label: "Open state file",
                hint: "",
            },
            PaletteCommand {
                id: COMMAND_HOVER_STRIP,
                label: "Switch the Arrange strip on taskbar hover on or off",
                hint: "",
            },
        ]
    }

    fn on_palette_command(&mut self, id: u32) {
        match id {
            COMMAND_OPEN_STATE => self.open_state_file(),
            COMMAND_HOVER_STRIP => self.toggle_hover(),
            _ => {}
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
        if self.hover.is_on() {
            parts.push(if self.hover.may_hide {
                "Arrange strip on taskbar hover: on".to_string()
            } else {
                "Arrange strip on taskbar hover: on, but Windows' preview is left alone in this test instance".to_string()
            });
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
            WM_TIMER if wparam == hover_preview::TIMER_ID => self.hover_tick(),
            WM_QUERYENDSESSION | WM_ENDSESSION if closes_app => self.save("shutdown"),
            WM_QUERYENDSESSION => self.save_at_end("shutdown"),
            WM_ENDSESSION if wparam != 0 => self.save_at_end("shutdown"),
            _ => {}
        }
    }

    fn teardown(&mut self) {
        run_hover(self.hover.stop());
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
            seen: None,
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
        // The flag follows instance::is_test, which reads WINCRAFT_INSTANCE
        // from the shell; this test must pass whatever the shell has set.
        assert_eq!(LayoutKeeper::default().read_only, instance::is_test());
        let mut keeper = LayoutKeeper {
            read_only: instance::is_test_suffix(""),
            ..LayoutKeeper::default()
        };
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
        keeper.start_restore(false);
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
            announce: true,
            targets: Vec::new(),
            orders: Vec::new(),
        });
        assert_eq!(keeper.refused, 1);
        assert!(keeper.last_restore.is_none());
        assert!(keeper.checks.is_empty(), "nothing to check either");
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
            ArrangeAction::MoveToMonitor {
                hwnd,
                monitor: r"\\.\DISPLAY2".to_string(),
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
    fn only_the_old_restore_defaults_move_to_the_new_ones() {
        let mut old = serde_json::json!({
            "settle_seconds": 5,
            "restore_window_minutes": 3,
            "snapshot_interval_seconds": 5,
        });
        keeper_migrate(&mut old);
        assert_eq!(old["settle_seconds"], 3);
        assert_eq!(old["restore_window_minutes"], 30);
        // 15 was the default until windows were placed as they open.
        let mut before = serde_json::json!({ "settle_seconds": 15 });
        keeper_migrate(&mut before);
        assert_eq!(before["settle_seconds"], 3);
        assert_eq!(old["snapshot_interval_seconds"], 5);

        let mut own = serde_json::json!({ "settle_seconds": 8, "restore_window_minutes": 10 });
        keeper_migrate(&mut own);
        assert_eq!(own["settle_seconds"], 8);
        assert_eq!(own["restore_window_minutes"], 10);

        let mut new = LayoutKeeper::default().default_settings();
        keeper_migrate(&mut new);
        assert_eq!(new["settle_seconds"], 3);
        assert_eq!(new["restore_window_minutes"], 30);

        // A float written by the settings page counts the same as an integer.
        let mut float = serde_json::json!({ "settle_seconds": 5.0 });
        assert!(migrate_default(&mut float, "settle_seconds", 5, 15));
        assert_eq!(float["settle_seconds"], 15);
    }

    fn keeper_migrate(settings: &mut serde_json::Value) {
        LayoutKeeper::default().migrate_settings(settings);
    }

    #[test]
    fn the_restore_runs_with_the_new_defaults() {
        let keeper = LayoutKeeper::default();
        assert_eq!(keeper.restore.settle_seconds, 3);
        assert_eq!(keeper.restore.give_up_seconds, 30 * 60);
        let mut applied = LayoutKeeper::default();
        applied.apply_settings(&keeper.default_settings());
        assert_eq!(applied.restore.settle_seconds, 3);
        assert_eq!(applied.restore.give_up_seconds, 30 * 60);
    }

    #[test]
    fn monitors_are_restored_and_the_strip_view_is_automatic_by_default() {
        let mut keeper = LayoutKeeper {
            restore_monitors: false,
            ..LayoutKeeper::default()
        };
        keeper.apply_settings(&LayoutKeeper::default().default_settings());
        assert!(keeper.restore_monitors);
        assert_eq!(keeper.arrange_view, ArrangeView::Auto);
        keeper.apply_settings(&serde_json::json!({
            "restore_monitors": false,
            "arrange_view": "flat",
        }));
        assert!(!keeper.restore_monitors);
        assert_eq!(keeper.arrange_view, ArrangeView::Flat);
    }

    #[test]
    fn the_hover_strip_is_off_by_default_and_switches_at_once() {
        let mut keeper = read_only();
        keeper.apply_settings(&LayoutKeeper::default().default_settings());
        assert!(!keeper.hover_strip);
        assert!(!keeper.hover.is_on());
        assert_eq!(keeper.hover_delay_ms, 400);

        keeper.apply_settings(&serde_json::json!({
            HOVER_STRIP_KEY: true,
            HOVER_DELAY_KEY: 0.8,
        }));
        assert!(keeper.hover.is_on(), "on without a restart");
        assert_eq!(keeper.hover_delay_ms, 800);
        assert!(keeper.tray_toggles()[0].on);

        keeper.apply_settings(&serde_json::json!({ HOVER_STRIP_KEY: false }));
        assert!(!keeper.hover.is_on(), "off at once");
        assert!(!keeper.tray_toggles()[0].on);
    }

    #[test]
    fn the_tray_and_the_palette_flip_the_hover_strip() {
        let mut keeper = read_only();
        keeper.on_tray_action(TRAY_HOVER_STRIP);
        assert!(keeper.hover_strip);
        assert!(keeper.hover.is_on());
        assert_eq!(
            keeper.palette_subtitle(ActionKind::Palette, COMMAND_HOVER_STRIP),
            Some(hover_subtitle(true).to_string())
        );
        keeper.on_palette_command(COMMAND_HOVER_STRIP);
        assert!(!keeper.hover_strip);
        assert!(!keeper.hover.is_on());
        assert_eq!(
            keeper.palette_subtitle(ActionKind::Palette, COMMAND_HOVER_STRIP),
            Some(hover_subtitle(false).to_string())
        );
        let toggles = keeper.tray_toggles();
        assert_eq!(toggles.len(), 1);
        assert_eq!(toggles[0].label, HOVER_STRIP_LABEL);
        assert!(toggles[0].id < 100, "tray ids stay below 100");
    }

    #[test]
    fn a_test_instance_says_it_leaves_windows_preview_alone() {
        let mut keeper = read_only();
        keeper.apply_settings(&serde_json::json!({ HOVER_STRIP_KEY: true }));
        let status = keeper.status().unwrap_or_default();
        assert!(
            status.contains("Windows' preview is left alone in this test instance"),
            "{status}"
        );
        assert!(!keeper.hover.may_hide);
    }

    #[test]
    fn an_app_counts_its_windows_on_every_desktop() {
        let identity = |title: &str| identity::WindowIdentity {
            exe: "chrome.exe".to_string(),
            name: None,
            title: title.to_string(),
            rect: [0, 0, 800, 600],
            maximized: false,
            group: "Chrome.UserData.Profile3".to_string(),
        };
        let mut groups = BTreeMap::new();
        groups.insert(
            "Chrome.UserData.Profile3".to_string(),
            OrderModel::from_matched(vec![
                (identity("one"), Some(1)),
                (identity("two"), Some(2)),
                (identity("closed"), None),
            ]),
        );
        groups.insert(
            "Telegram.TelegramDesktop".to_string(),
            OrderModel::from_matched(vec![(identity("chat"), Some(3))]),
        );
        assert_eq!(windows_of(&groups, "Chrome.UserData.Profile3"), 2);
        assert_eq!(windows_of(&groups, "Appid: chrome.userdata.profile3"), 2);
        assert_eq!(windows_of(&groups, "Telegram.TelegramDesktop"), 1);
        assert_eq!(windows_of(&groups, "md.obsidian"), 0);
    }

    #[test]
    fn a_store_app_shown_on_all_desktops_counts_all_its_windows() {
        let claude = r"d:\windowsapps\claude_3.6556.2.0_x64__pzs8sxrjxfjjc\app\claude.exe";
        let identity = |title: &str| identity::WindowIdentity {
            exe: claude.to_string(),
            name: None,
            title: title.to_string(),
            rect: [0, 0, 800, 600],
            maximized: false,
            group: claude.to_string(),
        };
        let mut groups = BTreeMap::new();
        groups.insert(
            claude.to_string(),
            OrderModel::from_matched(
                (1..=10)
                    .map(|n| (identity(&format!("chat {n}")), Some(n)))
                    .collect(),
            ),
        );
        assert_eq!(windows_of(&groups, "Claude_pzs8sxrjxfjjc!Claude"), 10);
        assert_eq!(
            windows_of(&groups, "Appid: Claude_pzs8sxrjxfjjc!Claude"),
            10
        );

        let registry = vec![Desktop {
            id: DesktopId::parse("{11111111-2222-3333-4444-555555555555}").expect("a GUID"),
            name: "Desktop 1".to_string(),
        }];
        assert!(known_desktop(DesktopId::ALL, &registry), "all desktops");
        assert!(known_desktop(registry[0].id, &registry));
        let gone = DesktopId::parse("{99999999-2222-3333-4444-555555555555}").expect("a GUID");
        assert!(!known_desktop(gone, &[]));
    }

    fn saved_window(title: &str, group: &str, desktop: &str, seen: Option<&str>) -> SavedWindow {
        SavedWindow {
            title: title.to_string(),
            name: None,
            desktop: Some(desktop.to_string()),
            seen: seen.map(str::to_string),
            ..saved(group)
        }
    }

    const DESKTOP_A: &str = "{11111111-1111-1111-1111-111111111111}";
    const DESKTOP_B: &str = "{22222222-2222-2222-2222-222222222222}";

    fn layout_file() -> Programs {
        let mut programs = Programs::new();
        programs.insert(
            "chrome.exe".to_string(),
            ProgramState {
                windows: vec![
                    saved_window(
                        "Inbox - Google Chrome",
                        "Chrome",
                        DESKTOP_A,
                        Some("2026-10-12"),
                    ),
                    saved_window("Docs - Google Chrome", "Chrome", DESKTOP_B, None),
                ],
            },
        );
        programs.insert(
            "viber.exe".to_string(),
            ProgramState {
                windows: vec![saved_window("Viber", "Viber.Desktop", DESKTOP_A, None)],
            },
        );
        programs
    }

    /// A keeper whose saved layout lives in its own scratch file.
    fn keeper_with(programs: Programs, name: &str) -> (LayoutKeeper, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "wincraft-test-layout-{name}-{}.json",
            std::process::id()
        ));
        let mut keeper = LayoutKeeper {
            watch: Watch::new("*", ""),
            ..LayoutKeeper::default()
        };
        keeper.writer = state::Writer::new(path.clone(), Some(programs));
        (keeper, path)
    }

    fn entry_of(keeper: &mut LayoutKeeper, title: &str) -> LayoutRef {
        let snapshot = keeper.layout_snapshot().unwrap();
        snapshot
            .apps
            .iter()
            .flat_map(|app| &app.windows)
            .find(|window| window.entry.title == title)
            .map(|window| window.entry.clone())
            .unwrap()
    }

    #[test]
    fn the_layout_page_lists_saved_windows_by_app_with_their_state() {
        let (mut keeper, _) = keeper_with(layout_file(), "list");
        let snapshot = keeper.layout_snapshot().unwrap();
        // The busiest app comes first.
        assert_eq!(snapshot.apps.len(), 2);
        assert_eq!(snapshot.apps[0].windows.len(), 2);
        assert_eq!(snapshot.apps[1].windows.len(), 1);
        let inbox = &snapshot.apps[0].windows[0];
        assert_eq!(inbox.title, "Inbox - Google Chrome");
        assert_eq!(inbox.hwnd, None, "no such window is open");
        assert_eq!(inbox.closed_since.as_deref(), Some("2026-10-12"));
        assert_eq!(snapshot.apps[0].windows[1].closed_since, None);
        // The desktops in the file are not the PC's own, so they count as gone.
        assert!(inbox.desktop_removed);
        assert_eq!(inbox.desktop, DESKTOP_A);
    }

    #[test]
    fn a_saved_window_is_matched_to_the_open_one_it_names() {
        let program = ProgramState {
            windows: vec![saved_window("Docs", "Chrome", DESKTOP_A, None)],
        };
        assert_eq!(
            LayoutKeeper::open_matches("chrome.exe", &program, &[]),
            [None]
        );
        let live = windows::LiveWindow {
            hwnd: 4242 as HWND,
            identity: identity::WindowIdentity::new("chrome.exe", "Docs", [0, 0, 800, 600], false)
                .in_group("Chrome"),
            group: "Chrome".to_string(),
            app_name: None,
            exe_path: r"c:\chrome.exe".to_string(),
        };
        assert_eq!(
            LayoutKeeper::open_matches("chrome.exe", &program, &[live]),
            [Some(4242)]
        );
    }

    #[test]
    fn a_new_desktop_is_saved_in_the_file() {
        let (mut keeper, path) = keeper_with(layout_file(), "desktop");
        let entry = entry_of(&mut keeper, "Docs - Google Chrome");
        // Desktop ids need to exist on this PC to be saved, so use the
        // saved id the file already has: the write itself is what is tested.
        keeper.on_layout_action(&LayoutAction::SetDesktop {
            entry: entry.clone(),
            desktop: DESKTOP_A.to_string(),
            hwnd: None,
        });
        let text = std::fs::read_to_string(&path).unwrap();
        let file: state::StateFile = serde_json::from_str(&text).unwrap();
        let docs = &file.programs["chrome.exe"].windows[1];
        assert_eq!(docs.desktop.as_deref(), Some(DESKTOP_A));
        assert_eq!(file.reason, "layout page");
        assert_eq!(keeper.writer.last(), Some(&file.programs));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_edit_from_a_stale_list_is_ignored() {
        let (mut keeper, path) = keeper_with(layout_file(), "stale");
        let mut entry = entry_of(&mut keeper, "Docs - Google Chrome");
        entry.title = "Something else".to_string();
        keeper.on_layout_action(&LayoutAction::Forget(entry.clone()));
        keeper.on_layout_action(&LayoutAction::SetDesktop {
            entry,
            desktop: DESKTOP_A.to_string(),
            hwnd: None,
        });
        assert!(!path.exists(), "nothing was written");
        assert_eq!(keeper.writer.last(), Some(&layout_file()));
    }

    #[test]
    fn a_closed_window_can_be_forgotten_and_an_empty_app_goes_with_it() {
        let (mut keeper, path) = keeper_with(layout_file(), "forget");
        let docs = entry_of(&mut keeper, "Docs - Google Chrome");
        keeper.on_layout_action(&LayoutAction::Forget(docs));
        let text = std::fs::read_to_string(&path).unwrap();
        let file: state::StateFile = serde_json::from_str(&text).unwrap();
        assert_eq!(file.programs["chrome.exe"].windows.len(), 1);
        assert_eq!(
            file.programs["chrome.exe"].windows[0].title,
            "Inbox - Google Chrome"
        );

        let viber = entry_of(&mut keeper, "Viber");
        keeper.on_layout_action(&LayoutAction::Forget(viber));
        assert!(!keeper.writer.last().unwrap().contains_key("viber.exe"));
        // A forgotten window is no longer waited for by a restore.
        assert_eq!(keeper.saved_groups(), ["Chrome"]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_seen_day_round_trips_and_older_files_have_none() {
        let window = saved_window("A", "G", DESKTOP_A, Some("2026-10-17"));
        let text = serde_json::to_string(&window).unwrap();
        assert!(text.contains(r#""seen":"2026-10-17""#), "{text}");
        let back: SavedWindow = serde_json::from_str(&text).unwrap();
        assert_eq!(back, window);
        let none = serde_json::to_string(&saved_window("A", "G", DESKTOP_A, None)).unwrap();
        assert!(!none.contains("seen"), "{none}");
    }

    #[test]
    fn a_saved_group_without_a_live_window_still_gets_a_name() {
        let (mut keeper, _) = keeper_with(Programs::new(), "label");
        assert_eq!(
            keeper.saved_label("chrome.exe", "Chrome.UserData.Profile3"),
            "Chrome \u{b7} Profile3"
        );
        assert_eq!(
            keeper.saved_label("charmap.exe", r"c:\windows\charmap.exe"),
            "Charmap"
        );
    }

    fn live_window(hwnd: isize, title: &str, group: &str) -> windows::LiveWindow {
        windows::LiveWindow {
            hwnd: hwnd as HWND,
            identity: identity::WindowIdentity::new("app.exe", title, [0, 0, 1, 1], false)
                .in_group(group),
            group: group.to_string(),
            app_name: None,
            exe_path: r"c:\app.exe".to_string(),
        }
    }

    #[test]
    fn saved_windows_share_a_title_only_inside_one_app() {
        let mut programs = Programs::new();
        let windows = |titles: &[&str], group: &str| ProgramState {
            windows: titles
                .iter()
                .map(|title| saved_window(title, group, DESKTOP_A, None))
                .collect(),
        };
        programs.insert(
            "browser.exe".to_string(),
            windows(&["Inbox", "inbox ", "Docs"], "Browser"),
        );
        // The same word in another app is not a clash.
        programs.insert("chat.exe".to_string(), windows(&["Inbox"], "Chat"));
        let (mut keeper, _) = keeper_with(programs, "same-title");
        let snapshot = keeper.layout_snapshot().unwrap();
        let flags = |label: &str| -> Vec<(String, bool)> {
            snapshot
                .apps
                .iter()
                .find(|app| app.label == label)
                .unwrap()
                .windows
                .iter()
                .map(|window| (window.title.clone(), window.same_title))
                .collect()
        };
        assert_eq!(
            flags("Browser"),
            [
                ("Inbox".to_string(), true),
                ("inbox ".to_string(), true),
                ("Docs".to_string(), false)
            ]
        );
        assert_eq!(flags("Chat"), [("Inbox".to_string(), false)]);
    }

    #[test]
    fn shared_titles_are_noted_when_the_set_changes_and_not_before() {
        let (mut keeper, _) = keeper_with(Programs::new(), "noted");
        let pair = [
            live_window(1, "New Tab", "Browser"),
            live_window(2, "new tab", "Browser"),
            live_window(3, "New Tab", "Other"),
        ];
        keeper.note_shared_titles(&pair);
        let key = ("Browser".to_string(), "new tab".to_string());
        assert_eq!(keeper.shared_titles.len(), 1);
        assert_eq!(keeper.shared_titles[&key], 2);

        // The same windows again change nothing; a third one changes the count.
        keeper.note_shared_titles(&pair);
        assert_eq!(keeper.shared_titles[&key], 2);
        let three = [
            live_window(1, "New Tab", "Browser"),
            live_window(2, "new tab", "Browser"),
            live_window(4, "New tab", "Browser"),
        ];
        keeper.note_shared_titles(&three);
        assert_eq!(keeper.shared_titles[&key], 3);

        // Giving one a different name ends it.
        let named = [
            live_window(1, "New Tab", "Browser"),
            live_window(2, "Work", "Browser"),
        ];
        keeper.note_shared_titles(&named);
        assert!(keeper.shared_titles.is_empty());
    }

    #[test]
    fn the_progress_panel_follows_its_setting() {
        let mut keeper = LayoutKeeper::default();
        assert!(keeper.show_progress);
        keeper.apply_settings(&serde_json::json!({ "show_restore_progress": false }));
        assert!(!keeper.show_progress);
        keeper.apply_settings(&LayoutKeeper::default().default_settings());
        assert!(keeper.show_progress);
    }

    #[test]
    fn a_group_without_a_window_still_has_a_readable_name() {
        let mut programs = Programs::new();
        programs.insert(
            "claude.exe".to_string(),
            ProgramState {
                windows: vec![saved_window(
                    "Claude",
                    r"d:\windowsapps\claude_2.4_x64__pzs8sxrjxfjjc\app\claude.exe",
                    DESKTOP_A,
                    None,
                )],
            },
        );
        programs.insert(
            "chrome.exe".to_string(),
            ProgramState {
                windows: vec![saved_window(
                    "Inbox",
                    "Chrome.UserData.Profile3",
                    DESKTOP_A,
                    None,
                )],
            },
        );
        let (mut keeper, _) = keeper_with(programs, "names");
        assert_eq!(
            keeper.group_name(r"d:\windowsapps\claude_2.4_x64__pzs8sxrjxfjjc\app\claude.exe"),
            "Claude"
        );
        assert_eq!(
            keeper.group_name("Chrome.UserData.Profile3"),
            "Chrome \u{b7} Profile3"
        );
        // A group the layout does not know is still not shown as a path.
        assert_eq!(
            keeper.group_name(r"c:\program files\windowsapps\x\windowsterminal.exe"),
            "Windowsterminal"
        );
        assert_eq!(
            keeper.group_name("Telegram.TelegramDesktop"),
            "TelegramDesktop"
        );
        assert_eq!(friendly_key("Pkg_8wekyb!App"), "App");
    }

    #[test]
    fn a_label_seen_on_a_live_window_is_used_for_the_panel() {
        let (mut keeper, _) = keeper_with(Programs::new(), "seen");
        let mut window = live_window(7, "Docs", "Chrome.UserData.Profile3");
        window.identity.exe = "chrome.exe".to_string();
        window.app_name = Some("Dmytro (Oliver) - Chrome".to_string());
        keeper.remember_label(&window);
        assert_eq!(
            keeper.group_name("Chrome.UserData.Profile3"),
            "Chrome \u{b7} Dmytro (Oliver)"
        );
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

    #[test]
    fn an_app_that_gives_no_name_gets_its_start_menu_name() {
        let lookup = |key: &str| match key {
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App" => Some("Terminal".to_string()),
            "Com.Codewulf.WulfScreen" => Some("WulfScreen".to_string()),
            _ => None,
        };
        let (name, done) = start_menu_name(
            None,
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            Some(&lookup),
        );
        assert!(done);
        // The Start menu name beats the program file's description.
        assert_eq!(
            app_label(
                name.as_deref(),
                Some("Windows Terminal Host"),
                "openconsole.exe"
            ),
            "Terminal"
        );
        let (name, _) = start_menu_name(None, "Com.Codewulf.WulfScreen", Some(&lookup));
        assert_eq!(
            app_label(name.as_deref(), Some("Com.Codewulf.WulfScreen"), "wulf.exe"),
            "WulfScreen"
        );
    }

    #[test]
    fn a_name_the_app_gave_itself_is_kept() {
        let lookup = |_: &str| Some("From the index".to_string());
        let (name, done) = start_menu_name(Some("Mine".to_string()), "any", Some(&lookup));
        assert_eq!((name.as_deref(), done), (Some("Mine"), true));
        // Not even asked while the index is missing.
        assert_eq!(
            start_menu_name(Some("Mine".to_string()), "any", None),
            (Some("Mine".to_string()), true)
        );
    }

    #[test]
    fn with_no_match_the_old_label_stays_and_a_missing_index_is_not_final() {
        let none = |_: &str| None;
        let (name, done) = start_menu_name(None, "Some.App", Some(&none));
        assert_eq!(
            (name.clone(), done),
            (None, true),
            "an answer: there is none"
        );
        assert_eq!(
            app_label(name.as_deref(), Some("Windows Terminal Host"), "x.exe"),
            "Windows Terminal Host"
        );
        // The index is still being built: the label is made again later.
        assert_eq!(start_menu_name(None, "Some.App", None), (None, false));
    }

    #[test]
    fn a_label_made_before_the_index_was_ready_is_made_again_when_it_is() {
        let (mut keeper, _) = keeper_with(Programs::new(), "unresolved");
        let mut window = live_window(7, "Host", "Some.Unlisted.App");
        window.identity.exe = "unlisted.exe".to_string();
        keeper.remember_label(&window);
        // Nothing builds the index in a test, so it is not final.
        assert!(keeper.unresolved.contains("Some.Unlisted.App"));
        assert!(keeper.labels.contains_key("Some.Unlisted.App"));
    }

    // Saves and restores below run over fake windows, desktops and monitors.
    // Nothing in them reaches a real window: the handles are made up and the
    // only writes go to scratch files.

    const D1: DesktopId = DesktopId(1);
    const D2: DesktopId = DesktopId(2);
    /// A desktop the PC no longer has.
    const D9: DesktopId = DesktopId(9);

    fn two_desktops() -> Vec<Desktop> {
        vec![
            Desktop {
                id: D1,
                name: "Desktop 1".to_string(),
            },
            Desktop {
                id: D2,
                name: "Work".to_string(),
            },
        ]
    }

    fn screen(device: &str, name: &str, position: usize) -> SavedMonitor {
        SavedMonitor {
            device: format!(r"\\.\{device}"),
            name: name.to_string(),
            position,
        }
    }

    /// DELL on the left, LG right of it at x 1920.
    fn two_monitors() -> Vec<SavedMonitor> {
        vec![screen("DISPLAY1", "DELL", 0), screen("DISPLAY2", "LG", 1)]
    }

    fn by_left_edge(rect: Rect) -> Option<usize> {
        Some(usize::from(rect[0] >= 1920))
    }

    fn open(hwnd: isize, exe: &str, title: &str, group: &str, rect: Rect) -> windows::LiveWindow {
        windows::LiveWindow {
            hwnd: hwnd as HWND,
            identity: identity::WindowIdentity::new(exe, title, rect, false).in_group(group),
            group: group.to_string(),
            app_name: None,
            exe_path: format!(r"c:\apps\{exe}"),
        }
    }

    fn chrome(hwnd: isize, title: &str, left: i32) -> windows::LiveWindow {
        open(
            hwnd,
            "chrome.exe",
            title,
            "Chrome",
            [left, 10 * hwnd as i32, left + 700, 500],
        )
    }

    fn kept(
        title: &str,
        desktop: Option<DesktopId>,
        monitor: Option<SavedMonitor>,
        place: usize,
    ) -> SavedWindow {
        SavedWindow {
            name: Some(title.to_string()),
            title: title.to_string(),
            rect: [5000 + place as i32, 0, 5600, 400],
            desktop: desktop.map(|id| id.to_string()),
            monitor,
            taskbar_index: place,
            z_index: place,
            ..saved("Chrome")
        }
    }

    fn programs(entries: Vec<(&str, Vec<SavedWindow>)>) -> Programs {
        entries
            .into_iter()
            .map(|(exe, windows)| (exe.to_string(), ProgramState { windows }))
            .collect()
    }

    fn read_file(path: &std::path::Path) -> state::StateFile {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn titles(file: &state::StateFile, exe: &str) -> Vec<String> {
        file.programs[exe]
            .windows
            .iter()
            .map(|window| window.title.clone())
            .collect()
    }

    /// The titles whatever their taskbar order, which a restart scrambles.
    fn title_set(path: &std::path::Path, exe: &str) -> BTreeSet<String> {
        titles(&read_file(path), exe).into_iter().collect()
    }

    fn set(titles: &[&str]) -> BTreeSet<String> {
        titles.iter().map(|title| title.to_string()).collect()
    }

    /// Saves `live` the way a timer or a hotkey would, against two desktops
    /// and two monitors, with window 1 on the Work desktop.
    fn save_seen(
        keeper: &mut LayoutKeeper,
        live: &[windows::LiveWindow],
        reason: &str,
    ) -> (bool, usize) {
        let desktops = two_desktops();
        let monitors = two_monitors();
        let world = World {
            desktops: &desktops,
            monitors: &monitors,
            desktop_of: &|hwnd| (hwnd as isize == 1).then_some(D2),
            monitor_of: &by_left_edge,
            today: Some("2026-10-18".to_string()),
        };
        let force = reason == "manual";
        keeper.write_seen(live, &world, reason, force).unwrap()
    }

    fn clean_up(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("prev.json"));
        let _ = std::fs::remove_file(state::snapshot_path(path));
    }

    #[test]
    fn a_save_writes_each_window_with_its_desktop_monitor_order_and_day() {
        let (mut keeper, path) = keeper_with(Programs::new(), "save-fresh");
        let live = [
            chrome(1, "Inbox", 2000),
            chrome(2, "Docs", 0),
            open(3, "viber.exe", "Viber", "Viber.Desktop", [0, 0, 400, 400]),
        ];
        assert_eq!(save_seen(&mut keeper, &live, "manual"), (true, 3));
        let file = read_file(&path);
        assert_eq!(file.reason, "manual");
        // A group seen for the first time is in taskbar order bottom first.
        assert_eq!(titles(&file, "chrome.exe"), ["Docs", "Inbox"]);
        let docs = &file.programs["chrome.exe"].windows[0];
        let inbox = &file.programs["chrome.exe"].windows[1];
        assert_eq!(inbox.desktop, Some(D2.to_string()));
        assert_eq!(inbox.desktop_name.as_deref(), Some("Work"));
        assert_eq!(inbox.monitor, Some(screen("DISPLAY2", "LG", 1)));
        assert_eq!((inbox.z_index, docs.z_index), (0, 1));
        assert_eq!(docs.desktop, None);
        assert_eq!(docs.monitor, Some(screen("DISPLAY1", "DELL", 0)));
        assert_eq!(docs.seen.as_deref(), Some("2026-10-18"));
        assert_eq!(file.programs["viber.exe"].windows[0].z_index, 2);

        // The same windows again: a timer save writes nothing.
        assert_eq!(save_seen(&mut keeper, &live, "timer"), (false, 3));
        clean_up(&path);
    }

    #[test]
    fn a_save_keeps_closed_apps_and_drops_the_ones_no_longer_watched() {
        let before = programs(vec![
            ("viber.exe", vec![kept("Viber", None, None, 0)]),
            ("telegram.exe", vec![kept("Telegram", None, None, 0)]),
        ]);
        let (mut keeper, path) = keeper_with(before, "save-closed");
        keeper.watch = Watch::new("*", "telegram.exe");
        save_seen(&mut keeper, &[chrome(2, "Docs", 0)], "timer");
        let file = read_file(&path);
        assert_eq!(
            file.programs.keys().collect::<Vec<_>>(),
            ["chrome.exe", "viber.exe"]
        );
        assert_eq!(titles(&file, "viber.exe"), ["Viber"]);
        clean_up(&path);
    }

    #[test]
    fn a_window_saved_on_an_unplugged_monitor_keeps_it_until_moved_in_the_strip() {
        let samsung = screen("DISPLAY3", "SAMSUNG", 2);
        let before = programs(vec![(
            "chrome.exe",
            vec![kept("Mail", None, Some(samsung.clone()), 0)],
        )]);
        let (mut keeper, path) = keeper_with(before, "save-unplugged");
        let live = [chrome(2, "Mail", 0)];
        save_seen(&mut keeper, &live, "manual");
        let mail = &read_file(&path).programs["chrome.exe"].windows[0];
        assert_eq!(mail.monitor, Some(samsung));

        keeper.released.insert(2);
        save_seen(&mut keeper, &live, "manual");
        let mail = &read_file(&path).programs["chrome.exe"].windows[0];
        assert_eq!(mail.monitor, Some(screen("DISPLAY1", "DELL", 0)));
        assert!(keeper.released.is_empty(), "a save uses the release up");
        clean_up(&path);
    }

    fn four_tabs() -> Vec<windows::LiveWindow> {
        ["A", "B", "C", "D"]
            .iter()
            .enumerate()
            .map(|(index, title)| chrome(index as isize + 1, title, 0))
            .collect()
    }

    /// Chrome restarted: two new windows in place of the four saved ones.
    fn after_restart() -> Vec<windows::LiveWindow> {
        vec![chrome(11, "X", 0), chrome(12, "Y", 0)]
    }

    #[test]
    fn a_crash_burst_keeps_the_saved_windows_and_a_manual_save_copies_the_file_first() {
        let (mut keeper, path) = keeper_with(Programs::new(), "save-burst");
        let prev = path.with_extension("prev.json");
        save_seen(&mut keeper, &four_tabs(), "timer");

        keeper.ticks = 30;
        assert_eq!(
            save_seen(&mut keeper, &after_restart(), "timer"),
            (false, 4)
        );
        assert_eq!(title_set(&path, "chrome.exe"), set(&["A", "B", "C", "D"]));
        assert!(!prev.exists(), "nothing was written over yet");

        assert_eq!(
            save_seen(&mut keeper, &after_restart(), "manual"),
            (true, 2)
        );
        assert_eq!(title_set(&path, "chrome.exe"), set(&["X", "Y"]));
        assert_eq!(title_set(&prev, "chrome.exe"), set(&["A", "B", "C", "D"]));

        // The copy was made once; later writes leave it alone.
        std::fs::remove_file(&prev).unwrap();
        keeper.ticks = 90;
        let more = [chrome(11, "X", 0), chrome(12, "Y", 0), chrome(13, "Z", 0)];
        assert_eq!(save_seen(&mut keeper, &more, "manual"), (true, 3));
        assert!(!prev.exists());
        clean_up(&path);
    }

    #[test]
    fn the_first_timer_save_after_the_hold_ends_copies_the_file_first() {
        let (mut keeper, path) = keeper_with(Programs::new(), "save-release");
        let prev = path.with_extension("prev.json");
        save_seen(&mut keeper, &four_tabs(), "timer");
        keeper.ticks = 30;
        save_seen(&mut keeper, &after_restart(), "timer");
        keeper.ticks = 45;
        assert_eq!(
            save_seen(&mut keeper, &after_restart(), "timer"),
            (false, 4)
        );
        assert!(!prev.exists());

        keeper.ticks = 60;
        assert_eq!(save_seen(&mut keeper, &after_restart(), "timer"), (true, 2));
        assert_eq!(title_set(&prev, "chrome.exe"), set(&["A", "B", "C", "D"]));
        assert_eq!(title_set(&path, "chrome.exe"), set(&["X", "Y"]));
        clean_up(&path);
    }

    #[test]
    fn groups_still_waiting_for_their_restore_keep_their_saved_windows_in_any_save() {
        let before = programs(vec![(
            "chrome.exe",
            vec![kept("A", None, None, 0), kept("B", None, None, 1)],
        )]);
        let (mut keeper, path) = keeper_with(before, "save-pending");
        keeper.restore.start(0, vec!["Chrome".to_string()]);
        let live = [chrome(1, "B", 0), chrome(2, "A", 0), chrome(3, "New", 0)];
        assert_eq!(save_seen(&mut keeper, &live, "manual"), (true, 2));
        assert_eq!(titles(&read_file(&path), "chrome.exe"), ["A", "B"]);
        clean_up(&path);
    }

    #[test]
    fn a_group_still_being_checked_after_its_restore_keeps_its_saved_windows() {
        let before = programs(vec![(
            "chrome.exe",
            vec![kept("A", None, None, 0), kept("B", None, None, 1)],
        )]);
        let (mut keeper, path) = keeper_with(before, "save-checking");
        keeper.checks.push(check_of(vec![verify::Target {
            hwnd: 1,
            group: "Chrome".to_string(),
            label: "B".to_string(),
            desktop: Some(D1),
            monitor: None,
        }]));
        let live = [chrome(1, "B", 0), chrome(3, "New", 0)];
        assert_eq!(
            save_seen(&mut keeper, &live, "timer"),
            (false, 2),
            "the saved A and B stay as they were"
        );
        keeper.checks.clear();
        assert_eq!(save_seen(&mut keeper, &live, "timer"), (true, 2));
        assert_eq!(set(&["B", "New"]), title_set(&path, "chrome.exe"));
        clean_up(&path);
    }

    #[test]
    fn a_save_plan_copies_aside_only_when_asked_and_something_changes() {
        let watch = Watch::new("*", "");
        let mut guard = guard::Guard::default();
        let timing = guard::Timing {
            stable_seconds: 30,
            refill_seconds: 180,
        };
        let look = |handles: &[Handle]| BTreeMap::from([("Chrome".to_string(), handles.to_vec())]);
        guard.observe(0, &look(&[1, 2, 3, 4]), timing);
        guard.observe(30, &look(&[]), timing);
        let saved = programs(vec![("chrome.exe", vec![kept("A", None, None, 0)])]);

        let same = plan_save(
            Some(&saved),
            saved.clone(),
            "manual",
            &watch,
            &guard,
            vec![],
        );
        assert!(same.backup && !same.copy_aside, "nothing would be lost");

        let other = programs(vec![("chrome.exe", vec![kept("B", None, None, 0)])]);
        let manual = plan_save(
            Some(&saved),
            other.clone(),
            "manual",
            &watch,
            &guard,
            vec![],
        );
        assert!(manual.backup && manual.copy_aside);
        assert_eq!(manual.programs, other);

        let timer = plan_save(Some(&saved), other, "timer", &watch, &guard, vec![]);
        assert!(!timer.backup && !timer.copy_aside);
        assert_eq!(timer.programs, saved, "the held group keeps its windows");
        assert_eq!(timer.count, 1);
    }

    fn due(group: &str, pass: restore::Pass) -> Vec<restore::Due> {
        vec![restore::Due {
            group: group.to_string(),
            pass,
        }]
    }

    /// Docs on desktop 1 of the left monitor, Inbox on Work on the right one,
    /// Old on a desktop that is gone, Lost no longer open.
    fn saved_chrome() -> ProgramState {
        let dell = screen("DISPLAY1", "DELL", 0);
        let lg = screen("DISPLAY2", "LG", 1);
        ProgramState {
            windows: vec![
                kept("Inbox", Some(D2), Some(lg), 1),
                kept("Docs", Some(D1), Some(dell.clone()), 0),
                kept("Old", Some(D9), Some(dell), 2),
                kept("Lost", Some(D1), None, 3),
            ],
        }
    }

    /// Every window was pulled to desktop 1 on the left monitor.
    fn reopened() -> Vec<windows::LiveWindow> {
        vec![
            chrome(10, "Inbox", 0),
            chrome(11, "Docs", 0),
            chrome(12, "Old", 0),
            open(20, "viber.exe", "Viber", "Viber.Desktop", [0, 0, 300, 300]),
        ]
    }

    struct Plan {
        plans: Vec<GroupPlan>,
        groups: BTreeMap<String, OrderModel>,
        placed: BTreeSet<Handle>,
        stack: Vec<(usize, HWND)>,
    }

    fn plan_for(
        program: &ProgramState,
        live: &[windows::LiveWindow],
        due: &[restore::Due],
        placed: BTreeSet<Handle>,
        can_move: bool,
        restore_monitors: bool,
    ) -> Plan {
        let view = ExeView::new("chrome.exe", program, live, &|_| Some(D1), &by_left_edge);
        let monitors = two_monitors();
        let cx = RestoreContext {
            live,
            known: &[D1, D2],
            monitors: &monitors,
            can_move,
            restore_monitors,
            ticks: 0,
            snapshot_seconds: 30,
        };
        let mut plan = Plan {
            plans: Vec::new(),
            groups: BTreeMap::new(),
            placed,
            stack: Vec::new(),
        };
        plan.plans = plan_restore(
            &view,
            due,
            &cx,
            &mut plan.groups,
            &mut plan.placed,
            &mut plan.stack,
        );
        plan
    }

    fn hwnd(handle: isize) -> HWND {
        handle as HWND
    }

    #[test]
    fn a_restore_plans_monitor_desktop_and_taskbar_moves_for_each_matched_window() {
        let plan = plan_for(
            &saved_chrome(),
            &reopened(),
            &due("Chrome", restore::Pass::Settled),
            BTreeSet::new(),
            true,
            true,
        );
        let [group] = plan.plans.as_slice() else {
            panic!("{:?}", plan.plans)
        };
        assert_eq!((group.matched, group.saved), (3, 4));
        assert_eq!(group.lost, ["Lost"]);
        let targets: Vec<(isize, monitors::Return)> = group
            .monitors
            .iter()
            .map(|step| (step.hwnd as isize, step.target))
            .collect();
        assert_eq!(
            targets,
            [
                (11, monitors::Return::Stay),
                (10, monitors::Return::To(1)),
                (12, monitors::Return::Stay),
            ]
        );
        // Every window gets a desktop move, if only to undo the pull of
        // its re-added button; a window from a desktop that is gone stays.
        let moves: Vec<(isize, DesktopId)> = group
            .moves
            .iter()
            .map(|(hwnd, target, _)| (*hwnd as isize, *target))
            .collect();
        assert_eq!(moves, [(10, D2), (11, D1), (12, D1)]);
        assert_eq!(group.gone, [("Old".to_string(), D9)]);
        // The taskbar gets the saved order.
        assert_eq!(plan.groups["Chrome"].pending(), Some(vec![11, 10, 12]));
        assert_eq!(plan.placed, BTreeSet::from([10, 11, 12]));
        assert_eq!(plan.stack, [(0, hwnd(11)), (1, hwnd(10)), (2, hwnd(12))]);
    }

    fn placements_for(
        live: &[windows::LiveWindow],
        pending: &[&str],
        placed: BTreeSet<Handle>,
    ) -> Vec<Placement> {
        let program = saved_chrome();
        // Desktop 1 for every open window, as after sign-in.
        let view = ExeView::new("chrome.exe", &program, live, &|_| Some(D1), &by_left_edge);
        let monitors = two_monitors();
        let cx = RestoreContext {
            live,
            known: &[D1, D2],
            monitors: &monitors,
            can_move: true,
            restore_monitors: true,
            ticks: 0,
            snapshot_seconds: 30,
        };
        let pending: Vec<String> = pending.iter().map(|group| group.to_string()).collect();
        plan_placements(&view, &pending, &placed, &cx)
    }

    #[test]
    fn a_window_that_opens_is_placed_at_once_without_waiting_for_the_others() {
        // Only Inbox and Docs are open so far; Old and Lost have not come.
        let live = [chrome(10, "Inbox", 0), chrome(11, "Docs", 0)];
        let mut placements = placements_for(&live, &["Chrome"], BTreeSet::new());
        placements.sort_by_key(|placement| placement.hwnd);
        assert_eq!(
            placements,
            [
                Placement {
                    hwnd: 10,
                    group: "Chrome".to_string(),
                    label: "Inbox".to_string(),
                    monitor: Some((0, 1)),
                    desktop: Some(D2),
                },
                Placement {
                    hwnd: 11,
                    group: "Chrome".to_string(),
                    label: "Docs".to_string(),
                    monitor: None,
                    desktop: None,
                },
            ],
            "Docs is in place already and is only counted as placed"
        );
    }

    #[test]
    fn a_placed_window_or_one_of_a_group_not_in_the_restore_is_left_alone() {
        let live = [chrome(10, "Inbox", 0), chrome(11, "Docs", 0)];
        let placements = placements_for(&live, &["Chrome"], BTreeSet::from([10, 11]));
        assert!(placements.is_empty(), "each window is placed once");
        let placements = placements_for(&live, &["Viber.Desktop"], BTreeSet::new());
        assert!(placements.is_empty(), "Chrome is not in the restore");
        // A window that matches no saved one is not placed.
        let stranger = [chrome(12, "Something new", 9000)];
        let saved_one = placements_for(&stranger, &["Chrome"], BTreeSet::new());
        assert!(saved_one.iter().all(|placement| placement.hwnd != 12) || saved_one.len() <= 1);
    }

    #[test]
    fn a_second_pass_moves_only_the_windows_that_came_late() {
        let mut program = saved_chrome();
        program.windows.push(kept(
            "Later",
            Some(D2),
            Some(screen("DISPLAY2", "LG", 1)),
            4,
        ));
        let mut live = reopened();
        live.push(chrome(13, "Later", 0));
        let plan = plan_for(
            &program,
            &live,
            &due("Chrome", restore::Pass::Late),
            BTreeSet::from([10, 11, 12]),
            true,
            true,
        );
        let [group] = plan.plans.as_slice() else {
            panic!("{:?}", plan.plans)
        };
        assert!(group.late);
        let stepped: Vec<isize> = group
            .monitors
            .iter()
            .map(|step| step.hwnd as isize)
            .collect();
        assert_eq!(stepped, [13], "placed windows stay on their monitor");
        let moves: Vec<(isize, DesktopId)> = group
            .moves
            .iter()
            .map(|(hwnd, target, _)| (*hwnd as isize, *target))
            .collect();
        // Inbox was put on Work by the first pass and moved back here since:
        // it only gets its pull undone, and only Later counts as moved.
        assert_eq!(moves, [(10, D1), (11, D1), (12, D1), (13, D2)]);
        assert_eq!(group.moved_late, 1);
        assert!(group.gone.is_empty() && group.lost.is_empty());
        assert!(plan.stack.is_empty(), "nothing is raised again");
        assert!(plan.placed.contains(&13));
    }

    #[test]
    fn the_second_order_after_a_sign_in_rebuilds_the_taskbar_and_moves_nothing() {
        let plan = plan_for(
            &saved_chrome(),
            &reopened(),
            &due("Chrome", restore::Pass::Again),
            BTreeSet::from([10, 11, 12]),
            true,
            true,
        );
        let [group] = plan.plans.as_slice() else {
            panic!("{:?}", plan.plans)
        };
        assert!(group.again && group.late);
        assert!(
            group.monitors.is_empty(),
            "placed windows stay on their monitor"
        );
        let targets: Vec<DesktopId> = group.moves.iter().map(|(_, target, _)| *target).collect();
        assert_eq!(
            targets,
            [D1, D1, D1],
            "each only undoes the pull of its button"
        );
        assert_eq!(plan.groups["Chrome"].pending(), Some(vec![11, 10, 12]));
        assert!(plan.stack.is_empty(), "nothing is raised again");

        let mut keeper = read_only();
        let mut done = Restored::default();
        keeper.carry_out(plan.plans, true, &[], &mut done);
        assert!(
            done.parts.is_empty() && done.late_parts.is_empty(),
            "no summary line"
        );
    }

    #[test]
    fn without_the_desktop_mover_or_monitor_restore_those_moves_are_left_out() {
        let plan = plan_for(
            &saved_chrome(),
            &reopened(),
            &due("Chrome", restore::Pass::Settled),
            BTreeSet::new(),
            false,
            false,
        );
        let group = &plan.plans[0];
        assert!(group.moves.is_empty() && group.gone.is_empty());
        assert!(group.monitors.is_empty());
        assert_eq!(group.matched, 3);
    }

    #[test]
    fn a_group_not_open_or_not_due_is_not_planned() {
        let plan = plan_for(
            &saved_chrome(),
            &[open(
                20,
                "viber.exe",
                "Viber",
                "Viber.Desktop",
                [0, 0, 300, 300],
            )],
            &due("Chrome", restore::Pass::Settled),
            BTreeSet::new(),
            true,
            true,
        );
        assert!(plan.plans.is_empty());
        let plan = plan_for(
            &saved_chrome(),
            &reopened(),
            &due("Viber.Desktop", restore::Pass::Settled),
            BTreeSet::new(),
            true,
            true,
        );
        assert!(plan.plans.is_empty(), "Viber is not chrome.exe's group");
    }

    #[test]
    fn only_watched_programs_with_a_due_group_are_restored() {
        let saved = programs(vec![
            ("chrome.exe", saved_chrome().windows),
            ("telegram.exe", vec![kept("Chat", None, None, 0)]),
            (
                "viber.exe",
                vec![SavedWindow {
                    group: "Viber.Desktop".to_string(),
                    ..kept("Viber", None, None, 0)
                }],
            ),
        ]);
        let watch = Watch::new("*", "telegram.exe");
        let due = due("Chrome", restore::Pass::Settled);
        assert_eq!(restore_exes(&saved, &due, &watch), ["chrome.exe"]);
    }

    #[test]
    fn the_summary_names_each_group_and_what_was_not_found() {
        let mut done = Restored::default();
        assert_eq!(summary(&done), None);
        done.parts = vec![
            "2 of 3 Chrome windows".to_string(),
            "1 of 1 Viber windows".to_string(),
        ];
        done.missing = 1;
        assert_eq!(
            summary(&done).as_deref(),
            Some("Restored 2 of 3 Chrome windows, 1 of 1 Viber windows (1 not found)")
        );
        done.late_parts = vec!["second pass: moved 1 late Chrome windows".to_string()];
        assert_eq!(
            summary(&done).as_deref(),
            Some(
                "Restored 2 of 3 Chrome windows, 1 of 1 Viber windows (1 not found); second pass: moved 1 late Chrome windows"
            )
        );
        done.parts.clear();
        done.missing = 0;
        assert_eq!(
            summary(&done).as_deref(),
            Some("second pass: moved 1 late Chrome windows")
        );
    }

    fn finish(stack: Vec<(usize, HWND)>, foreground: HWND) -> Finish {
        Finish {
            moves: vec![
                (hwnd(10), D2, "Inbox".to_string()),
                (hwnd(11), D1, "Docs".to_string()),
            ],
            stack,
            foreground,
            summary: String::new(),
            announce: false,
            targets: Vec::new(),
            orders: Vec::new(),
        }
    }

    #[test]
    fn the_second_half_skips_windows_on_the_current_desktop_and_raises_back_to_front() {
        let done = finish(vec![(1, hwnd(10)), (0, hwnd(11)), (2, hwnd(12))], hwnd(99));
        let (moves, raises) = finish_steps(&done, Some(D1), true);
        let moved: Vec<isize> = moves.iter().map(|(hwnd, _, _)| *hwnd as isize).collect();
        assert_eq!(moved, [10]);
        let raised: Vec<isize> = raises.iter().map(|hwnd| *hwnd as isize).collect();
        assert_eq!(raised, [12, 10, 11, 99], "the old front window ends on top");

        let (_, raises) = finish_steps(&done, Some(D1), false);
        assert!(raises.is_empty(), "the front order is not restored");
        let late = finish(Vec::new(), hwnd(99));
        let (moves, raises) = finish_steps(&late, None, true);
        assert_eq!(moves.len(), 2, "with no current desktop every move runs");
        assert!(raises.is_empty(), "a second pass raises nothing");
        let (_, raises) = finish_steps(
            &finish(vec![(0, hwnd(10))], std::ptr::null_mut()),
            None,
            true,
        );
        assert_eq!(raises, [hwnd(10)]);
    }

    #[test]
    fn a_read_only_instance_carries_out_no_planned_move_but_still_reports() {
        let mut keeper = read_only();
        let plans = vec![GroupPlan {
            group: "Chrome".to_string(),
            monitors: vec![MonitorStep {
                hwnd: NO_WINDOW,
                label: "Inbox".to_string(),
                saved: None,
                now: Some(0),
                target: monitors::Return::To(1),
            }],
            moves: vec![(NO_WINDOW, D2, "Inbox".to_string())],
            matched: 1,
            saved: 2,
            lost: vec!["Lost".to_string()],
            ..GroupPlan::default()
        }];
        let mut done = Restored::default();
        keeper.carry_out(plans, true, &[], &mut done);
        assert_eq!(keeper.refused, 2, "the monitor move and the taskbar order");
        assert_eq!(done.parts, ["1 of 2 Chrome windows"]);
        assert_eq!((done.missing, done.moves.len()), (1, 1));

        let late = vec![GroupPlan {
            group: "Chrome".to_string(),
            late: true,
            moved_late: 1,
            ..GroupPlan::default()
        }];
        keeper.carry_out(late, false, &[], &mut done);
        assert_eq!(
            done.late_parts,
            ["second pass: moved 1 late Chrome windows"]
        );
    }

    #[test]
    fn the_check_wants_each_window_on_its_move_desktop_and_step_monitor() {
        let plan = GroupPlan {
            group: "Chrome".to_string(),
            monitors: vec![
                MonitorStep {
                    hwnd: hwnd(10),
                    label: "Inbox".to_string(),
                    saved: None,
                    now: Some(0),
                    target: monitors::Return::To(1),
                },
                MonitorStep {
                    hwnd: hwnd(11),
                    label: "Docs".to_string(),
                    saved: None,
                    now: Some(0),
                    target: monitors::Return::Stay,
                },
                MonitorStep {
                    hwnd: hwnd(12),
                    label: "Old".to_string(),
                    saved: None,
                    now: None,
                    target: monitors::Return::Unplugged,
                },
            ],
            moves: vec![
                (hwnd(10), D2, "Inbox".to_string()),
                (hwnd(13), D1, "Late".to_string()),
                (hwnd(14), DesktopId::ALL, "Pinned".to_string()),
            ],
            ..GroupPlan::default()
        };
        let device = |index: usize| Some(format!("DISPLAY{}", index + 1));
        let wanted: Vec<(isize, Option<DesktopId>, Option<String>)> =
            check_targets_with(&plan, &device)
                .into_iter()
                .map(|target| (target.hwnd, target.desktop, target.monitor))
                .collect();
        assert_eq!(
            wanted,
            [
                (10, Some(D2), Some("DISPLAY2".to_string())),
                (11, None, Some("DISPLAY1".to_string())),
                (13, Some(D1), None),
            ],
            "an unplugged monitor and a pinned window are not checked"
        );
    }

    fn check_of(targets: Vec<verify::Target>) -> Check {
        Check {
            due: 0,
            orders: vec![(
                "Chrome".to_string(),
                targets.iter().map(|target| target.hwnd).collect(),
            )],
            targets,
            moved_again: None,
        }
    }

    #[test]
    fn a_window_out_of_place_is_moved_once_more_and_then_reported() {
        let mut keeper = read_only();
        let target = verify::Target {
            hwnd: 10,
            group: "Chrome".to_string(),
            label: "Inbox".to_string(),
            desktop: Some(D2),
            monitor: None,
        };
        // Without a desktop reader the window's desktop cannot be read, so
        // it is not on the wanted one.
        let live = [chrome(10, "Inbox", 0)];
        keeper.check_restore(check_of(vec![target]), &live, &[], &two_desktops());
        assert_eq!(keeper.refused, 1, "the second desktop move");
        let [again] = keeper.checks.as_slice() else {
            panic!("the check comes back")
        };
        assert_eq!(again.moved_again, Some(vec![0]));
        assert_eq!(again.due, CHECK_SECONDS);

        let check = keeper.checks.pop().unwrap();
        keeper.check_restore(check, &live, &[], &two_desktops());
        assert!(keeper.checks.is_empty(), "the second look is the last");
        assert_eq!(keeper.refused, 1, "nothing is moved a third time");
    }

    #[test]
    fn a_check_of_windows_that_closed_since_moves_nothing() {
        let mut keeper = read_only();
        let target = verify::Target {
            hwnd: 10,
            group: "Chrome".to_string(),
            label: "Inbox".to_string(),
            desktop: Some(D2),
            monitor: Some("DISPLAY1".to_string()),
        };
        keeper.check_restore(check_of(vec![target]), &[], &[], &two_desktops());
        assert!(keeper.checks.is_empty());
        assert_eq!(keeper.refused, 0);
        // A check that is not due yet waits.
        keeper.checks.push(Check {
            due: 5,
            ..check_of(Vec::new())
        });
        keeper.run_checks();
        assert_eq!(keeper.checks.len(), 1);
    }

    #[test]
    fn a_monitor_step_that_stays_or_is_unplugged_moves_nothing() {
        let mut keeper = read_only();
        for target in [monitors::Return::Stay, monitors::Return::Unplugged] {
            keeper.return_to_monitor(
                &MonitorStep {
                    hwnd: NO_WINDOW,
                    label: "Inbox".to_string(),
                    saved: Some(screen("DISPLAY3", "SAMSUNG", 2)),
                    now: Some(0),
                    target,
                },
                &[],
            );
        }
        assert_eq!(keeper.refused, 0, "no move was even tried");
    }

    #[test]
    fn timer_saves_come_every_snapshot_interval() {
        assert!(save_due(30, 30) && save_due(60, 30) && !save_due(31, 30));
        // A zero interval cannot come from the settings, but never divides by it.
        assert!(save_due(7, 0));
    }

    /// A keeper that watches a program nobody runs, so its saves see no
    /// window of the user's at all.
    fn watching_nothing(name: &str) -> (LayoutKeeper, std::path::PathBuf) {
        let before = programs(vec![("viber.exe", vec![kept("Viber", None, None, 0)])]);
        let (mut keeper, path) = keeper_with(before, name);
        keeper.watch = Watch::new("wincraft-test-nothing.exe", "");
        (keeper, path)
    }

    #[test]
    fn a_tick_saves_on_the_interval() {
        let (mut keeper, path) = watching_nothing("tick-save");
        keeper.snapshot_seconds = 3;
        keeper.tick();
        keeper.tick();
        assert!(!path.exists());
        keeper.tick();
        let file = read_file(&path);
        assert_eq!(file.reason, "timer");
        assert!(
            file.programs.is_empty(),
            "viber.exe is not watched any more"
        );
        assert!(keeper.last_saved.is_some());
        clean_up(&path);
    }

    #[test]
    fn a_tick_finishes_a_restore_first_and_saves_nothing_then() {
        let (mut keeper, path) = watching_nothing("tick-finish");
        keeper.read_only = true;
        keeper.snapshot_seconds = 1;
        keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
        keeper.tick();
        assert!(keeper.finishing.is_none());
        assert_eq!(keeper.refused, 1);
        assert!(!path.exists(), "the half-moved windows were not saved");
        // While a restore is between its halves no save runs at all.
        keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
        keeper.save("timer");
        assert!(!path.exists());
        keeper.finishing = None;
        keeper.tick();
        assert!(path.exists());
        clean_up(&path);
    }

    #[test]
    fn a_tick_rebuilds_the_groups_a_desktop_move_changed() {
        let (mut keeper, _) = watching_nothing("tick-reapply");
        keeper.read_only = true;
        keeper.reapply = vec!["Chrome".to_string()];
        keeper.arrange_stale = true;
        keeper.tick();
        assert!(keeper.reapply.is_empty() && !keeper.arrange_stale);
        assert_eq!(keeper.refused, 1, "the taskbar reorder was turned down");
    }

    #[test]
    fn a_restart_of_explorer_starts_the_wait_unless_the_instance_is_read_only() {
        let (mut keeper, _) = watching_nothing("explorer-restart");
        keeper.read_only = false;
        keeper.ticks = 40;
        keeper.on_taskbar_created();
        assert!(!keeper.explorer.due(44));
        assert!(keeper.explorer.due(45));

        let (mut test_copy, _) = watching_nothing("explorer-restart-read-only");
        test_copy.read_only = true;
        test_copy.on_taskbar_created();
        assert!(!test_copy.explorer.due(1000));
        assert_eq!(test_copy.refused, 1);
    }

    #[test]
    fn the_end_of_a_session_keeps_a_snapshot_but_an_installer_closing_apps_does_not() {
        let (mut keeper, path) = watching_nothing("end-session");
        let snapshot = state::snapshot_path(&path);
        keeper.on_windows_message(WM_QUERYENDSESSION, 0, ENDSESSION_CLOSEAPP as LPARAM);
        assert_eq!(read_file(&path).reason, "shutdown");
        assert!(!snapshot.exists(), "Windows keeps running after this");
        keeper.on_windows_message(WM_ENDSESSION, 0, 0);
        assert!(!snapshot.exists(), "the session did not end after all");
        keeper.on_windows_message(WM_QUERYENDSESSION, 0, 0);
        let kept = state::load(&snapshot).expect("a snapshot");
        assert_eq!(kept.reason, "shutdown");
        assert_eq!(Some(&kept.programs), keeper.writer.last());
        clean_up(&path);

        // Between the two halves of a restore neither file is written.
        let (mut keeper, path) = watching_nothing("end-session-restore");
        keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
        keeper.on_windows_message(WM_ENDSESSION, 1, 0);
        assert!(!path.exists() && !state::snapshot_path(&path).exists());
        clean_up(&path);
    }

    fn saved_at(programs: Programs, saved: &str, reason: &str) -> state::StateFile {
        state::StateFile {
            version: state::VERSION,
            saved: saved.to_string(),
            reason: reason.to_string(),
            programs,
        }
    }

    #[test]
    fn the_start_layout_is_the_snapshot_unless_something_was_saved_after_it() {
        let at_shutdown = programs(vec![("chrome.exe", vec![kept("Shutdown", None, None, 0)])]);
        let later = programs(vec![("chrome.exe", vec![kept("Later", None, None, 0)])]);
        let snapshot = saved_at(at_shutdown.clone(), "2026-10-22T21:20:00+03:00", "shutdown");

        let unchanged = saved_at(later.clone(), "2026-10-22T18:00:00+03:00", "timer");
        let chosen = start_layout(Some(unchanged), Some(snapshot.clone())).unwrap();
        assert_eq!(chosen.programs, at_shutdown);

        let crashed = saved_at(later.clone(), "2026-10-23T09:00:00+03:00", "timer");
        let chosen = start_layout(Some(crashed), Some(snapshot.clone())).unwrap();
        assert_eq!(
            chosen.programs, later,
            "the session after it ended in a crash"
        );

        assert_eq!(
            start_layout(None, Some(snapshot)).unwrap().programs,
            at_shutdown
        );
        assert!(start_layout(None, None).is_none());
    }

    #[test]
    fn init_starts_from_the_snapshot_when_it_is_the_newest() {
        let path = std::env::temp_dir().join(format!(
            "wincraft-test-layout-init-snapshot-{}.json",
            std::process::id()
        ));
        let mut writer = state::Writer::new(path.clone(), None);
        writer
            .write(
                programs(vec![("viber.exe", vec![kept("Older", None, None, 0)])]),
                "timer",
                "2026-10-22T18:00:00+03:00".to_string(),
                true,
            )
            .unwrap();
        let snapshot = state::Writer::new(path.clone(), Some(layout_file()));
        snapshot
            .write_snapshot("shutdown", "2026-10-22T21:20:00+03:00".to_string())
            .unwrap();
        let mut keeper = LayoutKeeper {
            state_path: path.clone(),
            ..LayoutKeeper::default()
        };
        let settings = serde_json::json!({
            "programs": "wincraft-test-nothing.exe",
            "restore_on_start": false,
        });
        let ctx = HostContext {
            hwnd: std::ptr::null_mut(),
            settings: &settings,
        };
        keeper.init(&ctx).unwrap();
        assert_eq!(keeper.writer.last(), Some(&layout_file()));
        keeper.host_hwnd = None;
        keeper.teardown();
        clean_up(&path);
    }

    #[test]
    fn a_run_where_no_app_opened_says_so() {
        let mut run = Restore::new(15, 30 * 60);
        assert_eq!(
            nothing_restored(&run).as_deref(),
            Some(
                "None of the saved apps opened a window within 30 minutes, so nothing was restored"
            )
        );
        run.start(0, vec!["Chrome".to_string()]);
        assert_eq!(nothing_restored(&run), None, "Chrome is still waited for");
    }

    #[test]
    fn saved_groups_start_with_their_saved_taskbar_order() {
        let mut viber = kept("Viber", None, None, 0);
        viber.group = "Viber.Desktop".to_string();
        let mut old = kept("From version 1", None, None, 0);
        old.group = String::new();
        let saved = programs(vec![
            (
                "chrome.exe",
                vec![kept("B", None, None, 1), kept("A", None, None, 0), old],
            ),
            ("viber.exe", vec![viber]),
        ]);
        let mut models = saved_models(Some(&saved));
        assert_eq!(
            models.keys().collect::<Vec<_>>(),
            ["Chrome", "Viber.Desktop"]
        );
        let chrome = models.get_mut("Chrome").unwrap();
        let live = [chrome_handle(1, "B"), chrome_handle(2, "A")];
        chrome.refresh(&live, 0, 30);
        assert_eq!(chrome.handles(), [2, 1], "A was saved first");
        assert!(saved_models(None).is_empty());
    }

    fn chrome_handle(handle: Handle, title: &str) -> (Handle, identity::WindowIdentity) {
        let window = chrome(handle, title, 0);
        (handle, window.identity)
    }

    #[test]
    fn init_reads_the_saved_layout_and_teardown_saves_and_forgets() {
        let path = std::env::temp_dir().join(format!(
            "wincraft-test-layout-init-{}.json",
            std::process::id()
        ));
        let mut writer = state::Writer::new(path.clone(), None);
        writer
            .write(
                layout_file(),
                "manual",
                "2026-10-18T09:00:00".to_string(),
                true,
            )
            .unwrap();
        let mut keeper = LayoutKeeper {
            state_path: path.clone(),
            ..LayoutKeeper::default()
        };
        let settings = serde_json::json!({
            "programs": "wincraft-test-nothing.exe",
            "restore_on_start": false,
        });
        // No window: the timer is the thread's own, and goes with it.
        let ctx = HostContext {
            hwnd: std::ptr::null_mut(),
            settings: &settings,
        };
        keeper.init(&ctx).unwrap();
        assert_eq!(keeper.writer.last(), Some(&layout_file()));
        assert_eq!(
            keeper.groups.keys().collect::<Vec<_>>(),
            ["Chrome", "Viber.Desktop"]
        );
        assert!(keeper.host_hwnd.is_some() && keeper.started_once);
        assert!(!keeper.restore.is_running());

        keeper
            .labels
            .insert("Other".to_string(), "Other".to_string());
        keeper.teardown();
        assert_eq!(read_file(&path).reason, "teardown");
        assert!(keeper.host_hwnd.is_none() && keeper.reader.is_none());
        assert!(keeper.groups.is_empty() && keeper.labels.is_empty());

        // Switched on again, it reads what teardown wrote. Torn down in the
        // middle of a restore, it drops the restore and saves nothing.
        keeper.init(&ctx).unwrap();
        assert!(keeper.groups.is_empty(), "nothing watched was open");
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
        keeper.teardown();
        assert!(keeper.finishing.is_none());
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            written
        );
        clean_up(&path);
    }
}
