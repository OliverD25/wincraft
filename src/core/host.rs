use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, SetFocus, UnregisterHotKey};
use windows_sys::Win32::UI::Shell::{ShellExecuteW, NIN_BALLOONUSERCLICK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DispatchMessageW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, KillTimer, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterShellHookWindow, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, TranslateMessage, ENDSESSION_CLOSEAPP, ENDSESSION_CRITICAL,
    ENDSESSION_LOGOFF, MSG, PBT_APMRESUMEAUTOMATIC, SPI_SETHIGHCONTRAST, SW_SHOWNORMAL, WM_APP,
    WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP,
    WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
    WS_OVERLAPPED,
};

use serde_json::Value;

use crate::core::attention::Attention;
use crate::core::config::{Config, PluginConfig, UpdateConfig, DEFAULT_PALETTE_HOTKEY};
use crate::core::hotkeys::Shortcut;
use crate::core::open_timing::{self, OpenStep};
use crate::core::shortcuts::{self, FirstPress};
use crate::core::traits::{
    HostContext, Hotkey, PluginWindow, StripLook, WinCraftPlugin, WindowGroups,
};
use crate::core::tray::{show_menu, MenuItem, Tray, WM_TRAY_CALLBACK};
use crate::core::ui_bridge::{
    self, ActionKind, ArrangeAction, ArrangeGroup, ArrangeSnapshot, CommandId, FieldInfo,
    HostCommand, HostRequest, HostSetting, HotkeyInfo, MonitorRect, NewRelease, Page, PaletteEntry,
    PeekCard, PluginInfo, Prompt, SettingsTarget, ShortcutChange, ShortcutInfo, ToggleState,
    UiChannel, UiCommand, UiSnapshot, UpdateStatus, UpdateView, WM_APP_UI,
};
use crate::core::update::{self, Checked, Handover};
use crate::core::update_notice::{self, UpdateNotice};
use crate::core::{
    autostart, chord, config, hotkeys, install, instance, mask_key, release_notes, taskbar,
    taskbar_number, theme, update_stage, update_timing, wide, window_snapshot,
};
use crate::search::{self, Router, SearchProvider};
use crate::store::github::{self, Release};
use crate::ui;

mod api;
mod arrange;
mod bindings;
mod detailed_log;
mod plugins;
mod prepared;
mod requests;
mod taskbar_cache;
mod updates;
mod view;
mod wndproc;

pub use api::*;
use arrange::*;
use plugins::*;
use updates::*;
#[cfg(test)]
use view::announce_update;
use wndproc::*;

/// The host window's class, which `--quit` looks for; see instance::name.
pub const CLASS_NAME: &str = "WinCraftHost";

const MENU_QUIT: u32 = 5;
const MENU_PALETTE: u32 = 6;
const MENU_SETTINGS: u32 = 7;
const MENU_UPDATE: u32 = 8;
const MENU_DETAILED_LOG: u32 = 9;
const MENU_PLUGIN_ACTION_BASE: u32 = 1000;

const DETECTOR_ID: &str = "shortcut_detector";
const DETECTOR_OPEN_ACTION: u32 = 1;
/// Window names go to this plugin, whichever plugin draws the strip.
const NAMER_ID: &str = "window_namer";

/// The palette belongs to the host, not to a plugin, so it takes the one id
/// that plugin hotkeys never use.
const PALETTE_HOTKEY_ID: i32 = 0;
/// The ids of the shortcuts the host registers itself: commands without a
/// hotkey of their own, and the first press of every chord. Plugin hotkeys
/// count up from 1 and never get this far.
const SHORTCUT_ID_BASE: i32 = 0x6000;
const UI_STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// Posted by `notify` and `plugin_changed`, which plugins call while the host
/// is busy calling them; the work happens when the message comes round.
const WM_APP_PLUGIN: u32 = WM_APP + 21;

