mod explorer;
mod guard;
mod monitors;
// The look of the panel is shared with the host's update notice.
pub(crate) mod notice;
mod plugin;
mod restore;
mod restore_plan;
mod restore_run;
mod saving;
mod settings;
mod state;
mod taskbar;
mod tick;
mod tracking;
mod verify;
use restore_plan::*;
use saving::*;
use settings::*;

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

#[cfg(test)]
mod tests;