/// Delivers again the messages that found the host busy; see
/// `with_host_or_later`.
const RETRY_TIMER: usize = 0x5743_0001;
const RETRY_MS: u32 = 50;
const RETRY_LIMIT: usize = 64;

/// Posted by the update threads once they have put their result in the
/// host's channel.
const WM_APP_UPDATE: u32 = WM_APP + 22;
/// The first update check waits a while after start, when the network may
/// not be up yet and the other programs are starting too; then the timer
/// comes round every hour and asks GitHub each time.
const UPDATE_TIMER: usize = 0x5743_0002;
const FIRST_CHECK_MS: u32 = 2 * 60 * 1000;
const CHECK_TIMER_MS: u32 = update_timing::CHECK_EVERY.as_millis() as u32;
/// A version an update started tells the old one it is healthy when this
/// fires.
const HEALTHY_TIMER: usize = 0x5743_0003;
/// Fades the "Updated to" notice, and the message its click posts.
const NOTICE_TIMER: usize = 0x5743_0004;
/// Shows the number badges once Win+Alt+A has waited a moment without a
/// digit (`taskbar_number::REVEAL_MS`).
const BADGE_TIMER: usize = 0x5743_0005;
/// Checks every 50 ms whether the app index is ready, so the palette that
/// reopens after an update is drawn complete.
const REOPEN_TIMER: usize = 0x5743_0006;
/// A window came or went: the taskbar's buttons are read again once things
/// have settled for `BUTTONS_SETTLE_MS`.
const BUTTONS_TIMER: usize = 0x5743_0007;
const BUTTONS_SETTLE_MS: u32 = 400;
/// Fires once, `update_timing::WAKE_DELAY` after the PC woke from sleep.
const WAKE_TIMER: usize = 0x5743_0008;
const WM_APP_NOTICE_CLICK: u32 = WM_APP + 23;
/// Win+Alt+A, then a digit: the message the keyboard hook posts for a key it
/// kept or for the end of the wait.
const WM_APP_WAIT_KEY: u32 = WM_APP + 24;
/// Win and a digit from a keyboard hook that runs on a thread of its own;
/// see `post_taskbar_button`.
const WM_APP_WIN_DIGIT: u32 = WM_APP + 25;
/// A Windows key let go, from the same hook; see `post_win_released`.
const WM_APP_WIN_UP: u32 = WM_APP + 26;
/// The taskbar's buttons read on a thread of their own are ready.
const WM_APP_BUTTONS: u32 = WM_APP + 27;
/// A Windows key went down, from WindowDeck's hook; see `post_win_down`.
const WM_APP_WIN_DOWN: u32 = WM_APP + 28;
/// The windows read on a thread of their own after a Windows key went down
/// are ready; see `Host::prepare_deck`.
const WM_APP_SNAPSHOT: u32 = WM_APP + 29;
/// How long an opening strip waits for windows that are still being read
/// on their own thread. They take 10–30 ms; reading them again on the host
/// takes as long, so waiting is never slower unless the thread is stuck.
const SNAPSHOT_WAIT: std::time::Duration = std::time::Duration::from_millis(100);
/// The strip Win and a digit asked for counts as open this long before the
/// UI thread has its window, so the same digit again picks the next card
/// instead of opening the strip a second time.
const STRIP_COMING: std::time::Duration = std::time::Duration::from_secs(1);

struct RegisteredHotkey {
    action_id: u32,
    global_id: i32,
    label: &'static str,
    hotkey: Hotkey,
    /// The shortcut as config text; empty when the user removed it.
    keys: String,
    ok: bool,
    /// Registered here. A chord's first press is registered with the other
    /// shortcuts instead, and a removed shortcut not at all.
    active: bool,
}

struct PluginSlot {
    plugin: Box<dyn WinCraftPlugin>,
    enabled: bool,
    registered: Vec<RegisteredHotkey>,
}

struct Host {
    hwnd: HWND,
    config: Config,
    plugins: PluginSet,
    tray: Tray,
    to_ui: Arc<UiChannel>,
    host_rx: Receiver<HostRequest>,
    palette_hwnd: HWND,
    balloon_click: BalloonClick,
    /// A test scene's group and peek for the next time the strip opens.
    arrange_focus: Option<String>,
    peek_card: Option<PeekCard>,
    /// The hovered taskbar button the next strip opens above: its app and
    /// its place.
    arrange_anchor: Option<(String, [i32; 4])>,
    /// The digits typed after Win+Alt+A, and the main taskbar's buttons
    /// they count.
    digits: taskbar_number::Digits,
    buttons: Vec<taskbar::Button>,
    buttons_at: Option<std::time::Instant>,
    /// What the cache of `buttons` knows; see `taskbar_cache`.
    buttons_cache: taskbar_cache::Cache,
    /// WindowDeck's windows, gathered when a Windows key went down.
    prepared: prepared::Prepared<(&'static str, WindowGroups)>,
    /// The list being read on a thread of its own.
    buttons_coming: Option<std::sync::mpsc::Receiver<Result<Vec<taskbar::Button>, String>>>,
    /// The windows being read on a thread of their own for `prepared`, and
    /// when they were asked for.
    snapshot_coming: Option<(
        std::sync::mpsc::Receiver<crate::core::window_snapshot::Snapshot>,
        std::time::Instant,
    )>,
    clock: std::time::Instant,
    /// The number badges and the line while Win+Alt+A waits for a digit.
    wait_hint: Option<ui::taskbar_badges::WaitHint>,
    /// When they show, and the line they show with it.
    reveal: taskbar_number::Reveal,
    reveal_line: String,
    updates: Updates,
    /// The "Updated to" panel while it is on screen.
    update_notice: Option<UpdateNotice>,
    /// Why this copy leaves the logon task alone, worked out once at start.
    autostart_note: Option<String>,
    /// The first presses registered for shortcuts: id, what they do, and
    /// whether Windows took them.
    shortcut_keys: Vec<(i32, FirstPress, bool)>,
    /// The chord whose second press is awaited.
    chord_wait: Option<FirstPress>,
    /// When Win and a digit last opened the strip; see `STRIP_COMING`.
    win_digit_opened: Option<std::time::Instant>,
    /// The app of the taskbar button the strip last opened above, so Win
    /// and that button's digit again pick the next card.
    deck_app: Option<String>,
}

/// What became of a Win+number that WindowDeck's hook kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WinDigit {
    Opened,
    NextCard,
    /// The button's app has no window the strip shows (a pinned app that
    /// is not running, say), or there is no such button: Windows gets the
    /// keys back and does what it always does.
    GiveBack,
}

/// What clicking the last balloon opens: a "hotkey is taken" balloon opens
/// ShortcutDetector, an "update available" one the About page, and a
/// plugin's own notice nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BalloonClick {
    Detector,
    About,
    Nothing,
}

#[derive(Clone, Debug, Default)]
pub struct StartupFlags {
    pub open_detector: bool,
    pub open_palette: bool,
    pub open_arrange: bool,
    pub open_settings: bool,
    /// `--updated-from <version>`: an update started this process.
    pub updated_from: Option<String>,
    /// `--update-failed <version>`: a rollback from that version started it.
    pub update_failed: Option<String>,
    /// `--reopen palette|about`, given with either of those: the surface the
    /// user started the update from.
    pub reopen: Option<update::Reopen>,
    /// `--restarted`: Windows started WinCraft again after an installer
    /// closed it.
    pub restarted: bool,
    /// The rest are for test instances only; main.rs leaves them empty
    /// otherwise.
    pub scene: SceneFlags,
}

/// What a test scene opens, from the `--open-*=` and `--peek-card=` flags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneFlags {
    pub palette_query: Option<String>,
    pub settings: Option<SettingsTarget>,
    pub arrange_focus: Option<String>,
    pub peek_card: Option<PeekCard>,
    /// A window to rename at start, and the name: how a live test renames
    /// a window of its own without any input.
    pub rename: Option<(isize, String)>,
    /// A window to open the palette's rename box for.
    pub ask_name: Option<isize>,
    /// Show LayoutKeeper's please-wait panel with sample progress.
    pub restore_panel: bool,
    /// Let the hover strip hide Windows' taskbar preview, for a supervised
    /// live test of it.
    pub allow_hide_popup: bool,
    /// A folder, file or address to open the palette's bookmark box for.
    pub ask_bookmark: Option<String>,
    /// Check for updates at once and install what the check finds.
    pub update_now: bool,
    /// Show the "Updated to" notice, as after an update.
    pub update_notice: bool,
    /// Show About's update progress bar half way through a sample download;
    /// nothing is downloaded.
    pub update_progress: bool,
    /// Show About with a sample newer release; nothing is downloaded.
    pub update_available: bool,
    /// A window to mark as wanting attention, as if it had flashed.
    pub attention: Option<isize>,
}

static DEMO_RESTORE_PANEL: AtomicBool = AtomicBool::new(false);
static ALLOW_HIDE_POPUP: AtomicBool = AtomicBool::new(false);
static STARTED_AGAIN: AtomicBool = AtomicBool::new(false);

/// True when something other than the sign-in task started this process: an
/// update, a rollback, or Windows starting WinCraft again after an installer
/// closed it. LayoutKeeper restores the saved layout only after a sign-in.
pub fn started_again() -> bool {
    STARTED_AGAIN.load(Ordering::Relaxed)
}

/// True in a test instance started with `--show-restore-panel`: LayoutKeeper
/// shows its please-wait panel with sample progress instead of a restore.
pub fn demo_restore_panel() -> bool {
    DEMO_RESTORE_PANEL.load(Ordering::Relaxed)
}

thread_local! {
    /// When the palette's reopening after an update began to wait for the app
    /// index (`api::reopen_palette_when_ready`).
    static REOPEN_SINCE: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// Kept outside Host so the WndProc can check it without borrowing. Windows
    /// re-enters a WndProc whenever it likes, and a borrow taken on every single
    /// message would turn that into a crash.
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    /// The shell hook's message and the windows it says want attention;
    /// outside Host for the same reason, as they come with every switch
    /// between windows.
    static SHELL_HOOK: Cell<u32> = const { Cell::new(0) };
    static ATTENTION: RefCell<Attention> = RefCell::new(Attention::default());
    static HOST_WINDOW: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
    static NOTICES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// A tooltip a plugin asked for and the host has not applied yet; the
    /// inner None means the plain name.
    static TRAY_TIP: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    static ARRANGE_WANTED: Cell<bool> = const { Cell::new(false) };
    static ARRANGE_HOTKEY: Cell<bool> = const { Cell::new(false) };
    static PROMPT: RefCell<Option<Prompt>> = const { RefCell::new(None) };
    static ARRANGE_STALE: Cell<bool> = const { Cell::new(false) };
    static ARRANGE_ANCHOR: RefCell<Option<(String, [i32; 4])>> = const { RefCell::new(None) };
    static ARRANGE_CLOSE: Cell<bool> = const { Cell::new(false) };
    /// A plugin that gives the strip its look was stopped: once no running
    /// plugin does, the UI frees the strip's kept window.
    static ARRANGE_FREE: Cell<bool> = const { Cell::new(false) };
    /// Win+number presses a plugin's hook kept: the digit, and whether it
    /// is the same digit again while Win stayed down.
    static WIN_DIGITS: RefCell<Vec<(u8, bool)>> = const { RefCell::new(Vec::new()) };
    static SETTINGS_WANTED: RefCell<Vec<(String, String, Value)>> = const { RefCell::new(Vec::new()) };
    /// Plugin windows to open or close: the plugin, the window (None for
    /// all of them) and whether to open it.
    static WINDOWS_WANTED: RefCell<Vec<(String, Option<String>, bool)>> = const { RefCell::new(Vec::new()) };
    static RETRY: RefCell<Vec<(u32, WPARAM, LPARAM)>> = const { RefCell::new(Vec::new()) };
    /// The message the WndProc is working on, so the log can name a message
    /// that found the host busy.
    static CURRENT_MESSAGE: Cell<Option<(u32, WPARAM)>> = const { Cell::new(None) };
}

pub fn run(config: Config, plugins: Vec<Box<dyn WinCraftPlugin>>, flags: StartupFlags) {
    detailed_log::start_detailed_log(&config);
    DEMO_RESTORE_PANEL.store(flags.scene.restore_panel, Ordering::Relaxed);
    ALLOW_HIDE_POPUP.store(flags.scene.allow_hide_popup, Ordering::Relaxed);
    STARTED_AGAIN.store(
        flags.restarted || flags.updated_from.is_some() || flags.update_failed.is_some(),
        Ordering::Relaxed,
    );
    theme::set_current(config.theme, config.mode);
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };

    let class_name = wide(&instance::name(CLASS_NAME));
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(wnd_proc);
    class.hInstance = hinstance;
    class.lpszClassName = class_name.as_ptr();
    if unsafe { RegisterClassW(&class) } == 0 {
        log::error!("could not register the host window class");
        return;
    }

    // A normal top-level window that is simply never shown, not an HWND_MESSAGE
    // window. Message-only windows are skipped by broadcasts, and the host must
    // see WM_DISPLAYCHANGE to tell plugins that the monitor layout changed.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            wide("WinCraft").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        log::error!("could not create the host window");
        return;
    }

    let bridge = ui_bridge::create();
    bridge.to_host.set_host_window(hwnd);

    let mut host = Host {
        hwnd,
        config,
        plugins: PluginSet::new(
            hwnd,
            plugins,
            Box::new(WindowHotkeys(hwnd)),
            config::data_dir(),
        ),
        tray: Tray::new(hwnd, hinstance),
        to_ui: Arc::clone(&bridge.to_ui),
        host_rx: bridge.host_rx,
        palette_hwnd: std::ptr::null_mut(),
        balloon_click: BalloonClick::Detector,
        arrange_focus: flags.scene.arrange_focus.clone(),
        peek_card: flags.scene.peek_card,
        arrange_anchor: None,
        digits: taskbar_number::Digits::default(),
        buttons: Vec::new(),
        buttons_at: None,
        buttons_cache: taskbar_cache::Cache::default(),
        buttons_coming: None,
        snapshot_coming: None,
        prepared: prepared::Prepared::default(),
        clock: std::time::Instant::now(),
        wait_hint: None,
        reveal: taskbar_number::Reveal::default(),
        reveal_line: String::new(),
        updates: Updates::default(),
        update_notice: None,
        autostart_note: autostart::hands_off_note(),
        shortcut_keys: Vec::new(),
        chord_wait: None,
        win_digit_opened: None,
        deck_app: None,
    };
    HOST_WINDOW.with(|cell| cell.set(hwnd));
    HOST_HWND.store(hwnd as isize, Ordering::Relaxed);
    let _ = TO_UI.set(Arc::clone(&host.to_ui));
    // Notices queued before the window existed have nobody to post them.
    plugin_changed();
    TASKBAR_CREATED
        .with(|cell| cell.set(unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) }));
    SHELL_HOOK.with(|cell| cell.set(unsafe { RegisterWindowMessageW(wide("SHELLHOOK").as_ptr()) }));
    if unsafe { RegisterShellHookWindow(hwnd) } == 0 {
        log::warn!("cannot hear which windows want attention; the strip will not mark them");
    }
    if let Some(window) = flags.scene.attention {
        ATTENTION.with(|attention| attention.borrow_mut().mark(window));
    }

    host.plugins.load_configs();
    let palette = host.config.palette_hotkey.clone();
    host.plugins.register_palette(&palette);
    for index in host.plugins.wanted() {
        host.plugins.enable(index);
    }
    host.register_shortcuts();
    // Before the tray icon exists, so these balloons are not shown, as
    // they never were.
    host.show_warnings();
    host.save_config();

    let ui_thread = ui::start(
        bridge.ui_rx,
        Arc::clone(&bridge.to_ui),
        Arc::clone(&bridge.to_host),
        host.snapshot(),
        host.search_router(),
        host.plugin_windows(),
    );

    host.tray.add();
    log::info!("WinCraft {} is running", env!("CARGO_PKG_VERSION"));
    host.detailed_log_at_start();
    if update::automatic_checks_allowed() {
        unsafe { SetTimer(hwnd, UPDATE_TIMER, FIRST_CHECK_MS, None) };
    }
    host.start_updates(&flags);

    HOST.with(|cell| *cell.borrow_mut() = Some(host));

    if flags.open_detector {
        open_shortcut_detector();
    }
    if flags.open_palette || flags.scene.palette_query.is_some() {
        show_palette();
    } else if flags.reopen == Some(update::Reopen::Palette) {
        reopen_palette_when_ready(hwnd);
    }
    if let Some(text) = flags.scene.palette_query.filter(|text| !text.is_empty()) {
        with_host(|host| host.to_ui.send(UiCommand::TypeInPalette(text)));
    }
    if flags.open_arrange || flags.scene.arrange_focus.is_some() || flags.scene.peek_card.is_some()
    {
        open_arrange();
    }
    if let Some((hwnd, name)) = flags.scene.rename {
        with_host(|host| {
            host.arrange_action(
                NAMER_ID,
                &ArrangeAction::Rename {
                    hwnd,
                    name: Some(name),
                },
            )
        });
    }
    if let Some(target) = flags.scene.ask_bookmark {
        ask(Prompt::from_ask(
            crate::search::providers::bookmarks::ask_for(&target),
        ));
    }
    if let Some(hwnd) = flags.scene.ask_name {
        with_host(|host| host.arrange_action(NAMER_ID, &ArrangeAction::AskName(hwnd)));
    }
    match flags.scene.settings {
        Some(SettingsTarget::Page(page)) => {
            with_host(|host| host.to_ui.send(UiCommand::ShowSettings(page)));
        }
        Some(SettingsTarget::Plugin(id)) => {
            with_host(|host| host.to_ui.send(UiCommand::ShowPluginPage(id)));
        }
        Some(SettingsTarget::HotkeyDialog { id, keys }) => {
            with_host(|host| host.to_ui.send(UiCommand::ShowHotkeyDialog { id, keys }));
        }
        None if flags.open_settings => {
            with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::General)));
        }
        None if flags.reopen == Some(update::Reopen::About) => {
            with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::About)));
        }
        None => {}
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    HOST.with(|cell| *cell.borrow_mut() = None);
    if let Some(thread) = ui_thread {
        wait_for_ui(thread);
    }
    log::info!("WinCraft stopped");
    crate::core::detailed::flush();
}

/// Gives the UI thread a moment to close its windows (the Arrange peek
/// among them) after being told to quit, without ever hanging the exit.
fn wait_for_ui(thread: std::thread::JoinHandle<()>) {
    let start = std::time::Instant::now();
    while !thread.is_finished() && start.elapsed() < UI_STOP_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if thread.is_finished() {
        let _ = thread.join();
        log::info!("the UI thread stopped");
    } else {
        log::warn!("the UI thread did not stop within {UI_STOP_WAIT:?}");
    }
}

#[cfg(test)]
mod tests;
