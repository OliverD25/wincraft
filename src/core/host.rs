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
    ENDSESSION_LOGOFF, MSG, SPI_SETHIGHCONTRAST, SW_SHOWNORMAL, WM_APP, WM_CONTEXTMENU, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP, WM_POWERBROADCAST,
    WM_QUERYENDSESSION, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use serde_json::Value;

use crate::core::attention::Attention;
use crate::core::config::{Config, PluginConfig, UpdateConfig, DEFAULT_PALETTE_HOTKEY};
use crate::core::hotkeys::Shortcut;
use crate::core::shortcuts::{self, FirstPress};
use crate::core::traits::{HostContext, Hotkey, PluginWindow, WinCraftPlugin, WindowGroups};
use crate::core::tray::{show_menu, MenuItem, Tray, WM_TRAY_CALLBACK};
use crate::core::ui_bridge::{
    self, ActionKind, ArrangeAction, ArrangeGroup, ArrangeSnapshot, CommandId, FieldInfo,
    HostCommand, HostRequest, HostSetting, HotkeyInfo, MonitorRect, NewRelease, Page, PaletteEntry,
    PeekCard, PluginInfo, Prompt, SettingsTarget, ShortcutChange, ShortcutInfo, UiChannel,
    UiCommand, UiSnapshot, UpdateStatus, UpdateView, WM_APP_UI,
};
use crate::core::update::{self, Checked, Handover};
use crate::core::update_notice::{self, UpdateNotice};
use crate::core::{
    autostart, chord, config, hotkeys, install, instance, mask_key, release_notes, taskbar,
    taskbar_number, theme, wide,
};
use crate::search::{self, Router, SearchProvider};
use crate::store::github::{self, Release};
use crate::ui;

/// The host window's class, which `--quit` looks for; see instance::name.
pub const CLASS_NAME: &str = "WinCraftHost";

const MENU_QUIT: u32 = 5;
const MENU_PALETTE: u32 = 6;
const MENU_SETTINGS: u32 = 7;
const MENU_UPDATE: u32 = 8;
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
/// comes round every hour and asks GitHub once a day.
const UPDATE_TIMER: usize = 0x5743_0002;
const FIRST_CHECK_MS: u32 = 2 * 60 * 1000;
const CHECK_TIMER_MS: u32 = 60 * 60 * 1000;
/// A version an update started tells the old one it is healthy when this
/// fires.
const HEALTHY_TIMER: usize = 0x5743_0003;
/// Fades the "Updated to" notice, and the message its click posts.
const NOTICE_TIMER: usize = 0x5743_0004;
/// Shows the number badges once Win+Alt+A has waited a moment without a
/// digit (`taskbar_number::REVEAL_MS`).
const BADGE_TIMER: usize = 0x5743_0005;
const WM_APP_NOTICE_CLICK: u32 = WM_APP + 23;
/// Win+Alt+A, then a digit: the message the keyboard hook posts for a key it
/// kept or for the end of the wait.
const WM_APP_WAIT_KEY: u32 = WM_APP + 24;
/// Win and a digit from a keyboard hook that runs on a thread of its own;
/// see `post_taskbar_button`.
const WM_APP_WIN_DIGIT: u32 = WM_APP + 25;
/// The strip Win and a digit asked for counts as open this long before the
/// UI thread has its window, so the same digit again picks the next card
/// instead of opening the strip a second time.
const STRIP_COMING: std::time::Duration = std::time::Duration::from_secs(1);
/// The taskbar's buttons read this recently still count for a digit typed
/// in the strip, which saves reading them again before it can jump.
const BUTTONS_FRESH: std::time::Duration = std::time::Duration::from_secs(10);

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

enum UpdateEvent {
    Checked(Result<Checked, String>),
    /// Percent downloaded, when the server said how much there is.
    Progress(Option<u8>),
    Downloaded(Result<std::path::PathBuf, String>),
    /// The release notes of the installed version, fetched after an update.
    InstalledNotes(String),
}

struct Updates {
    checking: bool,
    /// The check running now was asked for in About, so its failure is
    /// shown; a daily check stays silent when offline.
    manual: bool,
    status: UpdateStatus,
    newer: Option<Release>,
    /// Downloading or swapping; a second Update now waits for it.
    busy: bool,
    /// Why this copy cannot update itself, worked out once at start.
    blocked: Option<String>,
    /// What went wrong with the update a rollback came back from.
    failed: Option<String>,
    /// The version that started this one and waits to hear it is healthy.
    healthy_pending: Option<String>,
    /// Update as soon as the check finds a newer release: About's Update
    /// now, the palette's "Update WinCraft" or a test scene's `--update-now`.
    auto_update: bool,
    /// The palette's "Update WinCraft" started this check, so its answer is
    /// said in a notice whatever it is.
    report: bool,
    /// What About shows under "What's new": the notes of this version.
    installed_notes: Option<String>,
    /// Where the user started the update that is under way, for the next
    /// start to open again. None for the tray menu and for unattended ones.
    reopen: Option<update::Reopen>,
    tx: Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
}

impl Default for Updates {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            checking: false,
            manual: false,
            status: UpdateStatus::Idle,
            newer: None,
            busy: false,
            blocked: None,
            failed: None,
            healthy_pending: None,
            auto_update: false,
            report: false,
            installed_notes: None,
            reopen: None,
            tx,
            rx,
        }
    }
}

/// What the host does after an update check, decided by `Updates::checked`.
#[derive(Debug, Default, PartialEq)]
struct AfterCheck {
    /// GitHub answered; the config remembers when, and what it said.
    answered: bool,
    /// The balloon that says a newer version is there, once per new tag.
    notice: Option<String>,
    /// Read this version's notes again: the newest release is this one.
    reload_notes: bool,
    /// Install the newer version now: Update now started this check.
    update_now: bool,
}

impl Updates {
    fn checked(
        &mut self,
        result: Result<Checked, String>,
        config: &mut UpdateConfig,
        now: u64,
    ) -> AfterCheck {
        self.checking = false;
        let manual = std::mem::take(&mut self.manual);
        let auto_update = std::mem::take(&mut self.auto_update);
        let report = std::mem::take(&mut self.report);
        let mut after = AfterCheck::default();
        let checked = match result {
            Ok(checked) => checked,
            Err(err) => {
                log::info!("update check failed: {err}");
                if manual {
                    self.status = UpdateStatus::Offline;
                }
                if report {
                    after.notice =
                        Some("WinCraft could not check for updates. Is the PC online?".to_string());
                }
                return after;
            }
        };
        after.answered = true;
        config.last_check = now;
        match checked {
            Checked::NoRelease => {
                log::info!("update check: no release is published yet");
                self.status = UpdateStatus::NoRelease;
                self.newer = None;
                if report {
                    after.notice = Some("No WinCraft release is published yet".to_string());
                }
            }
            Checked::UpToDate(tag) => {
                log::info!("update check: {tag} is the newest release, nothing to update");
                self.status = UpdateStatus::UpToDate;
                self.newer = None;
                // The check keeps the notes when the newest release is this one.
                after.reload_notes = true;
                config.latest = tag;
                if report {
                    after.notice = Some(format!(
                        "WinCraft is up to date ({})",
                        env!("CARGO_PKG_VERSION")
                    ));
                }
            }
            Checked::Newer(release) => {
                log::info!("update check: {} is available", release.tag);
                self.status = UpdateStatus::Idle;
                if config.latest != release.tag {
                    after.notice = Some(format!(
                        "WinCraft {} is available \u{2014} open About to update",
                        release.tag.trim_start_matches(['v', 'V'])
                    ));
                    config.latest = release.tag.clone();
                }
                if report && auto_update {
                    after.notice = Some(format!(
                        "Updating WinCraft to {}. It restarts when the new version is in place.",
                        release.tag.trim_start_matches(['v', 'V'])
                    ));
                }
                self.newer = Some(release);
                after.update_now = auto_update;
            }
        }
        after
    }

    /// The version waiting to hear that this one is healthy, taken so it
    /// hears once, and whether the old version's files go afterwards: not
    /// when quitting, nor from a copy that cannot update itself.
    fn healthy_report(&mut self, quitting: bool) -> Option<(String, bool)> {
        let from = self.healthy_pending.take()?;
        Some((from, !quitting && self.blocked.is_none()))
    }
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
    };
    HOST_WINDOW.with(|cell| cell.set(hwnd));
    HOST_HWND.store(hwnd as isize, Ordering::Relaxed);
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
    if update::automatic_checks_allowed() {
        unsafe { SetTimer(hwnd, UPDATE_TIMER, FIRST_CHECK_MS, None) };
    }
    host.start_updates(&flags);

    HOST.with(|cell| *cell.borrow_mut() = Some(host));

    if flags.open_detector {
        open_shortcut_detector();
    }
    if flags.open_palette
        || flags.scene.palette_query.is_some()
        || flags.reopen == Some(update::Reopen::Palette)
    {
        show_palette();
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

impl Host {
    /// Shows the balloons the plugin bookkeeping asked for. Clicking one
    /// opens ShortcutDetector.
    fn show_warnings(&mut self) {
        for text in std::mem::take(&mut self.plugins.warnings) {
            self.balloon_click = BalloonClick::Detector;
            self.tray.balloon("WinCraft", &text);
        }
    }

    fn enable_slot(&mut self, index: usize) {
        self.plugins.enable(index);
        self.show_warnings();
    }

    /// Open palette, Settings, then one primary action per enabled plugin,
    /// then Quit. Everything else lives in the settings window now.
    fn menu_items(&self) -> Vec<MenuItem> {
        let entry = |id: u32, label: &str| MenuItem::Entry {
            id,
            label: label.to_string(),
            checked: false,
        };
        let mut items = vec![
            entry(MENU_PALETTE, "Open palette"),
            entry(MENU_SETTINGS, "Settings\u{2026}"),
        ];
        if let Some(release) = &self.updates.newer {
            items.push(entry(MENU_UPDATE, &format!("Update to {}", release.tag)));
        }
        items.push(MenuItem::Separator);
        let primary: Vec<MenuItem> = self
            .plugins
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.enabled)
            .flat_map(|(index, slot)| {
                let base = MENU_PLUGIN_ACTION_BASE + index as u32 * 100;
                let first = slot
                    .plugin
                    .tray_actions()
                    .into_iter()
                    .next()
                    .map(|action| entry(base + action.id, action.label));
                let toggles =
                    slot.plugin
                        .tray_toggles()
                        .into_iter()
                        .map(move |toggle| MenuItem::Entry {
                            id: base + toggle.id,
                            label: toggle.label.to_string(),
                            checked: toggle.on,
                        });
                first.into_iter().chain(toggles)
            })
            .collect();
        if !primary.is_empty() {
            items.extend(primary);
            items.push(MenuItem::Separator);
        }
        items.push(entry(MENU_QUIT, "Quit WinCraft"));
        items
    }

    fn snapshot(&self) -> UiSnapshot {
        let plugins: Vec<PluginInfo> = (0..self.plugins.slots.len())
            .map(|index| {
                let slot = &self.plugins.slots[index];
                let meta = slot.plugin.metadata();
                let settings = self.plugins.settings_of(meta.id);
                PluginInfo {
                    id: meta.id.to_string(),
                    name: meta.name.to_string(),
                    version: meta.version.to_string(),
                    author: meta.author.to_string(),
                    description: meta.description.to_string(),
                    readme: meta.readme.to_string(),
                    category: meta.category.to_string(),
                    icon: meta.icon.map(str::to_string),
                    enabled: slot.enabled,
                    config_path: config::plugin_path(meta.id),
                    hotkeys: self.plugins.hotkey_infos(index),
                    fields: field_infos(slot.plugin.as_ref(), &settings),
                    status: slot.plugin.status(),
                    page_action: page_button(slot.plugin.as_ref(), index),
                }
            })
            .collect();

        let update = self.update_view();
        let commands = self.commands_with(&plugins);
        UiSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            start_with_windows: self.config.start_with_windows,
            autostart_note: self.autostart_note.clone(),
            theme: self.config.theme,
            mode: self.config.mode,
            palette_hotkey: self.config.palette_hotkey.clone(),
            palette_hotkey_registered: self
                .plugins
                .palette_hotkey
                .map(|(_, ok)| ok)
                .unwrap_or(false),
            config_path: config::config_path(),
            log_path: config::log_path(),
            shortcuts: self.shortcut_infos(&commands),
            commands,
            plugins,
            search: self.config.search.clone(),
            tips: self.config.tips.clone(),
            update,
        }
    }

    fn update_view(&self) -> UpdateView {
        UpdateView {
            check_daily: self.config.updates.check_daily,
            status: self.updates.status.clone(),
            available: self.updates.newer.as_ref().map(|release| NewRelease {
                tag: release.tag.clone(),
                notes: release.notes.clone(),
                page: release.page.clone(),
            }),
            blocked: self.updates.blocked.clone(),
            failed: self.updates.failed.clone(),
            installed_notes: self.updates.installed_notes.clone(),
        }
    }

    /// What this start means for updates: whether this copy may update
    /// itself, whether an update started it, whether a rollback did.
    fn start_updates(&mut self, flags: &StartupFlags) {
        let exe = std::env::current_exe().unwrap_or_default();
        self.updates.blocked = update::blocked(&exe, update::current_package().as_deref());
        if let Some(reason) = &self.updates.blocked {
            log::info!("updates: {reason}");
        }
        if let Some(from) = &flags.updated_from {
            log::info!(
                "started by the update from {from}; it hears that this version is healthy in {} seconds",
                update::HEALTHY_AFTER.as_secs()
            );
            self.updates.healthy_pending = Some(from.clone());
            if !instance::is_test() {
                self.show_updated_notice();
            }
            self.fetch_installed_notes();
            unsafe {
                SetTimer(
                    self.hwnd,
                    HEALTHY_TIMER,
                    update::HEALTHY_AFTER.as_millis() as u32,
                    None,
                )
            };
        } else if self.updates.blocked.is_none() {
            update::remove_leftovers_soon(install::dir());
        }
        if let Some(to) = &flags.update_failed {
            let text = format!("Update to {to} failed, kept {}", env!("CARGO_PKG_VERSION"));
            log::warn!("{text}; the log above says why");
            self.updates.failed = Some(text.clone());
            self.update_notice(&text);
        }
        if flags.scene.update_notice {
            self.show_updated_notice();
        }
        if flags.scene.update_progress {
            self.updates.newer = Some(Release {
                tag: "v9.9.9".to_string(),
                notes: "A sample release: nothing is downloaded.".to_string(),
                page: github::repo_url().to_string(),
                assets: Vec::new(),
            });
            self.updates.busy = true;
            self.updates.status = UpdateStatus::Downloading(Some(42));
            self.publish();
        }
        if flags.scene.update_now {
            self.updates.auto_update = true;
            self.check_for_updates(true);
        }
    }

    /// The version that started this one waits for this; a clean quit
    /// before the timer counts too, since nothing crashed.
    fn report_healthy(&mut self, quitting: bool) {
        unsafe { KillTimer(self.hwnd, HEALTHY_TIMER) };
        let Some((from, tidy)) = self.updates.healthy_report(quitting) else {
            return;
        };
        if update::report_healthy() {
            log::info!("told {from} that this version is healthy");
        } else {
            log::info!("no WinCraft {from} was waiting to hear that this version is healthy");
        }
        if tidy {
            update::remove_leftovers_soon(install::dir());
        }
    }

    /// The palette's "Update WinCraft": choosing it is the consent. It
    /// checks now and installs a newer release at once, by the same path as
    /// About's Update now; any other answer is said in a notice.
    fn update_from_palette(&mut self) {
        if self.updates.busy {
            log::info!("update WinCraft: an update is already running");
            return;
        }
        if let Some(reason) = self.updates.blocked.clone() {
            self.update_notice(&format!("This WinCraft cannot update itself: {reason}"));
            return;
        }
        self.updates.auto_update = true;
        self.updates.report = true;
        self.updates.reopen = Some(update::Reopen::Palette);
        self.check_for_updates(true);
    }

    /// Downloads on its own thread; the swap and the restart follow in
    /// `downloaded`.
    fn update_now(&mut self) {
        if self.updates.busy {
            return;
        }
        if let Some(reason) = self.updates.blocked.clone() {
            self.update_notice(&reason);
            return;
        }
        let Some(release) = self.updates.newer.clone() else {
            self.updates.auto_update = true;
            self.check_for_updates(true);
            return;
        };
        log::info!("downloading {}", release.tag);
        self.updates.busy = true;
        self.updates.status = UpdateStatus::Downloading(None);
        self.publish();
        let tx = self.updates.tx.clone();
        let hwnd = self.hwnd as isize;
        let dir = install::dir();
        let local_fake = github::releases_override().is_some();
        let started = std::thread::Builder::new()
            .name("wincraft-update-download".to_string())
            .spawn(move || {
                let post = |event: UpdateEvent| {
                    if tx.send(event).is_ok() {
                        unsafe { PostMessageW(hwnd as HWND, WM_APP_UPDATE, 0, 0) };
                    }
                };
                let mut shown = None;
                let mut progress = |done: u64, total: Option<u64>| {
                    let percent = total
                        .filter(|total| *total > 0)
                        .map(|total| (done.saturating_mul(100) / total).min(100) as u8);
                    if shown != Some(percent) {
                        shown = Some(percent);
                        post(UpdateEvent::Progress(percent));
                    }
                };
                let result =
                    update::download(&release, &dir, &update::Web, local_fake, &mut progress);
                post(UpdateEvent::Downloaded(result));
            });
        if let Err(err) = started {
            log::warn!("could not start the download: {err}");
            self.updates.busy = false;
            self.updates.status =
                UpdateStatus::Error(format!("Could not start the download: {err}"));
        }
    }

    fn downloaded(&mut self, result: Result<std::path::PathBuf, String>) {
        let tag = self
            .updates
            .newer
            .as_ref()
            .map(|release| release.tag.clone())
            .unwrap_or_default();
        if let Err(err) = result {
            log::warn!("the update to {tag} was not installed: {err}");
            self.updates.busy = false;
            self.updates.reopen = None;
            self.updates.status = UpdateStatus::Error(err);
            return;
        }
        log::info!("{tag} is downloaded and matches its checksum");
        if let Some(release) = &self.updates.newer {
            release_notes::store(&release.tag, &release.notes);
        }
        self.updates.status = UpdateStatus::Installing;
        self.publish();
        let dir = install::dir();
        if let Err(err) = update::swap_in(&dir, &mut update::Disk) {
            log::error!("could not put {tag} in place: {err}");
            self.updates.busy = false;
            self.updates.reopen = None;
            self.updates.status =
                UpdateStatus::Error(format!("Could not put the new version in place: {err}"));
            return;
        }
        log::info!("{tag} is in place; WinCraft closes and starts it");
        update::hand_over_at_exit(Handover {
            dir,
            from: env!("CARGO_PKG_VERSION").to_string(),
            to: tag,
            reopen: self.updates.reopen.take(),
        });
        self.shutdown();
    }

    /// On its own thread: GitHub can take seconds to answer, or never.
    fn check_for_updates(&mut self, manual: bool) {
        if self.updates.busy {
            return;
        }
        if manual {
            self.updates.manual = true;
            self.updates.status = UpdateStatus::Checking;
            self.publish();
        }
        if self.updates.checking {
            return;
        }
        self.updates.checking = true;
        let tx = self.updates.tx.clone();
        let hwnd = self.hwnd as isize;
        let started = std::thread::Builder::new()
            .name("wincraft-update-check".to_string())
            .spawn(move || {
                let result = update::check(env!("CARGO_PKG_VERSION"));
                if tx.send(UpdateEvent::Checked(result)).is_ok() {
                    unsafe { PostMessageW(hwnd as HWND, WM_APP_UPDATE, 0, 0) };
                }
            });
        if let Err(err) = started {
            log::warn!("could not start the update check: {err}");
            self.updates.checking = false;
        }
    }

    /// The hourly timer: asks GitHub when a day has passed since it last
    /// answered.
    fn daily_check(&mut self) {
        unsafe { SetTimer(self.hwnd, UPDATE_TIMER, CHECK_TIMER_MS, None) };
        let updates = &self.config.updates;
        if update::check_due(update::now_secs(), updates.last_check, updates.check_daily) {
            self.check_for_updates(false);
        }
    }

    /// The panel that says the update worked. A click opens About.
    fn show_updated_notice(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        log::info!("update notice: Updated to {version}");
        self.close_update_notice();
        match UpdateNotice::show(version, self.hwnd, WM_APP_NOTICE_CLICK) {
            Ok(notice) => {
                self.update_notice = Some(notice);
                unsafe { SetTimer(self.hwnd, NOTICE_TIMER, update_notice::TICK_MS, None) };
            }
            Err(err) => log::warn!("could not show the update notice: {err}"),
        }
    }

    fn notice_tick(&mut self) {
        let showing = self.update_notice.as_mut().is_some_and(UpdateNotice::tick);
        if !showing {
            self.close_update_notice();
        }
    }

    fn close_update_notice(&mut self) {
        unsafe { KillTimer(self.hwnd, NOTICE_TIMER) };
        if let Some(mut notice) = self.update_notice.take() {
            notice.close();
        }
    }

    /// The notes of the version that just started, for About: from the file
    /// the update kept, else from GitHub on a thread of its own.
    fn fetch_installed_notes(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        self.updates.installed_notes = release_notes::load(version);
        if self.updates.installed_notes.is_some() || !update::automatic_checks_allowed() {
            return;
        }
        let tx = self.updates.tx.clone();
        let hwnd = self.hwnd as isize;
        let started = std::thread::Builder::new()
            .name("wincraft-release-notes".to_string())
            .spawn(
                move || match github::release_by_tag(&format!("v{version}")) {
                    Ok(Some(release)) => {
                        release_notes::store(&release.tag, &release.notes);
                        if !release.notes.trim().is_empty()
                            && tx.send(UpdateEvent::InstalledNotes(release.notes)).is_ok()
                        {
                            unsafe { PostMessageW(hwnd as HWND, WM_APP_UPDATE, 0, 0) };
                        }
                    }
                    Ok(None) => log::info!("no release notes are published for v{version}"),
                    Err(err) => {
                        log::info!("the release notes of v{version} could not be fetched: {err}")
                    }
                },
            );
        if let Err(err) = started {
            log::warn!("could not start the release notes fetch: {err}");
        }
    }

    /// A balloon that opens About when clicked. A test instance only logs
    /// it: a fake release must never put a notice on the user's screen.
    fn update_notice(&mut self, text: &str) {
        log::info!("update notice: {text}");
        if !instance::is_test() {
            self.balloon_click = BalloonClick::About;
            self.tray.balloon("WinCraft", text);
        }
    }

    fn update_events(&mut self) {
        while let Ok(event) = self.updates.rx.try_recv() {
            match event {
                UpdateEvent::Checked(result) => self.checked(result),
                UpdateEvent::Progress(percent) => {
                    if self.updates.busy {
                        self.updates.status = UpdateStatus::Downloading(percent);
                    }
                }
                UpdateEvent::Downloaded(result) => self.downloaded(result),
                UpdateEvent::InstalledNotes(notes) => self.updates.installed_notes = Some(notes),
            }
        }
        self.publish();
    }

    fn checked(&mut self, result: Result<Checked, String>) {
        let after = self
            .updates
            .checked(result, &mut self.config.updates, update::now_secs());
        if after.reload_notes {
            self.updates.installed_notes = release_notes::load(env!("CARGO_PKG_VERSION"));
        }
        if let Some(text) = after.notice {
            self.update_notice(&text);
        }
        if !after.update_now && !self.updates.busy {
            self.updates.reopen = None;
        }
        if after.update_now {
            self.update_now();
        }
        if after.answered {
            self.save_config();
        }
    }

    fn search_router(&self) -> Router {
        let mut providers: Vec<(Option<String>, Box<dyn SearchProvider>)> =
            search::providers::built_in()
                .into_iter()
                .map(|provider| (None, provider))
                .collect();
        for slot in &self.plugins.slots {
            let id = slot.plugin.metadata().id;
            for provider in slot.plugin.search_providers() {
                providers.push((Some(id.to_string()), provider));
            }
        }
        let mut router = Router::new(providers, &self.config.search.prefixes);
        router.load_usage(config::data_dir().join("palette_usage.json"));
        router
    }

    /// Every plugin's own windows, for the UI thread, each with its plugin.
    fn plugin_windows(&self) -> Vec<(String, Box<dyn PluginWindow>)> {
        self.plugins
            .slots
            .iter()
            .flat_map(|slot| {
                let id = slot.plugin.metadata().id;
                slot.plugin
                    .windows()
                    .into_iter()
                    .map(move |window| (id.to_string(), window))
            })
            .collect()
    }

    /// Every change that can switch a plugin on or off, or change a
    /// shortcut, ends here, so the shortcuts are registered again first.
    fn publish(&mut self) {
        self.register_shortcuts();
        self.to_ui
            .send(UiCommand::Snapshot(Box::new(self.snapshot())));
    }

    /// The commands with a shortcut the host registers itself, of WinCraft
    /// and of the plugins that are on.
    fn bound_shortcuts(&self) -> Vec<(Shortcut, CommandId)> {
        let mut bound = self.plugins.plugin_chords();
        for entry in self.plugins.palette_entries(&[]) {
            let hotkey = matches!(
                entry.id,
                CommandId::Plugin {
                    kind: ActionKind::Hotkey,
                    ..
                }
            );
            if entry.disabled || hotkey {
                continue;
            }
            let keys = self
                .config
                .shortcuts
                .get(&entry.shortcut_id)
                .and_then(|text| shortcuts::parse(text));
            if let Some(keys) = keys {
                bound.push((keys, entry.id));
            }
        }
        bound
    }

    /// Registers the first presses the shortcuts need, when they changed.
    fn register_shortcuts(&mut self) {
        let plan = shortcuts::plan(&self.bound_shortcuts());
        let same = plan.len() == self.shortcut_keys.len()
            && plan
                .iter()
                .zip(&self.shortcut_keys)
                .all(|(wanted, (_, have, _))| wanted == have);
        if same {
            return;
        }
        for (id, _, ok) in self.shortcut_keys.drain(..) {
            if ok {
                self.plugins.keys.unregister(id);
            }
        }
        for (offset, press) in plan.into_iter().enumerate() {
            let id = SHORTCUT_ID_BASE + offset as i32;
            let keys = hotkeys::format(press.hotkey);
            let ok = self.plugins.keys.register(id, press.hotkey);
            if ok {
                log::info!("registered {keys} for a shortcut");
            } else {
                let taken = self.plugins.taken_message(press.hotkey);
                log::warn!("could not register {keys} for a shortcut: {taken}");
            }
            self.shortcut_keys.push((id, press, ok));
        }
    }

    /// A shortcut's first press came: run its command, or wait three
    /// seconds for the second press of a chord.
    fn shortcut_pressed(&mut self, global_id: i32) {
        let Some((_, press, _)) = self
            .shortcut_keys
            .iter()
            .find(|(id, _, _)| *id == global_id)
            .cloned()
        else {
            return;
        };
        self.end_wait();
        if press.seconds.is_empty() {
            if let Some(command) = press.plain {
                self.run_command(command);
            }
            return;
        }
        let keys = hotkeys::format(press.hotkey);
        CHORD_PRESS.with(|cell| *cell.borrow_mut() = Some(press.clone()));
        if !chord::listen(
            self.hwnd as isize,
            WM_APP_WAIT_KEY,
            shortcuts::CHORD_WAIT,
            chord_rule,
        ) {
            CHORD_PRESS.with(|cell| cell.borrow_mut().take());
            return;
        }
        log::info!("{keys}: waiting 3 s for the second key of a chord");
        self.chord_wait = Some(press);
        self.wait_hint = Some(ui::taskbar_badges::WaitHint::line_only(
            &shortcuts::wait_line(&keys),
        ));
    }

    /// Applies the Hotkeys page's changes in order, so Replace
    /// frees the keys before the new command takes them.
    fn set_shortcuts(&mut self, changes: Vec<ShortcutChange>) {
        let infos = self.shortcut_infos(&self.commands());
        for change in changes {
            let Some(info) = infos.iter().find(|info| info.id == change.id) else {
                log::warn!("no command {} for a shortcut", change.id);
                continue;
            };
            let text = change
                .binding
                .clone()
                .unwrap_or_else(|| info.default_binding.clone());
            let text = match shortcuts::parse(&text) {
                Some(keys) => hotkeys::format_shortcut(&keys),
                None => String::new(),
            };
            match info.command {
                None => {
                    // The palette always has a hotkey: removing it gives
                    // the default back.
                    let text = if text.is_empty() {
                        DEFAULT_PALETTE_HOTKEY.to_string()
                    } else {
                        text
                    };
                    match self.plugins.rebind_palette(&text) {
                        Ok(text) => self.config.palette_hotkey = text,
                        Err(err) => log::warn!("palette hotkey: {err}"),
                    }
                }
                Some(CommandId::Plugin {
                    index,
                    kind: ActionKind::Hotkey,
                    action,
                }) => {
                    let name = self.plugins.action_name(index, action);
                    if let Some(plugin) = &info.plugin {
                        self.plugins.set_shortcut_text(plugin, name, &text);
                    }
                }
                Some(_) => {
                    if text.is_empty() {
                        self.config.shortcuts.remove(&info.id);
                    } else {
                        self.config.shortcuts.insert(info.id.clone(), text.clone());
                    }
                    log::info!("{} shortcut set to \"{text}\"", info.id);
                }
            }
        }
        self.show_warnings();
        self.save_config();
        self.publish();
    }

    /// The palette's commands, with the shortcuts the host keeps filled in.
    fn commands(&self) -> Vec<PaletteEntry> {
        let plugins: Vec<PluginInfo> = (0..self.plugins.slots.len())
            .map(|index| PluginInfo {
                hotkeys: self.plugins.hotkey_infos(index),
                ..PluginInfo::default()
            })
            .collect();
        self.commands_with(&plugins)
    }

    fn commands_with(&self, plugins: &[PluginInfo]) -> Vec<PaletteEntry> {
        let mut entries = self.plugins.palette_entries(plugins);
        announce_update(&mut entries, &self.update_view());
        for entry in &mut entries {
            let hotkey = matches!(
                entry.id,
                CommandId::Plugin {
                    kind: ActionKind::Hotkey,
                    ..
                }
            );
            if !hotkey {
                entry.shortcut = self
                    .config
                    .shortcuts
                    .get(&entry.shortcut_id)
                    .and_then(|text| shortcuts::parse(text))
                    .map(|keys| hotkeys::format_shortcut(&keys))
                    .unwrap_or_default();
            }
        }
        entries
    }

    /// The rows of the Hotkeys page: the palette hotkey, then
    /// every command the palette lists.
    fn shortcut_infos(&self, commands: &[PaletteEntry]) -> Vec<ShortcutInfo> {
        let mut infos = vec![ShortcutInfo {
            id: "wincraft.palette".to_string(),
            command: None,
            group: shortcuts::HOST_GROUP.to_string(),
            plugin: None,
            category: String::new(),
            icon: None,
            label: "Open the palette".to_string(),
            description: "Search apps, windows, files and commands".to_string(),
            binding: self.config.palette_hotkey.clone(),
            default_binding: DEFAULT_PALETTE_HOTKEY.to_string(),
            registered: self.plugins.palette_hotkey.is_none_or(|(_, ok)| ok),
            one_press: true,
        }];
        for entry in commands {
            let index = match entry.id {
                CommandId::Plugin { index, .. } => Some(index),
                CommandId::Host(HostCommand::TogglePlugin(index)) => Some(index),
                CommandId::Host(_) => None,
            };
            let meta = index
                .and_then(|index| self.plugins.slots.get(index))
                .map(|slot| slot.plugin.metadata());
            let category = meta
                .as_ref()
                .map(|meta| meta.category.to_string())
                .unwrap_or_default();
            let icon = meta.and_then(|meta| meta.icon).map(str::to_string);
            let (default_binding, registered) = match entry.id {
                CommandId::Plugin {
                    index,
                    kind: ActionKind::Hotkey,
                    action,
                } => {
                    let name = self.plugins.action_name(index, action);
                    self.plugins
                        .hotkey_infos(index)
                        .into_iter()
                        .find(|info| info.action == name)
                        .map_or((String::new(), true), |info| {
                            (info.default_binding, info.registered)
                        })
                }
                _ => {
                    let first = shortcuts::parse(&entry.shortcut).map(|keys| keys.first);
                    let ok = first.is_none_or(|first| {
                        self.shortcut_keys
                            .iter()
                            .find(|(_, press, _)| hotkeys::same(press.hotkey, first))
                            .is_none_or(|(_, _, ok)| *ok)
                    });
                    (String::new(), ok)
                }
            };
            infos.push(ShortcutInfo {
                id: entry.shortcut_id.clone(),
                command: Some(entry.id),
                group: entry.group.clone(),
                plugin: entry.plugin.clone(),
                category,
                icon,
                label: entry.label.clone(),
                description: entry.subtitle.clone().unwrap_or_default(),
                binding: entry.shortcut.clone(),
                default_binding,
                registered,
                one_press: false,
            });
        }
        infos
    }

    /// Asks an enabled plugin that keeps a window order for its groups, or
    /// else the strip's own plugin, and sends them to the Arrange window.
    fn send_arrange(&mut self, open: bool) {
        self.send_arrange_for(open, false);
    }

    /// `send_arrange`, but with `need_app` nothing is sent unless one of the
    /// groups is the anchor's app. Returns whether the strip was sent.
    fn send_arrange_for(&mut self, open: bool, need_app: bool) -> bool {
        let anchor = if open {
            self.arrange_anchor.take()
        } else {
            None
        };
        let look = self.plugins.slots.iter().find_map(|slot| {
            if !slot.enabled {
                return None;
            }
            let look = slot.plugin.strip_look()?;
            Some((slot.plugin.metadata().id, look))
        });
        let Some((look_plugin, look)) = look else {
            if open {
                log::info!("WindowDeck is off, so the strip does not open");
            }
            return false;
        };
        // Read before the plugin builds its groups, so the mouse is still
        // where it was when the hotkey came.
        let hovered = match (&anchor, open) {
            (Some((app, _)), _) => Some(app.clone()),
            (None, true) => taskbar::app_under_cursor(),
            (None, false) => None,
        };
        let Some((id, mut groups)) = self.plugins.window_groups(look_plugin) else {
            log::info!("no enabled plugin gives the strip its windows");
            return false;
        };
        let under_cursor = hovered.as_deref().and_then(|app| {
            groups
                .groups
                .iter()
                .position(|group| taskbar::same_app(app, &group.key))
        });
        if need_app && under_cursor.is_none() {
            return false;
        }
        ATTENTION.with(|attention| {
            let attention = attention.borrow();
            for window in groups.groups.iter_mut().flat_map(|g| g.windows.iter_mut()) {
                window.attention = attention.wants(window.hwnd);
            }
        });
        let mut focus = groups.focus;
        if open {
            let asked = self.arrange_focus.take().and_then(|wanted| {
                let found = ArrangeGroup::pick(&groups.groups, &wanted);
                if found.is_none() {
                    log::info!("the scene asked for group \"{wanted}\", which is not open");
                }
                found
            });
            let rule = match (asked, under_cursor) {
                (Some(index), _) => {
                    focus = index;
                    "asked for by the test scene"
                }
                (None, Some(index)) => {
                    focus = index;
                    if anchor.is_some() {
                        "mouse resting on its taskbar button"
                    } else {
                        "taskbar button under cursor"
                    }
                }
                (None, None) => "front window",
            };
            match groups.groups.get(focus) {
                Some(group) => log::info!("arrange opened on {} ({rule})", group.label),
                None => log::info!("arrange opened with no app windows"),
            }
        }
        let snapshot = ArrangeSnapshot {
            plugin: id.to_string(),
            groups: groups.groups,
            desktops: groups.desktops,
            focus,
            watched: groups.watched,
            preview: look.preview,
            can_rename: self.plugins.is_on(NAMER_ID),
            monitors: groups.monitors,
            view: look.view,
            look_plugin: look_plugin.to_string(),
            keeps_order: id != look_plugin,
            anchor: anchor.map(|(_, rect)| rect),
        };
        self.to_ui.send(if open {
            UiCommand::ShowArrange(snapshot)
        } else {
            UiCommand::ArrangeUpdate(snapshot)
        });
        if open {
            if let Some(card) = self.peek_card.take() {
                self.to_ui.send(UiCommand::PeekCard(card));
            }
        }
        true
    }

    /// A digit typed in the strip, or with Win+Alt still held after
    /// Win+Alt+A: the strip for that taskbar button.
    fn arrange_digit(&mut self, digit: u8) {
        let started = std::time::Instant::now();
        let now = self.clock.elapsed().as_millis() as u64;
        // The badges show the buttons read when the wait started, and the
        // digits must count those; a second digit counts the same list.
        let fresh = self
            .buttons_at
            .is_some_and(|at| at.elapsed() < BUTTONS_FRESH);
        if !(self.wait_hint.is_some()
            || self.reveal.pending()
            || self.digits.may_follow(now)
            || fresh)
        {
            self.read_buttons();
        }
        let number = self.digits.press(digit, now, self.buttons.len());
        // The badges go at once, or never come; the hook stays only as long
        // as a second digit typed with Win+Alt still held could follow.
        self.stop_reveal();
        self.wait_hint = None;
        if self.digits.may_follow(now) {
            let wait = std::time::Duration::from_millis(taskbar_number::SECOND_DIGIT_MS);
            chord::listen(self.hwnd as isize, WM_APP_WAIT_KEY, wait, wait_rule);
        } else {
            chord::stop_for(self.hwnd as isize);
        }
        self.open_button(number);
        log::info!(
            "digit {digit}: the strip for taskbar button {number} was asked for {} ms after the digit came",
            started.elapsed().as_millis()
        );
    }

    fn read_buttons(&mut self) {
        self.buttons = taskbar::main_buttons().unwrap_or_else(|reason| {
            log::info!("the taskbar's buttons could not be read: {reason}");
            Vec::new()
        });
        self.buttons_at = Some(std::time::Instant::now());
    }

    /// The Arrange strip's hotkey. The first press waits 3 s for a digit,
    /// with a number over each taskbar button; nothing opens yet. A second
    /// press during the wait opens the strip for the window in front, with
    /// that window picked.
    fn arrange_hotkey(&mut self, hotkey: Hotkey, keys: &str) {
        if self.chord_wait.is_some() {
            self.end_wait();
        }
        if self.wait_hint.is_some() || chord::waiting_for(self.hwnd as isize) {
            self.end_wait();
            let front = unsafe { GetForegroundWindow() } as isize;
            log::info!("the Arrange hotkey twice: the strip for the window in front");
            self.arrange_anchor = None;
            self.send_arrange(true);
            self.to_ui.send(UiCommand::ArrangeSelectWindow(front));
            return;
        }
        self.read_buttons();
        self.digits = taskbar_number::Digits::default();
        WAIT_HOTKEY.with(|cell| cell.set(Some(hotkey)));
        let wait = std::time::Duration::from_millis(taskbar_number::WAIT_MS);
        if !chord::listen(self.hwnd as isize, WM_APP_WAIT_KEY, wait, wait_rule) {
            // No hook: the digits cannot be caught, so the strip opens at
            // once, as the hotkey did before.
            self.arrange_anchor = None;
            self.send_arrange(true);
            return;
        }
        log::info!(
            "the Arrange hotkey: waiting 3 s for a taskbar number ({} buttons)",
            self.buttons.len()
        );
        self.reveal = taskbar_number::Reveal::start(self.now_ms());
        self.reveal_line = taskbar_number::wait_line(keys);
        unsafe {
            SetTimer(
                self.hwnd,
                BADGE_TIMER,
                taskbar_number::REVEAL_MS as u32,
                None,
            )
        };
    }

    fn now_ms(&self) -> u64 {
        self.clock.elapsed().as_millis() as u64
    }

    /// The badge timer came round: the badges and the line show if the
    /// wait still runs and no digit came.
    fn reveal_tick(&mut self) {
        unsafe { KillTimer(self.hwnd, BADGE_TIMER) };
        let now = self.now_ms();
        if !chord::waiting_for(self.hwnd as isize) {
            self.reveal.cancel();
            return;
        }
        if self.reveal.take_due(now) {
            log::info!(
                "no digit after {} ms: the badges show",
                taskbar_number::REVEAL_MS
            );
            self.wait_hint = Some(ui::taskbar_badges::WaitHint::show(
                &self.buttons,
                &self.reveal_line,
            ));
        } else if let Some(left) = self.reveal.left(now) {
            unsafe { SetTimer(self.hwnd, BADGE_TIMER, left.max(1) as u32, None) };
        }
    }

    fn stop_reveal(&mut self) {
        if self.reveal.pending() {
            self.reveal.cancel();
            unsafe { KillTimer(self.hwnd, BADGE_TIMER) };
        }
    }

    /// The wait after the Arrange hotkey is over: the badges and the line go,
    /// and a first digit is no longer joined by a second one.
    fn end_wait(&mut self) {
        chord::stop_for(self.hwnd as isize);
        self.chord_wait = None;
        CHORD_PRESS.with(|cell| cell.borrow_mut().take());
        self.stop_reveal();
        self.wait_hint = None;
        self.digits = taskbar_number::Digits::default();
    }

    /// Keys the hook kept during the wait, or the news that it ended.
    fn wait_news(&mut self, news: chord::News) {
        if let Some(press) = self.chord_wait.clone() {
            for stroke in news.taken {
                match shortcuts::chord_key(&press, stroke) {
                    shortcuts::ChordKey::Run(command) => {
                        self.end_wait();
                        log::info!(
                            "chord {}: second key matched",
                            hotkeys::format(press.hotkey)
                        );
                        self.run_command(command);
                        return;
                    }
                    shortcuts::ChordKey::Cancel => {
                        self.end_wait();
                        return;
                    }
                    shortcuts::ChordKey::Other => {}
                }
            }
            if news.ended {
                self.end_wait();
            }
            return;
        }
        let hotkey = WAIT_HOTKEY.with(|cell| cell.get());
        for stroke in news.taken {
            let key = hotkey.map_or(taskbar_number::WaitKey::Other, |hotkey| {
                taskbar_number::wait_key(stroke, hotkey)
            });
            match key {
                taskbar_number::WaitKey::Digit(digit) => self.arrange_digit(digit),
                taskbar_number::WaitKey::Cancel => self.end_wait(),
                taskbar_number::WaitKey::Again | taskbar_number::WaitKey::Other => {}
            }
        }
        if news.ended {
            self.end_wait();
        }
    }

    /// Opens the strip above taskbar button `number` with its window
    /// picked, where the hover strip would open for that button. A number
    /// with no button does nothing.
    fn open_button(&mut self, number: usize) {
        let Some((button, place)) = taskbar_number::button_number(&self.buttons, number) else {
            log::info!("taskbar button {number} does not exist");
            return;
        };
        let (app, rect) = (button.app.clone(), button.rect);
        self.arrange_anchor = Some((app.clone(), rect));
        // One look at the windows: the strip opens only if the app has some.
        if !self.send_arrange_for(true, true) {
            log::info!("taskbar button {number} ({app}) has no windows the strip shows");
            self.to_ui
                .send(UiCommand::ArrangeNote(taskbar_number::no_windows(number)));
            return;
        }
        log::info!("arrange opens for taskbar button {number} ({app}), window {place}");
        self.to_ui.send(UiCommand::ArrangeSelect(place));
    }

    /// Win and a digit, kept by WindowDeck's hook: the strip for that
    /// taskbar button with its window picked, or the next card when the
    /// same digit comes again while Win stayed down. The caller hands a
    /// `GiveBack` to Windows.
    fn win_digit(&mut self, digit: u8, again: bool) -> WinDigit {
        let coming = self
            .win_digit_opened
            .is_some_and(|at| at.elapsed() < STRIP_COMING);
        if again && (coming || ui_bridge::arrange_window().is_some()) {
            log::info!("Win+{digit} again: the next card");
            self.to_ui.send(UiCommand::ArrangeSelectNext);
            return WinDigit::NextCard;
        }
        let fresh = self
            .buttons_at
            .is_some_and(|at| at.elapsed() < BUTTONS_FRESH);
        if !fresh {
            self.read_buttons();
        }
        let number = if digit == 0 { 10 } else { usize::from(digit) };
        let Some((button, place)) = taskbar_number::button_number(&self.buttons, number) else {
            log::info!("Win+{digit}: taskbar button {number} does not exist; Windows has it");
            return WinDigit::GiveBack;
        };
        let (app, rect) = (button.app.clone(), button.rect);
        self.arrange_anchor = Some((app.clone(), rect));
        if !self.send_arrange_for(true, true) {
            self.arrange_anchor = None;
            log::info!("Win+{digit}: {app} has no windows the strip shows; Windows has it");
            return WinDigit::GiveBack;
        }
        log::info!("Win+{digit}: the strip for taskbar button {number} ({app}), window {place}");
        self.to_ui.send(UiCommand::ArrangeSelect(place));
        self.win_digit_opened = Some(std::time::Instant::now());
        WinDigit::Opened
    }

    /// Asks the first enabled plugin that keeps a layout for its saved
    /// windows and sends them to the Layout page.
    fn send_layout(&mut self) {
        let can_rename = self.plugins.is_on(NAMER_ID);
        let snapshot = self.plugins.slots.iter_mut().find_map(|slot| {
            if !slot.enabled {
                return None;
            }
            slot.plugin.layout_snapshot()
        });
        self.to_ui
            .send(UiCommand::LayoutUpdate(snapshot.map(|mut s| {
                s.can_rename = can_rename;
                s
            })));
    }

    fn run_command(&mut self, id: CommandId) {
        match id {
            CommandId::Host(HostCommand::OpenSettings) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::General))
            }
            CommandId::Host(HostCommand::OpenStore) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::Store))
            }
            CommandId::Host(HostCommand::OpenLayout) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::Layout))
            }
            CommandId::Host(HostCommand::OpenAbout) => {
                self.to_ui.send(UiCommand::ShowSettings(Page::About))
            }
            CommandId::Host(HostCommand::OpenConfig) => open_in_notepad(&config::config_path()),
            CommandId::Host(HostCommand::OpenLog) => open_in_notepad(&config::log_path()),
            CommandId::Host(HostCommand::UpdateWinCraft) => self.update_from_palette(),
            CommandId::Host(HostCommand::Exit) => self.shutdown(),
            CommandId::Host(HostCommand::TogglePlugin(index)) => {
                if index < self.plugins.slots.len() {
                    if self.plugins.slots[index].enabled {
                        self.plugins.disable(index);
                    } else {
                        self.enable_slot(index);
                    }
                    self.publish();
                }
            }
            CommandId::Plugin {
                index,
                kind,
                action,
            } => {
                let Some(slot) = self.plugins.slots.get_mut(index) else {
                    return;
                };
                if !slot.enabled {
                    return;
                }
                match kind {
                    ActionKind::Hotkey => {
                        // From the palette, not the keyboard: the action runs
                        // at once, without the hotkey's wait for a digit.
                        LAST_HOTKEY.with(|cell| cell.borrow_mut().take());
                        slot.plugin.on_hotkey(action)
                    }
                    ActionKind::Tray => slot.plugin.on_tray_action(action),
                    ActionKind::Palette => slot.plugin.on_palette_command(action),
                }
            }
        }
    }

    fn handle_request(&mut self, request: HostRequest) {
        match request {
            HostRequest::UiReady { palette_hwnd } => {
                self.palette_hwnd = palette_hwnd as HWND;
                clear_system_frame(self.palette_hwnd);
                self.publish();
            }
            HostRequest::RunCommand(id) => self.run_command(id),
            HostRequest::RunAction(search::Action::Command(id)) => self.run_command(id),
            HostRequest::RunAction(search::Action::Arrange(hwnd)) => {
                if self.send_arrange_for(true, false) {
                    self.to_ui.send(UiCommand::ArrangeSelectWindow(hwnd));
                }
            }
            HostRequest::RunAction(action) => search::actions::perform(&action, self.hwnd),
            HostRequest::PromptAnswer {
                plugin,
                id,
                target,
                text,
            } => match self.plugins.index_of(&plugin) {
                Some(index) if self.plugins.slots[index].enabled => self.plugins.slots[index]
                    .plugin
                    .on_prompt_answer(id, target, &text),
                _ => log::info!("{plugin} is off; the answer to its question was dropped"),
            },
            HostRequest::Arrange { plugin, action } => {
                let owner = if action.is_about_names() {
                    NAMER_ID
                } else {
                    plugin.as_str()
                };
                self.arrange_action(owner, &action);
                if !matches!(action, ArrangeAction::Activate(_)) {
                    self.send_arrange(false);
                }
            }
            HostRequest::LayoutRefresh => self.send_layout(),
            HostRequest::Layout { plugin, action } => {
                match self.plugins.index_of(&plugin) {
                    Some(index) if self.plugins.slots[index].enabled => {
                        self.plugins.slots[index].plugin.on_layout_action(&action)
                    }
                    _ => log::info!("{plugin} is off; the Layout page's request was dropped"),
                }
                self.send_layout();
            }
            HostRequest::FocusWindow(hwnd) => bring_to_front(hwnd as HWND),
            HostRequest::ArrangeDigit(digit) => self.arrange_digit(digit),
            HostRequest::ClearPaletteUsage => self.to_ui.send(UiCommand::ClearPaletteUsage),
            HostRequest::SetPluginEnabled { id, enabled } => {
                if let Some(index) = self.plugins.index_of(&id) {
                    if enabled {
                        self.enable_slot(index);
                    } else {
                        self.plugins.disable(index);
                    }
                    self.publish();
                }
            }
            HostRequest::SetHotkey {
                plugin,
                action,
                binding,
            } => self.set_hotkey(&plugin, &action, &binding),
            HostRequest::SetSetting { plugin, key, value } => {
                self.set_setting(&plugin, &key, value)
            }
            HostRequest::SetShortcuts(changes) => self.set_shortcuts(changes),
            HostRequest::ResetPlugin(id) => self.reset_plugin(&id),
            HostRequest::SetHostSetting(setting) => self.set_host_setting(setting),
            HostRequest::CheckForUpdates => self.check_for_updates(true),
            HostRequest::UpdateNow => {
                if !self.updates.busy && self.updates.blocked.is_none() {
                    self.updates.reopen = Some(update::Reopen::About);
                }
                self.update_now();
            }
            HostRequest::OpenPath(path) => open_in_notepad(&path),
            HostRequest::Exit => self.shutdown(),
        }
    }

    fn arrange_action(&mut self, plugin: &str, action: &ArrangeAction) {
        match self.plugins.index_of(plugin) {
            Some(index) if self.plugins.slots[index].enabled => {
                self.plugins.slots[index].plugin.on_arrange_action(action)
            }
            _ => log::info!("{plugin} is off; the strip's request was dropped"),
        }
    }

    fn set_hotkey(&mut self, plugin_id: &str, action: &str, binding: &str) {
        self.plugins.set_hotkey(plugin_id, action, binding);
        self.show_warnings();
        self.publish();
    }

    fn set_setting(&mut self, plugin_id: &str, key: &str, value: Value) {
        self.plugins.set_setting(plugin_id, key, value);
        self.show_warnings();
        self.publish();
    }

    fn reset_plugin(&mut self, plugin_id: &str) {
        if self.plugins.reset(plugin_id) {
            self.show_warnings();
            self.publish();
        }
    }

    fn set_host_setting(&mut self, setting: HostSetting) {
        match setting {
            HostSetting::StartWithWindows(wanted) => match autostart::set(wanted) {
                Ok(()) => {
                    self.config.start_with_windows = wanted;
                    log::info!("start with Windows: {wanted}");
                }
                Err(err) => log::error!("could not change autostart: {err}"),
            },
            HostSetting::Search(search) => self.config.search = search,
            HostSetting::Tips(tips) => self.config.tips = tips,
            HostSetting::CheckUpdatesDaily(on) => {
                self.config.updates.check_daily = on;
                log::info!("check for updates daily: {on}");
            }
            HostSetting::Theme(name) => {
                self.config.theme = name;
                theme::set_current(name, self.config.mode);
                log::info!("theme: {}", name.label());
                self.to_ui.send(UiCommand::ThemeChanged);
            }
            HostSetting::Mode(mode) => {
                self.config.mode = mode;
                theme::set_current(self.config.theme, mode);
                self.to_ui.send(UiCommand::ThemeChanged);
            }
            HostSetting::PaletteHotkey(binding) => match self.plugins.rebind_palette(&binding) {
                Ok(text) => {
                    self.config.palette_hotkey = text;
                    self.show_warnings();
                }
                Err(err) => log::warn!("palette hotkey: {err}"),
            },
        }
        self.save_config();
        self.publish();
    }

    fn save_config(&self) {
        if let Err(err) = self.config.save_in(&self.plugins.data) {
            log::error!("could not save config.json: {err}");
        }
    }

    fn shutdown(&mut self) {
        log::info!("WinCraft is shutting down");
        self.report_healthy(true);
        self.to_ui.send(UiCommand::Quit);
        self.plugins.stop_all();
        self.end_wait();
        for (id, _, ok) in self.shortcut_keys.drain(..) {
            if ok {
                self.plugins.keys.unregister(id);
            }
        }
        unsafe { DeregisterShellHookWindow(self.hwnd) };
        self.tray.remove();
        unsafe { PostQuitMessage(0) };
    }
}

/// Global hotkeys, behind a trait so the bookkeeping runs in tests with a
/// fake that never registers anything with Windows.
trait Registrar {
    /// False when the keys are taken, by another app or by WinCraft itself.
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool;
    fn unregister(&mut self, id: i32);
}

/// The hotkeys of the host window.
struct WindowHotkeys(HWND);

impl Registrar for WindowHotkeys {
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool {
        unsafe { RegisterHotKey(self.0, id, hotkey.modifiers, hotkey.vk) != 0 }
    }

    fn unregister(&mut self, id: i32) {
        unsafe { UnregisterHotKey(self.0, id) };
    }
}

/// The plugins and their hotkeys: which plugins run, what each one's
/// settings file says, and which global hotkeys are registered. Kept apart
/// from the tray and the UI, so it runs in tests with fake plugins and a
/// fake registrar.
struct PluginSet {
    /// The host window, which plugins get in their `HostContext`.
    hwnd: HWND,
    slots: Vec<PluginSlot>,
    configs: BTreeMap<String, PluginConfig>,
    keys: Box<dyn Registrar>,
    next_hotkey_id: i32,
    palette_hotkey: Option<(Hotkey, bool)>,
    /// The folder of config.json and the plugin files.
    data: PathBuf,
    /// Balloons for the host to show; a click on one opens ShortcutDetector.
    warnings: Vec<String>,
}

impl PluginSet {
    fn new(
        hwnd: HWND,
        plugins: Vec<Box<dyn WinCraftPlugin>>,
        keys: Box<dyn Registrar>,
        data: PathBuf,
    ) -> Self {
        Self {
            hwnd,
            slots: plugins
                .into_iter()
                .map(|plugin| PluginSlot {
                    plugin,
                    enabled: false,
                    registered: Vec::new(),
                })
                .collect(),
            configs: BTreeMap::new(),
            keys,
            next_hotkey_id: 1,
            palette_hotkey: None,
            data,
            warnings: Vec::new(),
        }
    }

    fn load_configs(&mut self) {
        for index in 0..self.slots.len() {
            self.load_config(index);
        }
    }

    fn load_config(&mut self, index: usize) {
        let slot = &self.slots[index];
        let meta = slot.plugin.metadata();
        let defaults: Vec<(&str, String)> = slot
            .plugin
            .hotkey_actions()
            .iter()
            .map(|action| (action.name, hotkeys::format(action.default)))
            .collect();
        let settings = slot.plugin.default_settings();

        let mut plugin = PluginConfig::load_in(&self.data, meta.id);
        if let Some(from) = slot.plugin.split_from() {
            let first_start = !PluginConfig::exists_in(&self.data, meta.id);
            if first_start && PluginConfig::exists_in(&self.data, from) {
                let mut old = self
                    .configs
                    .get(from)
                    .cloned()
                    .unwrap_or_else(|| PluginConfig::load_in(&self.data, from));
                if slot.plugin.take_over(&mut old, &mut plugin) {
                    log::info!(
                        "{}: took over its settings from {from}; it starts {}",
                        meta.id,
                        if plugin.enabled { "on" } else { "off" }
                    );
                    if let Err(err) = old.save_in(&self.data, from) {
                        log::error!("{err}");
                    }
                    if self.configs.contains_key(from) {
                        self.configs.insert(from.to_string(), old);
                    }
                }
            }
        }
        plugin.merge_defaults(&defaults, &settings);
        slot.plugin
            .migrate_hotkeys(&mut plugin.hotkeys, &mut plugin.settings);
        slot.plugin.migrate_settings(&mut plugin.settings);
        self.configs.insert(meta.id.to_string(), plugin);
        self.save(meta.id);
    }

    /// The plugins to start: those their file has on, and any without one.
    fn wanted(&self) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|index| {
                let id = self.slots[*index].plugin.metadata().id;
                self.configs
                    .get(id)
                    .map(|plugin| plugin.enabled)
                    .unwrap_or(true)
            })
            .collect()
    }

    fn register_palette(&mut self, text: &str) {
        let hotkey = match hotkeys::parse(text) {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("palette_hotkey \"{text}\": {err}; using {DEFAULT_PALETTE_HOTKEY}");
                hotkeys::parse(DEFAULT_PALETTE_HOTKEY).expect("the built-in default parses")
            }
        };
        let ok = self.keys.register(PALETTE_HOTKEY_ID, hotkey);
        let keys = hotkeys::format(hotkey);
        if ok {
            log::info!("registered {keys} for host.palette");
        } else {
            let taken = self.taken_message(hotkey);
            log::warn!("could not register {keys} for host.palette: {taken}");
            self.warnings.push(taken);
        }
        self.palette_hotkey = Some((hotkey, ok));
    }

    /// Who holds keys that could not be registered: a WinCraft command,
    /// whose name the user can look for in Settings, or another app.
    fn holder(&self, hotkey: Hotkey) -> Option<String> {
        if self
            .palette_hotkey
            .is_some_and(|(held, ok)| ok && hotkeys::same(held, hotkey))
        {
            return Some("Open the palette".to_string());
        }
        self.slots.iter().find_map(|slot| {
            slot.registered
                .iter()
                .find(|entry| entry.active && entry.ok && hotkeys::same(entry.hotkey, hotkey))
                .map(|entry| format!("{} ({})", entry.label, slot.plugin.metadata().name))
        })
    }

    fn taken_message(&self, hotkey: Hotkey) -> String {
        let keys = hotkeys::format(hotkey);
        match self.holder(hotkey) {
            Some(holder) => format!("{keys} is used by {holder} in WinCraft"),
            None => format!("{keys} is used by another app"),
        }
    }

    /// Frees the palette hotkey and registers `binding` instead. Returns
    /// the binding as the config file spells it.
    fn rebind_palette(&mut self, binding: &str) -> Result<String, String> {
        let hotkey = hotkeys::parse(binding)?;
        self.free_palette();
        let text = hotkeys::format(hotkey);
        self.register_palette(&text);
        Ok(text)
    }

    fn free_palette(&mut self) {
        if let Some((_, true)) = self.palette_hotkey {
            self.keys.unregister(PALETTE_HOTKEY_ID);
        }
    }

    fn config(&self, id: &str) -> Option<&PluginConfig> {
        self.configs.get(id)
    }

    fn config_mut(&mut self, id: &str) -> &mut PluginConfig {
        self.configs.entry(id.to_string()).or_default()
    }

    fn settings_of(&self, id: &str) -> Value {
        self.config(id)
            .map(|plugin| plugin.settings.clone())
            .unwrap_or_else(|| serde_json::json!({}))
    }

    /// The action's shortcut: the file's text, one press or a chord; None
    /// when the user removed it (empty text); the default when the text
    /// does not parse.
    fn resolve_shortcut(&self, plugin_id: &str, name: &str, default: Hotkey) -> Option<Shortcut> {
        let Some(text) = self
            .config(plugin_id)
            .and_then(|plugin| plugin.hotkeys.get(name))
        else {
            return Some(Shortcut::single(default));
        };
        if text.trim().is_empty() {
            return None;
        }
        match hotkeys::parse_shortcut(text) {
            Ok(keys) => Some(keys),
            Err(err) => {
                log::warn!("{plugin_id}.{name}: {err}; using the plugin default");
                Some(Shortcut::single(default))
            }
        }
    }

    /// Writes an action's shortcut text (empty for none) to the plugin's
    /// file and starts the plugin again with it.
    fn set_shortcut_text(&mut self, plugin_id: &str, action: &str, text: &str) {
        self.config_mut(plugin_id)
            .hotkeys
            .insert(action.to_string(), text.to_string());
        self.save(plugin_id);
        if let Some(index) = self.index_of(plugin_id) {
            self.restart(index);
        }
        log::info!("{plugin_id}.{action} shortcut set to \"{text}\"");
    }

    /// The chords of the plugins that are on: their first press is
    /// registered with the other shortcuts.
    fn plugin_chords(&self) -> Vec<(Shortcut, CommandId)> {
        let mut chords = Vec::new();
        for (index, slot) in self.slots.iter().enumerate().filter(|(_, s)| s.enabled) {
            for entry in slot.registered.iter().filter(|entry| !entry.active) {
                if let Ok(keys) = hotkeys::parse_shortcut(&entry.keys) {
                    chords.push((
                        keys,
                        CommandId::Plugin {
                            index,
                            kind: ActionKind::Hotkey,
                            action: entry.action_id,
                        },
                    ));
                }
            }
        }
        chords
    }

    /// Starts a plugin and registers its hotkeys. A plugin whose start fails
    /// stays off for this session but keeps its saved enabled flag, so the
    /// next start tries it again.
    fn enable(&mut self, index: usize) {
        if self.slots[index].enabled {
            return;
        }
        let meta = self.slots[index].plugin.metadata();
        let settings = self.settings_of(meta.id);

        let init_result = {
            let ctx = HostContext {
                hwnd: self.hwnd,
                settings: &settings,
            };
            self.slots[index].plugin.init(&ctx)
        };
        if let Err(err) = init_result {
            log::error!("{}: init failed: {err}", meta.id);
            self.warnings
                .push(format!("{} failed to start: {err}", meta.name));
            return;
        }

        let actions: Vec<(u32, &'static str, &'static str, Hotkey)> = self.slots[index]
            .plugin
            .hotkey_actions()
            .iter()
            .map(|action| (action.id, action.name, action.label, action.default))
            .collect();

        for (action_id, name, label, default) in actions {
            let shortcut = self.resolve_shortcut(meta.id, name, default);
            let Some(hotkey) = shortcut
                .filter(|keys| !keys.is_chord())
                .map(|keys| keys.first)
            else {
                let keys = shortcut
                    .map(|keys| hotkeys::format_shortcut(&keys))
                    .unwrap_or_default();
                self.slots[index].registered.push(RegisteredHotkey {
                    action_id,
                    global_id: 0,
                    label,
                    hotkey: shortcut.map_or(default, |keys| keys.first),
                    keys,
                    ok: true,
                    active: false,
                });
                continue;
            };
            let global_id = self.next_hotkey_id;
            self.next_hotkey_id += 1;
            let keys = hotkeys::format(hotkey);
            let ok = self.keys.register(global_id, hotkey);
            if ok {
                log::info!("registered {keys} for {}.{name}", meta.id);
            } else {
                let taken = self.taken_message(hotkey);
                log::warn!("could not register {keys} for {}.{name}: {taken}", meta.id);
                self.warnings.push(taken);
            }
            self.slots[index].registered.push(RegisteredHotkey {
                action_id,
                global_id,
                label,
                hotkey,
                keys,
                ok,
                active: true,
            });
        }

        let bound: Vec<(u32, String)> = self.slots[index]
            .registered
            .iter()
            .map(|entry| (entry.action_id, entry.keys.clone()))
            .collect();
        self.slots[index].plugin.on_hotkeys_bound(&bound);

        self.slots[index].enabled = true;
        self.config_mut(meta.id).enabled = true;
        self.save(meta.id);
        log::info!("{} enabled", meta.id);
    }

    fn disable(&mut self, index: usize) {
        if !self.stop(index) {
            return;
        }
        let id = self.slots[index].plugin.metadata().id;
        self.config_mut(id).enabled = false;
        self.save(id);
        log::info!("{id} disabled");
    }

    /// Frees the plugin's hotkeys and tears it down, leaving its saved
    /// enabled flag alone. False when it was not running.
    fn stop(&mut self, index: usize) -> bool {
        if !self.slots[index].enabled {
            return false;
        }
        for entry in self.slots[index].registered.drain(..) {
            if entry.active && entry.ok {
                self.keys.unregister(entry.global_id);
            }
        }
        self.slots[index].plugin.teardown();
        self.slots[index].enabled = false;
        let id = self.slots[index].plugin.metadata().id.to_string();
        WINDOWS_WANTED.with(|queue| queue.borrow_mut().push((id, None, false)));
        plugin_changed();
        true
    }

    /// Quitting stops every plugin but must not save it as disabled, or
    /// the next start would come up with every plugin switched off.
    fn stop_all(&mut self) {
        for index in 0..self.slots.len() {
            if self.stop(index) {
                log::info!("{} stopped", self.slots[index].plugin.metadata().id);
            }
        }
        self.free_palette();
    }

    /// Stops a running plugin and starts it again, for a change it can only
    /// take at start.
    fn restart(&mut self, index: usize) {
        if self.slots[index].enabled {
            self.disable(index);
            self.enable(index);
        }
    }

    fn set_hotkey(&mut self, plugin_id: &str, action: &str, binding: &str) {
        let hotkey = match hotkeys::parse(binding) {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("{plugin_id}.{action}: {err}");
                return;
            }
        };
        self.config_mut(plugin_id)
            .hotkeys
            .insert(action.to_string(), hotkeys::format(hotkey));
        self.save(plugin_id);

        if let Some(index) = self.index_of(plugin_id) {
            self.restart(index);
        }
        log::info!(
            "{plugin_id}.{action} rebound to {}",
            hotkeys::format(hotkey)
        );
    }

    fn set_setting(&mut self, plugin_id: &str, key: &str, value: Value) {
        {
            let plugin = self.config_mut(plugin_id);
            if !plugin.settings.is_object() {
                plugin.settings = serde_json::json!({});
            }
            if key.starts_with('/') {
                config::set_pointer(&mut plugin.settings, key, value);
            } else if let Some(object) = plugin.settings.as_object_mut() {
                object.insert(key.to_string(), value);
            }
        }
        self.save(plugin_id);

        let settings = self.settings_of(plugin_id);
        if let Some(index) = self.index_of(plugin_id) {
            if self.slots[index].enabled && !self.slots[index].plugin.on_settings_changed(&settings)
            {
                self.restart(index);
            }
        }
    }

    /// Deletes the plugin's settings file and starts it again from its
    /// defaults. False when the file could not be deleted.
    fn reset(&mut self, plugin_id: &str) -> bool {
        if let Err(err) = PluginConfig::reset_in(&self.data, plugin_id) {
            log::error!("{err}");
            return false;
        }
        let Some(index) = self.index_of(plugin_id) else {
            return false;
        };
        // Stop without saving: `disable` would write the old in-memory
        // settings back over the file just deleted.
        let was_enabled = self.stop(index);
        self.configs.remove(plugin_id);
        self.load_config(index);
        // The defaults say enabled; a plugin that was off must stay off.
        self.config_mut(plugin_id).enabled = was_enabled;
        self.save(plugin_id);
        if was_enabled {
            self.enable(index);
        }
        log::info!("{plugin_id} reset to defaults");
        true
    }

    /// The plugin and action a global hotkey belongs to, while it is on.
    fn hotkey_target(&self, global_id: i32) -> Option<(usize, u32)> {
        let target = self.slots.iter().enumerate().find_map(|(index, slot)| {
            slot.registered
                .iter()
                .find(|entry| entry.active && entry.global_id == global_id)
                .map(|entry| (index, entry.action_id))
        })?;
        log::debug!(
            "hotkey {global_id} goes to {}, action {}",
            self.slots[target.0].plugin.metadata().id,
            target.1
        );
        self.slots[target.0].enabled.then_some(target)
    }

    /// The registered hotkeys of the plugins that are on: (plugin name,
    /// action label, keys).
    fn registered(&self) -> Vec<(String, String, Hotkey)> {
        let mut found = Vec::new();
        for slot in self.slots.iter().filter(|slot| slot.enabled) {
            let plugin = slot.plugin.metadata().name;
            for entry in slot
                .registered
                .iter()
                .filter(|entry| entry.active && entry.ok)
            {
                found.push((plugin.to_string(), entry.label.to_string(), entry.hotkey));
            }
        }
        found
    }

    fn hotkey_infos(&self, index: usize) -> Vec<HotkeyInfo> {
        let slot = &self.slots[index];
        let meta = slot.plugin.metadata();
        let actions = slot.plugin.hotkey_actions();
        let default_of = |action_id: u32| {
            actions
                .iter()
                .find(|action| action.id == action_id)
                .map(|action| hotkeys::format(action.default))
                .unwrap_or_default()
        };
        if slot.enabled {
            return slot
                .registered
                .iter()
                .map(|entry| HotkeyInfo {
                    action: self.action_name(index, entry.action_id).to_string(),
                    label: entry.label.to_string(),
                    binding: entry.keys.clone(),
                    default_binding: default_of(entry.action_id),
                    registered: entry.ok,
                })
                .collect();
        }
        actions
            .iter()
            .map(|action| HotkeyInfo {
                action: action.name.to_string(),
                label: action.label.to_string(),
                binding: self
                    .resolve_shortcut(meta.id, action.name, action.default)
                    .map(|keys| hotkeys::format_shortcut(&keys))
                    .unwrap_or_default(),
                default_binding: hotkeys::format(action.default),
                registered: true,
            })
            .collect()
    }

    fn action_name(&self, index: usize, action_id: u32) -> &'static str {
        self.slots[index]
            .plugin
            .hotkey_actions()
            .into_iter()
            .find(|action| action.id == action_id)
            .map(|action| action.name)
            .unwrap_or("")
    }

    fn palette_entries(&self, plugins: &[PluginInfo]) -> Vec<PaletteEntry> {
        let mut entries = Vec::new();
        let host_group = "WinCraft".to_string();
        for (command, label) in [
            (HostCommand::OpenSettings, "Open settings"),
            (HostCommand::OpenStore, "Plugin store"),
            (HostCommand::OpenLayout, "Saved window layout"),
            (HostCommand::OpenAbout, "About WinCraft"),
            (HostCommand::OpenConfig, "Open the config file"),
            (HostCommand::OpenLog, "Open the log file"),
            (HostCommand::UpdateWinCraft, "Update WinCraft"),
            (HostCommand::Exit, "Quit WinCraft"),
        ] {
            entries.push(PaletteEntry {
                id: CommandId::Host(command),
                group: host_group.clone(),
                label: label.to_string(),
                hint: String::new(),
                disabled: false,
                plugin: None,
                subtitle: (command == HostCommand::UpdateWinCraft)
                    .then(|| "Checks now and installs a newer version at once".to_string()),
                shortcut_id: String::new(),
                shortcut: String::new(),
                quiet: matches!(
                    command,
                    HostCommand::OpenSettings
                        | HostCommand::OpenStore
                        | HostCommand::UpdateWinCraft
                ),
                badge: None,
            });
        }
        for (index, slot) in self.slots.iter().enumerate() {
            let meta = slot.plugin.metadata();
            let group = meta.name.to_string();
            entries.push(PaletteEntry {
                id: CommandId::Host(HostCommand::TogglePlugin(index)),
                group: group.clone(),
                label: if slot.enabled {
                    format!("Turn {} off", meta.name)
                } else {
                    format!("Turn {} on", meta.name)
                },
                hint: String::new(),
                disabled: false,
                plugin: Some(meta.id.to_string()),
                subtitle: None,
                shortcut_id: String::new(),
                shortcut: String::new(),
                quiet: false,
                badge: None,
            });
            // A plugin that is off keeps its commands in the list, dimmed, so
            // its hotkey does not seem to vanish; running one opens its page.
            let disabled = !slot.enabled;
            let plugin = Some(meta.id.to_string());
            let bindings = plugins.get(index).map(|info| &info.hotkeys);
            for action in slot.plugin.hotkey_actions() {
                let hint = bindings
                    .and_then(|list| list.iter().find(|info| info.label == action.label))
                    .map(|info| info.binding.clone())
                    .unwrap_or_default();
                entries.push(PaletteEntry {
                    id: CommandId::Plugin {
                        index,
                        kind: ActionKind::Hotkey,
                        action: action.id,
                    },
                    group: group.clone(),
                    label: action.label.to_string(),
                    hint,
                    disabled,
                    plugin: plugin.clone(),
                    subtitle: slot.plugin.palette_subtitle(ActionKind::Hotkey, action.id),
                    shortcut_id: String::new(),
                    shortcut: String::new(),
                    quiet: false,
                    badge: None,
                });
            }
            for action in slot.plugin.tray_actions() {
                if entries
                    .iter()
                    .any(|entry| entry.group == group && entry.label == action.label)
                {
                    continue;
                }
                entries.push(PaletteEntry {
                    id: CommandId::Plugin {
                        index,
                        kind: ActionKind::Tray,
                        action: action.id,
                    },
                    group: group.clone(),
                    label: action.label.to_string(),
                    hint: String::new(),
                    disabled,
                    plugin: plugin.clone(),
                    subtitle: slot.plugin.palette_subtitle(ActionKind::Tray, action.id),
                    shortcut_id: String::new(),
                    shortcut: String::new(),
                    quiet: false,
                    badge: None,
                });
            }
            for command in slot.plugin.palette_commands() {
                entries.push(PaletteEntry {
                    id: CommandId::Plugin {
                        index,
                        kind: ActionKind::Palette,
                        action: command.id,
                    },
                    group: group.clone(),
                    label: command.label.to_string(),
                    hint: command.hint.to_string(),
                    disabled,
                    plugin: plugin.clone(),
                    subtitle: slot
                        .plugin
                        .palette_subtitle(ActionKind::Palette, command.id),
                    shortcut_id: String::new(),
                    shortcut: String::new(),
                    quiet: false,
                    badge: None,
                });
            }
        }
        for entry in &mut entries {
            entry.shortcut_id = self.shortcut_id(entry.id);
            if let CommandId::Plugin {
                kind: ActionKind::Hotkey,
                ..
            } = entry.id
            {
                entry.shortcut = entry.hint.clone();
            }
        }
        entries
    }

    /// The id a command has on the Hotkeys page and in
    /// config.json: the plugin's id and the action, never a place in a list.
    fn shortcut_id(&self, id: CommandId) -> String {
        let plugin = |index: usize| {
            self.slots
                .get(index)
                .map_or("unknown", |slot| slot.plugin.metadata().id)
        };
        match id {
            CommandId::Host(HostCommand::TogglePlugin(index)) => {
                format!("{}.turn-on-off", plugin(index))
            }
            CommandId::Host(command) => format!("wincraft.{}", host_slug(command)),
            CommandId::Plugin {
                index,
                kind,
                action,
            } => match kind {
                ActionKind::Hotkey => {
                    format!("{}.{}", plugin(index), self.action_name(index, action))
                }
                ActionKind::Tray => format!("{}.tray-{action}", plugin(index)),
                ActionKind::Palette => format!("{}.command-{action}", plugin(index)),
            },
        }
    }

    /// The strip's windows: from an enabled plugin that keeps a window order
    /// (LayoutKeeper), else from the plugin that draws the strip
    /// (`look_plugin`, WindowDeck), which lists the open windows itself.
    fn window_groups(&mut self, look_plugin: &str) -> Option<(&'static str, WindowGroups)> {
        let mut ask = |own: bool| {
            self.slots.iter_mut().find_map(|slot| {
                let id = slot.plugin.metadata().id;
                if !slot.enabled || (id == look_plugin) != own {
                    return None;
                }
                Some((id, slot.plugin.window_groups()?))
            })
        };
        ask(false).or_else(|| ask(true))
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.plugin.metadata().id == id)
    }

    /// Whether the plugin with this id is there and on.
    fn is_on(&self, id: &str) -> bool {
        self.index_of(id)
            .is_some_and(|index| self.slots[index].enabled)
    }

    fn save(&self, id: &str) {
        let Some(plugin) = self.configs.get(id) else {
            return;
        };
        if let Err(err) = plugin.save_in(&self.data, id) {
            log::error!("could not save the settings for {id}: {err}");
        }
    }
}

/// A known newer release puts a badge on "Update WinCraft" and lifts it out
/// of the typed-only entries, so the user sees it under an empty box. A copy
/// that cannot update itself keeps the plain entry.
fn announce_update(entries: &mut [PaletteEntry], update: &UpdateView) {
    let (Some(release), None) = (&update.available, &update.blocked) else {
        return;
    };
    let version = release.tag.trim_start_matches('v');
    for entry in entries {
        if entry.id == CommandId::Host(HostCommand::UpdateWinCraft) {
            entry.badge = Some(format!("{version} available"));
            entry.subtitle = Some(format!("Installs WinCraft {version} now"));
            entry.quiet = false;
        }
    }
}

fn field_infos(plugin: &dyn WinCraftPlugin, settings: &Value) -> Vec<FieldInfo> {
    let fixed = plugin.settings_fields().into_iter().map(|field| FieldInfo {
        key: field.key.to_string(),
        label: field.label.to_string(),
        help: field.help.to_string(),
        kind: field.kind,
        value: settings.get(field.key).cloned().unwrap_or(Value::Null),
    });
    let dynamic = plugin.dynamic_fields().into_iter().map(|field| FieldInfo {
        key: field.key,
        label: field.label,
        help: field.help,
        kind: field.kind,
        value: field.value,
    });
    fixed.chain(dynamic).collect()
}

/// The button under a plugin's status line: one of its hotkey actions, or
/// failing that one of its palette commands.
fn page_button(plugin: &dyn WinCraftPlugin, index: usize) -> Option<(String, CommandId)> {
    let action = plugin.page_action()?;
    let hotkey = plugin
        .hotkey_actions()
        .into_iter()
        .find(|hotkey| hotkey.id == action)
        .map(|hotkey| (hotkey.label, ActionKind::Hotkey));
    let found = hotkey.or_else(|| {
        plugin
            .palette_commands()
            .into_iter()
            .find(|command| command.id == action)
            .map(|command| (command.label, ActionKind::Palette))
    });
    found.map(|(label, kind)| {
        (
            label.to_string(),
            CommandId::Plugin {
                index,
                kind,
                action,
            },
        )
    })
}

/// Shows a tray balloon for a plugin. Safe to call from any plugin callback.
pub fn notify(title: &str, text: &str) {
    NOTICES.with(|queue| {
        queue
            .borrow_mut()
            .push((title.to_string(), text.to_string()))
    });
    plugin_changed();
}

/// Sets the tray icon's tooltip, or with None puts the plain name back.
/// Safe to call from any plugin callback.
pub fn set_tray_tip(tip: Option<String>) {
    TRAY_TIP.with(|cell| *cell.borrow_mut() = Some(tip));
    plugin_changed();
}

/// Opens the palette as a one-line box that asks the user `prompt`; the
/// answer comes back to the plugin it names, through `on_prompt_answer`.
/// Safe to call from any plugin callback.
pub fn ask(prompt: Prompt) {
    PROMPT.with(|cell| *cell.borrow_mut() = Some(prompt));
    plugin_changed();
}

/// The Arrange strip's hotkey was pressed: wait for a taskbar number, or on
/// a second press open the strip for the window in front. Call it from
/// `on_hotkey`. Safe to call from any plugin callback.
pub fn arrange_hotkey() {
    ARRANGE_HOTKEY.with(|cell| cell.set(true));
    plugin_changed();
}

/// Opens the Arrange windows list. Safe to call from any plugin callback.
pub fn open_arrange() {
    ARRANGE_WANTED.with(|cell| cell.set(true));
    plugin_changed();
}

/// Opens the Arrange strip on `app`, centred above its taskbar button at
/// `button` (left, top, right, bottom in physical pixels), without taking
/// the keyboard from the window in front. Safe to call from any callback.
pub fn open_arrange_at(app: String, button: [i32; 4]) {
    ARRANGE_ANCHOR.with(|cell| *cell.borrow_mut() = Some((app, button)));
    open_arrange();
}

/// Win and `digit` were pressed and a plugin's keyboard hook kept them:
/// the strip for that taskbar button, even for an app with one window.
/// `again` is the same digit once more while Win stayed down, which picks
/// the next card of an open strip. A button whose app has no window goes
/// back to Windows. Safe to call from any callback, a hook's too.
pub fn open_taskbar_button(digit: u8, again: bool) {
    WIN_DIGITS.with(|queue| queue.borrow_mut().push((digit, again)));
    plugin_changed();
}

/// The host window, for the calls that other threads may make.
static HOST_HWND: AtomicIsize = AtomicIsize::new(0);

/// `open_taskbar_button` from any thread: for a keyboard hook that runs on
/// a thread of its own, so it answers Windows at once however busy the
/// host is.
pub fn post_taskbar_button(digit: u8, again: bool) {
    let hwnd = HOST_HWND.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe {
            PostMessageW(
                hwnd as HWND,
                WM_APP_WIN_DIGIT,
                usize::from(digit),
                isize::from(again),
            )
        };
    }
}

/// Closes the Arrange strip if it is open. Safe to call from any callback.
pub fn close_arrange() {
    ARRANGE_CLOSE.with(|cell| cell.set(true));
    plugin_changed();
}

/// Opens one of the plugin's own windows (see `WinCraftPlugin::windows`),
/// or brings it back if it is open. Safe to call from any callback.
pub fn show_window(plugin: &str, window: &str) {
    WINDOWS_WANTED.with(|queue| {
        queue
            .borrow_mut()
            .push((plugin.to_string(), Some(window.to_string()), true))
    });
    plugin_changed();
}

/// Changes one of a plugin's own settings and saves it, as if the user had
/// changed it on the plugin's page; the plugin hears of it through
/// `on_settings_changed`. Safe to call from any callback.
pub fn save_setting(plugin: &str, key: &str, value: Value) {
    SETTINGS_WANTED.with(|queue| {
        queue
            .borrow_mut()
            .push((plugin.to_string(), key.to_string(), value))
    });
    plugin_changed();
}

/// True in a test instance started with `--allow-hide-popup`: WindowDeck's
/// hover strip may hide Windows' taskbar preview, which a test instance
/// otherwise never touches.
pub fn allow_hide_popup() -> bool {
    ALLOW_HIDE_POPUP.load(Ordering::Relaxed)
}

/// Sends the Arrange strip fresh data if it is open, for changes that show
/// a moment after the plugin made them. Safe to call from any callback.
pub fn refresh_arrange() {
    ARRANGE_STALE.with(|cell| cell.set(true));
    plugin_changed();
}

thread_local! {
    /// The chord's first press while it waits for the second; the hook's
    /// rule reads it.
    static CHORD_PRESS: RefCell<Option<FirstPress>> = const { RefCell::new(None) };
    /// The Arrange strip's hotkey while it waits for a digit.
    static WAIT_HOTKEY: Cell<Option<Hotkey>> = const { Cell::new(None) };
    /// The hotkey that came last, as pressed and as the user names it, for
    /// a plugin that asks for `arrange_hotkey` from its `on_hotkey`.
    static LAST_HOTKEY: RefCell<Option<(Hotkey, String)>> = const { RefCell::new(None) };
}

/// The keyboard hook's rule while a chord waits for its second press.
fn chord_rule(stroke: chord::Stroke) -> chord::Verdict {
    CHORD_PRESS.with(|cell| {
        cell.borrow().as_ref().map_or(chord::Verdict::End, |press| {
            shortcuts::chord_verdict(shortcuts::chord_key(press, stroke))
        })
    })
}

/// The part of a WinCraft command's shortcut id after "wincraft.".
fn host_slug(command: HostCommand) -> &'static str {
    match command {
        HostCommand::OpenSettings => "open-settings",
        HostCommand::OpenStore => "plugin-store",
        HostCommand::OpenLayout => "saved-layout",
        HostCommand::OpenAbout => "about",
        HostCommand::OpenConfig => "config-file",
        HostCommand::OpenLog => "log-file",
        HostCommand::UpdateWinCraft => "update",
        HostCommand::Exit => "quit",
        HostCommand::TogglePlugin(_) => "turn-on-off",
    }
}

/// The keyboard hook's rule while the Arrange hotkey waits; see
/// `taskbar_number::wait_key`.
fn wait_rule(stroke: chord::Stroke) -> chord::Verdict {
    match WAIT_HOTKEY.with(|cell| cell.get()) {
        Some(hotkey) => taskbar_number::wait_verdict(taskbar_number::wait_key(stroke, hotkey)),
        None => chord::Verdict::End,
    }
}

/// A shell message that a window came, went or took the front: the open
/// Arrange strip shows a new window's card, and takes a closed one's away.
/// A new window is often still hidden when it is created, so the moment it
/// comes to the front counts too.
fn window_list_changed(code: usize) -> bool {
    const CREATED: u32 = 1;
    const DESTROYED: u32 = 2;
    const ACTIVATED: u32 = 4;
    const RUDE_ACTIVATED: u32 = 0x8004;
    matches!(
        code as u32,
        CREATED | DESTROYED | ACTIVATED | RUDE_ACTIVATED
    )
}

/// Asks the host to send the settings window a fresh snapshot, for example
/// because a plugin's `status` line changed.
pub fn plugin_changed() {
    let hwnd = HOST_WINDOW.with(|cell| cell.get());
    if !hwnd.is_null() {
        unsafe { PostMessageW(hwnd, WM_APP_PLUGIN, 0, 0) };
    }
}

/// The list the ShortcutDetector marks as WinCraft's own.
///
/// It takes an immutable borrow, so calling it while the host is mutably
/// borrowed panics instead of quietly misbehaving. That is the re-entrancy rule
/// with teeth: a plugin may call this from its own WndProc, never from inside
/// on_hotkey or on_tray_action.
pub fn registered_hotkeys() -> Vec<(String, String, Hotkey)> {
    HOST.with(|cell| {
        let borrowed = cell.borrow();
        borrowed
            .as_ref()
            .map(|host| {
                let mut found = host.plugins.registered();
                found.extend(host.shortcut_keys.iter().filter(|(_, _, ok)| *ok).map(
                    |(_, press, _)| {
                        let what = if press.seconds.is_empty() {
                            "Shortcut"
                        } else {
                            "Start of a chord"
                        };
                        ("WinCraft".to_string(), what.to_string(), press.hotkey)
                    },
                ));
                found
            })
            .unwrap_or_default()
    })
}

fn cursor_monitor() -> MonitorRect {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return MonitorRect::default();
    }
    MonitorRect {
        left: info.rcWork.left,
        top: info.rcWork.top,
        right: info.rcWork.right,
        bottom: info.rcWork.bottom,
    }
}

/// Runs on the host thread on purpose. Delivering the hotkey is what gives this
/// process the right to take the foreground, and that right belongs to the
/// thread the hotkey was delivered to.
pub fn bring_to_front(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    if unsafe { SetForegroundWindow(hwnd) } != 0 {
        return;
    }
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return;
    }
    let other = unsafe { GetWindowThreadProcessId(foreground, std::ptr::null_mut()) };
    let mine = unsafe { GetCurrentThreadId() };
    unsafe {
        AttachThreadInput(mine, other, 1);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        AttachThreadInput(mine, other, 0);
    }
}

/// The palette window is larger than the panel so the shadow fits inside it,
/// which means Windows' own rounded corners and 1 px border would outline the
/// transparent margin rather than the panel. Both are turned off; the panel
/// draws its own.
fn clear_system_frame(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let corners = DWMWCP_DONOTROUND;
    let border = DWMWA_COLOR_NONE;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corners as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&corners) as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&border) as u32,
        );
    }
}

fn show_palette() {
    search::actions::remember_front();
    let palette = with_host(|host| {
        host.to_ui.send(UiCommand::ShowPalette(cursor_monitor()));
        host.palette_hwnd
    });
    if let Some(hwnd) = palette {
        bring_to_front(hwnd);
    }
}

fn open_shortcut_detector() {
    with_host(|host| {
        let slot = host
            .plugins
            .slots
            .iter_mut()
            .find(|slot| slot.enabled && slot.plugin.metadata().id == DETECTOR_ID);
        match slot {
            Some(slot) => slot.plugin.on_tray_action(DETECTOR_OPEN_ACTION),
            None => log::info!("no enabled {DETECTOR_ID} plugin to open"),
        }
    });
}

fn open_in_notepad(path: &std::path::Path) {
    let args = wide(&format!("\"{}\"", path.display()));
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

/// Skips the work instead of panicking when the host is already borrowed.
/// Creating a window inside a callback lets Windows send other messages to the
/// host window before the first call has returned. For a message that cannot
/// wait or carries a pointer, dropping it is right and crashing the tray
/// program is not; the others go through `with_host_or_later`.
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| match cell.try_borrow_mut() {
        Ok(mut borrowed) => borrowed.as_mut().map(f),
        Err(_) => {
            let name = CURRENT_MESSAGE.with(|current| current.get()).map_or_else(
                || "call".to_string(),
                |(msg, wparam)| message_name(msg, wparam),
            );
            log::warn!("host message {name} arrived while the host was busy; ignored");
            None
        }
    })
}

fn host_is_busy() -> bool {
    HOST.with(|cell| cell.try_borrow_mut().is_err())
}

/// Runs `f` on what the cell holds, unless the cell is borrowed already.
/// Returns true when it was borrowed and `f` did not run.
fn run_unless_busy<T>(cell: &RefCell<Option<T>>, f: impl FnOnce(&mut T)) -> bool {
    match cell.try_borrow_mut() {
        Ok(mut borrowed) => {
            if let Some(value) = borrowed.as_mut() {
                f(value);
            }
            false
        }
        Err(_) => true,
    }
}

/// Like `with_host`, but a message that finds the host busy is delivered
/// again a moment later instead of being dropped. Windows sends
/// WM_DISPLAYCHANGE while the host thread waits on another program's
/// window, as when LayoutKeeper moves windows while a monitor powers up,
/// and a dropped one leaves ScreenDimmer on the old layout. Only for messages
/// whose parameters are plain values: a pointer in one would be stale by
/// then. Anything `f` takes out of a queue must be taken inside `f`, which
/// runs only when the host is free, or it is lost with the first attempt.
fn with_host_or_later(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    f: impl FnOnce(&mut Host),
) {
    if HOST.with(|cell| run_unless_busy(cell, f)) {
        deliver_later(hwnd, msg, wparam, lparam);
    }
}

fn deliver_later(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    log::info!(
        "host message {} arrived while the host was busy; delivering it again in {RETRY_MS} ms",
        message_name(msg, wparam)
    );
    RETRY.with(|queue| queue_again(&mut queue.borrow_mut(), msg, wparam, lparam));
    unsafe { SetTimer(hwnd, RETRY_TIMER, RETRY_MS, None) };
}

/// Only the newest display change is kept: each one describes the whole
/// layout, and running an old one after a newer one would be wrong. A plugin
/// request is kept once, because the notices and the other things it asks for
/// wait in their own queues, not in the message; and it is kept past the
/// limit, because losing it would lose what those queues hold.
fn queue_again(queue: &mut Vec<(u32, WPARAM, LPARAM)>, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    if msg == WM_DISPLAYCHANGE {
        queue.retain(|(queued, _, _)| *queued != WM_DISPLAYCHANGE);
    }
    if msg == WM_APP_PLUGIN {
        if !queue.iter().any(|(queued, _, _)| *queued == WM_APP_PLUGIN) {
            queue.push((msg, wparam, lparam));
        }
        return;
    }
    if queue.len() < RETRY_LIMIT {
        queue.push((msg, wparam, lparam));
    }
}

fn message_name(msg: u32, wparam: WPARAM) -> String {
    if msg != 0 && msg == TASKBAR_CREATED.with(|cell| cell.get()) {
        return "TaskbarCreated".to_string();
    }
    match msg {
        WM_DISPLAYCHANGE => "WM_DISPLAYCHANGE".to_string(),
        WM_HOTKEY => "WM_HOTKEY".to_string(),
        WM_TRAY_CALLBACK => "WM_TRAY_CALLBACK (tray icon)".to_string(),
        WM_APP_UI => "WM_APP_UI (request from the UI thread)".to_string(),
        WM_APP_PLUGIN => "WM_APP_PLUGIN (plugin request)".to_string(),
        WM_APP_UPDATE => "WM_APP_UPDATE (update result)".to_string(),
        WM_APP_NOTICE_CLICK => "WM_APP_NOTICE_CLICK (click on the update notice)".to_string(),
        WM_SETTINGCHANGE => "WM_SETTINGCHANGE".to_string(),
        WM_POWERBROADCAST => "WM_POWERBROADCAST".to_string(),
        WM_QUERYENDSESSION => "WM_QUERYENDSESSION".to_string(),
        WM_ENDSESSION => "WM_ENDSESSION".to_string(),
        WM_DESTROY => "WM_DESTROY".to_string(),
        WM_TIMER => match wparam {
            UPDATE_TIMER => "WM_TIMER (update check)".to_string(),
            HEALTHY_TIMER => "WM_TIMER (healthy report)".to_string(),
            NOTICE_TIMER => "WM_TIMER (update notice)".to_string(),
            BADGE_TIMER => "WM_TIMER (taskbar number badges)".to_string(),
            RETRY_TIMER => "WM_TIMER (redelivery)".to_string(),
            other => format!("WM_TIMER 0x{other:X}"),
        },
        other => format!("0x{other:04X}"),
    }
}

/// What plugins asked the host for through `notify`, `set_tray_tip`, `ask`,
/// `open_arrange` and `refresh_arrange`. Taking it empties the queues, so it
/// is taken only once the host is free to act on it.
struct PluginRequests {
    notices: Vec<(String, String)>,
    tip: Option<Option<String>>,
    arrange: bool,
    arrange_hotkey: bool,
    anchor: Option<(String, [i32; 4])>,
    close: bool,
    stale: bool,
    win_digits: Vec<(u8, bool)>,
    prompt: Option<Prompt>,
    settings: Vec<(String, String, Value)>,
    windows: Vec<(String, Option<String>, bool)>,
}

impl PluginRequests {
    fn take() -> Self {
        PluginRequests {
            notices: NOTICES.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
            tip: TRAY_TIP.with(|cell| cell.borrow_mut().take()),
            arrange: ARRANGE_WANTED.with(|cell| cell.replace(false)),
            arrange_hotkey: ARRANGE_HOTKEY.with(|cell| cell.replace(false)),
            anchor: ARRANGE_ANCHOR.with(|cell| cell.borrow_mut().take()),
            close: ARRANGE_CLOSE.with(|cell| cell.replace(false)),
            stale: ARRANGE_STALE.with(|cell| cell.replace(false)),
            win_digits: WIN_DIGITS.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
            prompt: PROMPT.with(|cell| cell.borrow_mut().take()),
            settings: SETTINGS_WANTED.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
            windows: WINDOWS_WANTED.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
        }
    }
}

fn handle_menu_choice(choice: u32) {
    match choice {
        MENU_PALETTE => show_palette(),
        MENU_SETTINGS => {
            with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::General)));
        }
        MENU_UPDATE => {
            with_host(|host| {
                host.to_ui.send(UiCommand::ShowSettings(Page::About));
                host.updates.reopen = None;
                host.update_now();
            });
        }
        MENU_QUIT => {
            with_host(|host| host.shutdown());
        }
        id if id >= MENU_PLUGIN_ACTION_BASE => {
            with_host(|host| {
                let offset = id - MENU_PLUGIN_ACTION_BASE;
                let index = (offset / 100) as usize;
                let action_id = offset % 100;
                if let Some(slot) = host.plugins.slots.get_mut(index) {
                    if slot.enabled {
                        slot.plugin.on_tray_action(action_id);
                    }
                }
            });
        }
        _ => {}
    }
}

/// What the flags of `WM_QUERYENDSESSION` and `WM_ENDSESSION` say, for the
/// log. An installer or Windows Update closes apps with `ENDSESSION_CLOSEAPP`
/// while Windows keeps running; a real shutdown or restart sends no flags.
fn end_session_words(lparam: LPARAM) -> String {
    let flags = lparam as u32;
    let mut words = Vec::new();
    if flags & ENDSESSION_CLOSEAPP != 0 {
        words.push("ENDSESSION_CLOSEAPP (an installer or update asks apps to close, Windows keeps running)");
    }
    if flags & ENDSESSION_CRITICAL != 0 {
        words.push("ENDSESSION_CRITICAL (Windows ends the session now and does not wait for apps)");
    }
    if flags & ENDSESSION_LOGOFF != 0 {
        words.push("ENDSESSION_LOGOFF (the user is signing out)");
    }
    if words.is_empty() {
        words.push("no flags (shutdown or restart)");
    }
    words.join("; ")
}

fn is_colour_change(lparam: LPARAM) -> bool {
    if lparam == 0 {
        return false;
    }
    let mut text = Vec::new();
    let mut at = lparam as *const u16;
    for _ in 0..64 {
        let ch = unsafe { *at };
        if ch == 0 {
            break;
        }
        text.push(ch);
        at = unsafe { at.add(1) };
    }
    String::from_utf16_lossy(&text) == "ImmersiveColorSet"
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let outer = CURRENT_MESSAGE.with(|current| current.replace(Some((msg, wparam))));
    let result = dispatch(hwnd, msg, wparam, lparam);
    CURRENT_MESSAGE.with(|current| current.set(outer));
    result
}

fn dispatch(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = TASKBAR_CREATED.with(|cell| cell.get());
    if taskbar_created != 0 && msg == taskbar_created {
        with_host_or_later(hwnd, msg, wparam, lparam, |host| {
            host.tray.add();
            for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                slot.plugin.on_taskbar_created();
            }
        });
        return 0;
    }
    let shell_hook = SHELL_HOOK.with(|cell| cell.get());
    if shell_hook != 0 && msg == shell_hook {
        let foreground = unsafe { GetForegroundWindow() } as isize;
        // Which window an app flashes is not always the one with the news
        // (one app may flash its main window for every window), so the log
        // names it.
        if wparam == 0x8006 && lparam != 0 {
            log::info!(
                "attention: window {lparam:#x} \"{}\" flashed{}",
                crate::core::windows_list::window_text(lparam as HWND),
                if lparam == foreground {
                    " (in front, not marked)"
                } else {
                    ""
                }
            );
        }
        let changed = ATTENTION.with(|attention| {
            attention
                .borrow_mut()
                .shell_message(wparam, lparam, foreground)
        });
        if (changed || window_list_changed(wparam)) && ui_bridge::arrange_window().is_some() {
            refresh_arrange();
        }
        return 0;
    }

    match msg {
        WM_HOTKEY => {
            let global_id = wparam as i32;
            if global_id == PALETTE_HOTKEY_ID {
                if host_is_busy() {
                    deliver_later(hwnd, msg, wparam, lparam);
                } else {
                    show_palette();
                }
                return 0;
            }
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                if global_id >= SHORTCUT_ID_BASE {
                    host.shortcut_pressed(global_id);
                    return;
                }
                if let Some((index, action_id)) = host.plugins.hotkey_target(global_id) {
                    let pressed = host.plugins.slots[index]
                        .registered
                        .iter()
                        .find(|entry| entry.active && entry.global_id == global_id)
                        .map(|entry| (entry.hotkey, entry.keys.clone()));
                    LAST_HOTKEY.with(|cell| *cell.borrow_mut() = pressed);
                    host.plugins.slots[index].plugin.on_hotkey(action_id);
                }
            });
            0
        }
        WM_TRAY_CALLBACK => {
            if host_is_busy() {
                deliver_later(hwnd, msg, wparam, lparam);
                return 0;
            }
            let event = (lparam as u32) & 0xFFFF;
            if event == NIN_BALLOONUSERCLICK {
                match with_host(|host| host.balloon_click) {
                    Some(BalloonClick::Detector) => open_shortcut_detector(),
                    Some(BalloonClick::About) => {
                        with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::About)));
                    }
                    _ => {}
                }
                return 0;
            }
            if event == WM_LBUTTONUP || event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                // The borrow must end before TrackPopupMenu: it runs its own
                // message loop, which re-enters this WndProc and would panic on
                // the second borrow_mut.
                let Some((items, menu_hwnd)) = with_host(|host| (host.menu_items(), host.hwnd))
                else {
                    return 0;
                };
                if let Some(choice) = show_menu(menu_hwnd, &items) {
                    handle_menu_choice(choice);
                }
            }
            0
        }
        WM_APP_PLUGIN => {
            let mut palette = None;
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                let requests = PluginRequests::take();
                if let Some(tip) = &requests.tip {
                    host.tray.set_tip(tip.as_deref());
                }
                for (title, text) in &requests.notices {
                    host.balloon_click = BalloonClick::Nothing;
                    host.tray.balloon(title, text);
                }
                for (plugin, key, value) in requests.settings {
                    host.plugins.set_setting(&plugin, &key, value);
                }
                if requests.close {
                    host.to_ui.send(UiCommand::HideArrange);
                }
                for (plugin, window, open) in requests.windows {
                    match window {
                        Some(window) if open && !host.plugins.is_on(&plugin) => {
                            log::info!("{plugin} is off, so its window {window} stays closed");
                        }
                        Some(window) => host.to_ui.send(UiCommand::PluginWindow {
                            plugin,
                            window,
                            open,
                        }),
                        None => host.to_ui.send(UiCommand::ClosePluginWindows(plugin)),
                    }
                }
                if requests.arrange_hotkey {
                    match LAST_HOTKEY.with(|cell| cell.borrow_mut().take()) {
                        Some((hotkey, keys)) => host.arrange_hotkey(hotkey, &keys),
                        None => {
                            host.arrange_anchor = None;
                            host.send_arrange(true);
                        }
                    }
                }
                if requests.arrange {
                    host.arrange_anchor = requests.anchor;
                    host.send_arrange(true);
                } else if requests.stale {
                    host.send_arrange(false);
                }
                for (digit, again) in requests.win_digits {
                    if host.win_digit(digit, again) == WinDigit::GiveBack {
                        mask_key::replay_win_digit(0x30 + u32::from(digit));
                    }
                }
                host.publish();
                if let Some(prompt) = requests.prompt {
                    host.to_ui
                        .send(UiCommand::ShowPrompt(prompt, cursor_monitor()));
                    palette = Some(host.palette_hwnd);
                }
            });
            if let Some(hwnd) = palette {
                bring_to_front(hwnd);
            }
            0
        }
        WM_APP_UI => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                while let Ok(request) = host.host_rx.try_recv() {
                    host.handle_request(request);
                }
            });
            0
        }
        WM_APP_UPDATE => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.update_events());
            0
        }
        WM_TIMER if wparam == UPDATE_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.daily_check());
            0
        }
        WM_APP_WIN_DIGIT => {
            open_taskbar_button(wparam as u8, lparam != 0);
            0
        }
        WM_APP_WAIT_KEY => {
            if host_is_busy() {
                deliver_later(hwnd, msg, wparam, lparam);
                return 0;
            }
            let news = chord::take_for(hwnd as isize);
            with_host(|host| host.wait_news(news));
            0
        }
        WM_TIMER if wparam == chord::TIMER_ID => {
            chord::on_timer(hwnd as isize);
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.end_wait());
            0
        }
        WM_TIMER if wparam == BADGE_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.reveal_tick());
            0
        }
        WM_TIMER if wparam == NOTICE_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.notice_tick());
            0
        }
        WM_APP_NOTICE_CLICK => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                host.close_update_notice();
                host.to_ui.send(UiCommand::ShowSettings(Page::About));
            });
            0
        }
        WM_TIMER if wparam == HEALTHY_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.report_healthy(false));
            0
        }
        WM_TIMER if wparam == RETRY_TIMER => {
            unsafe { KillTimer(hwnd, RETRY_TIMER) };
            let queued = RETRY.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
            for (msg, wparam, lparam) in queued {
                unsafe { wnd_proc(hwnd, msg, wparam, lparam) };
            }
            0
        }
        WM_DISPLAYCHANGE => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                // The palette was placed against a monitor layout that no
                // longer exists, so it is hidden rather than left stranded.
                host.to_ui.send(UiCommand::HideAll);
                for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.plugin.on_windows_message(msg, wparam, lparam);
                }
            });
            0
        }
        WM_SETTINGCHANGE | WM_POWERBROADCAST | WM_TIMER => {
            if msg == WM_SETTINGCHANGE
                && (is_colour_change(lparam) || wparam == SPI_SETHIGHCONTRAST as WPARAM)
            {
                with_host(|host| {
                    host.tray.refresh_icon();
                    host.to_ui.send(UiCommand::ThemeChanged);
                });
            }
            with_host(|host| {
                for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.plugin.on_windows_message(msg, wparam, lparam);
                }
            });
            0
        }
        // Plugins get these synchronously, while every program is still
        // open, so a plugin can record the session before it goes away.
        // Answering TRUE never holds up the shutdown.
        WM_QUERYENDSESSION | WM_ENDSESSION => {
            let why = end_session_words(lparam);
            if msg == WM_QUERYENDSESSION {
                log::info!("Windows asks if the session can end: {why}");
            } else if wparam != 0 {
                log::info!("the session is ending: {why}");
            } else {
                log::info!("the session did not end after all: {why}");
            }
            with_host(|host| {
                for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.plugin.on_windows_message(msg, wparam, lparam);
                }
            });
            if msg == WM_QUERYENDSESSION {
                1
            } else {
                0
            }
        }
        WM_DESTROY => {
            with_host(|host| host.shutdown());
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The plugin and hotkey bookkeeping below runs with fake plugins, a fake
    // keyboard instead of RegisterHotKey, and settings files in a scratch
    // folder. No test registers a real hotkey or touches the real profile.

    use std::rc::Rc;

    use crate::core::traits::{
        HotkeyAction, PaletteCommand, StripLook, TrayAction, TrayToggle, WindowGroups,
    };
    use crate::core::ui_bridge::{ArrangeView, ArrangeWindow, LayoutAction};

    #[test]
    fn the_strip_hears_of_windows_that_come_go_or_take_the_front() {
        for code in [1, 2, 4, 0x8004] {
            assert!(window_list_changed(code), "{code:#x}");
        }
        // A flash, a redraw and a title change leave the list as it is.
        for code in [0x8006, 6, 0] {
            assert!(!window_list_changed(code), "{code:#x}");
        }
    }

    /// Stands in for RegisterHotKey: keys another app holds, and keys
    /// already registered here, are refused, as Windows refuses them.
    #[derive(Default)]
    struct Keyboard {
        taken_by_others: Vec<Hotkey>,
        registered: BTreeMap<i32, Hotkey>,
        freed: Vec<i32>,
    }

    #[derive(Clone, Default)]
    struct FakeKeys(Rc<RefCell<Keyboard>>);

    impl Registrar for FakeKeys {
        fn register(&mut self, id: i32, hotkey: Hotkey) -> bool {
            let mut keyboard = self.0.borrow_mut();
            let taken = keyboard.taken_by_others.contains(&hotkey)
                || keyboard.registered.contains_key(&id)
                || keyboard.registered.values().any(|held| *held == hotkey);
            if !taken {
                keyboard.registered.insert(id, hotkey);
            }
            !taken
        }

        fn unregister(&mut self, id: i32) {
            let mut keyboard = self.0.borrow_mut();
            keyboard.registered.remove(&id);
            keyboard.freed.push(id);
        }
    }

    impl FakeKeys {
        fn held(&self) -> Vec<String> {
            self.0
                .borrow()
                .registered
                .values()
                .map(|hotkey| hotkeys::format(*hotkey))
                .collect()
        }
    }

    fn keys(text: &str) -> Hotkey {
        hotkeys::parse(text).unwrap()
    }

    type Diary = Rc<RefCell<Vec<String>>>;

    /// A plugin that writes down what the host asks of it.
    struct FakePlugin {
        id: &'static str,
        name: &'static str,
        /// (action id, name, label, default keys)
        actions: Vec<(u32, &'static str, &'static str, &'static str)>,
        fails_with: Option<&'static str>,
        takes_settings_live: bool,
        /// Keeps a window order for the strip, as LayoutKeeper does.
        groups: bool,
        /// Decides when the strip opens, as WindowDeck does.
        look: bool,
        diary: Diary,
    }

    fn fake(
        id: &'static str,
        name: &'static str,
        default: &'static str,
        diary: &Diary,
    ) -> FakePlugin {
        FakePlugin {
            id,
            name,
            actions: vec![(1, "go", "Go", default)],
            fails_with: None,
            takes_settings_live: true,
            groups: false,
            look: false,
            diary: Rc::clone(diary),
        }
    }

    impl FakePlugin {
        fn note(&self, what: String) {
            self.diary.borrow_mut().push(what);
        }
    }

    impl WinCraftPlugin for FakePlugin {
        fn metadata(&self) -> crate::core::traits::PluginMetadata {
            crate::core::traits::PluginMetadata {
                id: self.id,
                name: self.name,
                description: "A plugin for tests.",
                author: "test",
                version: "1.0.0",
                readme: "",
                category: "Test",
                icon: None,
                thumbnail: None,
            }
        }

        fn default_settings(&self) -> Value {
            serde_json::json!({ "level": 1 })
        }

        fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
            self.note(format!("init {} level {}", self.id, ctx.settings["level"]));
            match self.fails_with {
                Some(why) => Err(why.to_string()),
                None => Ok(()),
            }
        }

        fn hotkey_actions(&self) -> Vec<HotkeyAction> {
            self.actions
                .iter()
                .map(|(id, name, label, default)| HotkeyAction {
                    id: *id,
                    name,
                    label,
                    default: keys(default),
                })
                .collect()
        }

        fn on_hotkey(&mut self, action_id: u32) {
            self.note(format!("hotkey {} {action_id}", self.id));
        }

        fn tray_actions(&self) -> Vec<TrayAction> {
            vec![
                TrayAction { id: 1, label: "Go" },
                TrayAction {
                    id: 2,
                    label: "Open",
                },
            ]
        }

        fn on_tray_action(&mut self, action_id: u32) {
            self.note(format!("tray {} {action_id}", self.id));
        }

        fn tray_toggles(&self) -> Vec<TrayToggle> {
            vec![TrayToggle {
                id: 5,
                label: "Feature",
                on: true,
            }]
        }

        fn palette_commands(&self) -> Vec<PaletteCommand> {
            vec![PaletteCommand {
                id: 7,
                label: "Tidy up",
                hint: "tidy",
            }]
        }

        fn on_settings_changed(&mut self, settings: &Value) -> bool {
            self.note(format!("settings {} {settings}", self.id));
            self.takes_settings_live
        }

        fn on_prompt_answer(&mut self, prompt_id: u32, _target: isize, text: &str) {
            self.note(format!("answer {} {prompt_id} {text}", self.id));
        }

        fn on_layout_action(&mut self, _action: &LayoutAction) {
            self.note(format!("layout {}", self.id));
        }

        fn window_groups(&mut self) -> Option<WindowGroups> {
            self.groups.then(|| WindowGroups {
                groups: vec![ArrangeGroup {
                    key: "Chrome.UserData.Profile3".to_string(),
                    label: "Chrome".to_string(),
                    windows: vec![ArrangeWindow {
                        hwnd: 0x7fff_fff0,
                        label: "one".to_string(),
                        desktop: String::new(),
                        monitor: None,
                        on_monitor: None,
                        saved_monitor: None,
                        renamed: false,
                        same_title: false,
                        attention: false,
                    }],
                }],
                desktops: Vec::new(),
                focus: 0,
                watched: vec!["*".to_string()],
                monitors: Vec::new(),
            })
        }

        fn on_arrange_action(&mut self, _action: &ArrangeAction) {
            self.note(format!("arrange {}", self.id));
        }

        fn strip_look(&self) -> Option<StripLook> {
            self.look.then_some(StripLook {
                preview: false,
                view: ArrangeView::Flat,
            })
        }

        fn teardown(&mut self) {
            self.note(format!("teardown {}", self.id));
        }
    }

    /// A data folder of its own, removed afterwards.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("wincraft-host-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn saved(&self, id: &str) -> PluginConfig {
            PluginConfig::load_in(&self.0, id)
        }

        fn has_file(&self, id: &str) -> bool {
            self.0.join("plugins").join(format!("{id}.json")).exists()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn plugin_set(plugins: Vec<FakePlugin>, keyboard: &FakeKeys, data: &Scratch) -> PluginSet {
        let plugins = plugins
            .into_iter()
            .map(|plugin| Box::new(plugin) as Box<dyn WinCraftPlugin>)
            .collect();
        boxed_set(plugins, keyboard, data)
    }

    fn boxed_set(
        plugins: Vec<Box<dyn WinCraftPlugin>>,
        keyboard: &FakeKeys,
        data: &Scratch,
    ) -> PluginSet {
        let mut set = PluginSet::new(
            std::ptr::null_mut(),
            plugins,
            Box::new(keyboard.clone()),
            data.0.clone(),
        );
        set.load_configs();
        set
    }

    fn diary() -> Diary {
        Rc::new(RefCell::new(Vec::new()))
    }

    fn read(diary: &Diary) -> Vec<String> {
        std::mem::take(&mut *diary.borrow_mut())
    }

    #[test]
    fn a_plugin_starts_with_its_saved_settings_and_registers_its_hotkeys() {
        let data = Scratch::new("start");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![
                fake("fake_a", "Fake A", "Win+Alt+F1", &log),
                fake("fake_b", "Fake B", "Win+Alt+F2", &log),
            ],
            &keyboard,
            &data,
        );
        assert_eq!(set.wanted(), [0, 1], "no file says a plugin is off");
        for index in set.wanted() {
            set.enable(index);
        }
        assert_eq!(read(&log), ["init fake_a level 1", "init fake_b level 1"]);
        assert_eq!(keyboard.held(), ["Win+Alt+F1", "Win+Alt+F2"]);
        assert!(set.warnings.is_empty());
        let saved = data.saved("fake_a");
        assert!(saved.enabled);
        assert_eq!(saved.hotkeys["go"], "Win+Alt+F1");
        assert_eq!(saved.settings["level"], 1);
        assert_eq!(
            set.registered(),
            [
                ("Fake A".to_string(), "Go".to_string(), keys("Win+Alt+F1")),
                ("Fake B".to_string(), "Go".to_string(), keys("Win+Alt+F2")),
            ]
        );
        assert_eq!(set.hotkey_target(1), Some((0, 1)));
        assert_eq!(set.hotkey_target(2), Some((1, 1)));
        assert_eq!(set.hotkey_target(3), None);
        // Asked again, a running plugin is not started twice.
        set.enable(0);
        assert!(read(&log).is_empty());
    }

    #[test]
    fn a_plugin_saved_as_off_is_not_started() {
        let data = Scratch::new("saved-off");
        let off = PluginConfig {
            enabled: false,
            ..PluginConfig::default()
        };
        off.save_in(&data.0, "fake_b").unwrap();
        let log = diary();
        let set = plugin_set(
            vec![
                fake("fake_a", "Fake A", "Win+Alt+F1", &log),
                fake("fake_b", "Fake B", "Win+Alt+F2", &log),
            ],
            &FakeKeys::default(),
            &data,
        );
        assert_eq!(set.wanted(), [0]);
        // Its file got the defaults it was missing.
        assert_eq!(data.saved("fake_b").hotkeys["go"], "Win+Alt+F2");
    }

    #[test]
    fn a_plugin_whose_start_fails_stays_off_and_keeps_its_saved_flag() {
        let data = Scratch::new("crash");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut broken = fake("broken", "Broken", "Win+Alt+F1", &log);
        broken.fails_with = Some("no device");
        let mut set = plugin_set(vec![broken], &keyboard, &data);
        set.enable(0);
        assert!(!set.slots[0].enabled);
        assert!(set.slots[0].registered.is_empty());
        assert!(keyboard.held().is_empty());
        assert_eq!(set.next_hotkey_id, 1, "no hotkey id was used up");
        assert_eq!(set.warnings, ["Broken failed to start: no device"]);
        assert!(data.saved("broken").enabled, "the next start tries again");
        assert_eq!(set.wanted(), [0]);
        // Turning it off now changes nothing: it is not running.
        set.disable(0);
        assert!(data.saved("broken").enabled);
        assert_eq!(read(&log), ["init broken level 1"]);
    }

    #[test]
    fn turning_a_plugin_off_frees_its_hotkeys_and_saves_it_off() {
        let data = Scratch::new("off");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        set.enable(0);
        set.disable(0);
        assert!(!set.slots[0].enabled);
        assert!(keyboard.held().is_empty());
        assert_eq!(read(&log), ["init fake_a level 1", "teardown fake_a"]);
        assert!(!data.saved("fake_a").enabled);
        assert!(set.wanted().is_empty());
        set.disable(0);
        assert!(read(&log).is_empty(), "no second teardown");
        // Back on with a fresh hotkey id, saved on again.
        set.enable(0);
        assert_eq!(set.slots[0].registered[0].global_id, 2);
        assert!(data.saved("fake_a").enabled);
    }

    #[test]
    fn quitting_stops_every_plugin_but_saves_none_of_them_as_off() {
        let data = Scratch::new("quit");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![
                fake("fake_a", "Fake A", "Win+Alt+F1", &log),
                fake("fake_b", "Fake B", "Win+Alt+F2", &log),
            ],
            &keyboard,
            &data,
        );
        set.register_palette("Win+Alt+Space");
        set.enable(0);
        set.enable(1);
        read(&log);
        set.stop_all();
        assert_eq!(read(&log), ["teardown fake_a", "teardown fake_b"]);
        assert!(
            keyboard.held().is_empty(),
            "the palette's keys are free too"
        );
        assert!(set.slots.iter().all(|slot| !slot.enabled));
        assert!(data.saved("fake_a").enabled && data.saved("fake_b").enabled);
        assert!(set.configs["fake_a"].enabled);
        assert_eq!(set.wanted(), [0, 1]);
    }

    #[test]
    fn a_hotkey_another_app_holds_is_reported_and_never_freed() {
        let data = Scratch::new("taken");
        let (log, keyboard) = (diary(), FakeKeys::default());
        keyboard
            .0
            .borrow_mut()
            .taken_by_others
            .push(keys("Win+Alt+F1"));
        let mut a = fake("fake_a", "Fake A", "Win+Alt+F1", &log);
        a.actions.push((2, "stop", "Stop", "Win+Alt+F4"));
        let mut set = plugin_set(vec![a], &keyboard, &data);
        set.enable(0);
        assert!(set.slots[0].enabled, "the plugin runs without that hotkey");
        assert_eq!(set.warnings, ["Win+Alt+F1 is used by another app"]);
        let infos = set.hotkey_infos(0);
        let shown: Vec<(&str, &str, bool)> = infos
            .iter()
            .map(|info| (info.action.as_str(), info.binding.as_str(), info.registered))
            .collect();
        assert_eq!(
            shown,
            [("go", "Win+Alt+F1", false), ("stop", "Win+Alt+F4", true)]
        );
        assert_eq!(
            set.registered().len(),
            1,
            "only keys it holds count as its own"
        );
        set.disable(0);
        assert_eq!(
            keyboard.0.borrow().freed,
            [2],
            "the refused id is not freed"
        );
    }

    #[test]
    fn two_plugins_that_want_the_same_keys_get_them_in_turn() {
        let data = Scratch::new("clash");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![
                fake("fake_a", "Fake A", "Win+Alt+F1", &log),
                fake("fake_b", "Fake B", "Win+Alt+F1", &log),
            ],
            &keyboard,
            &data,
        );
        set.enable(0);
        set.enable(1);
        assert!(set.slots[0].registered[0].ok);
        assert!(!set.slots[1].registered[0].ok);
        assert_eq!(
            set.warnings,
            ["Win+Alt+F1 is used by Go (Fake A) in WinCraft"]
        );
        // Once the first lets go, the second gets them on its next start.
        set.disable(0);
        set.set_hotkey("fake_b", "go", "Win+Alt+F1");
        assert!(set.slots[1].registered[0].ok);
    }

    #[test]
    fn the_palette_hotkey_clashes_like_any_other_and_can_be_rebound() {
        let data = Scratch::new("palette");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        set.register_palette("Win+Alt+F1");
        assert_eq!(set.palette_hotkey, Some((keys("Win+Alt+F1"), true)));
        set.enable(0);
        assert!(!set.slots[0].registered[0].ok, "the palette was first");
        assert_eq!(
            set.warnings,
            ["Win+Alt+F1 is used by Open the palette in WinCraft"]
        );

        assert_eq!(
            set.rebind_palette("Win+Alt+F9"),
            Ok("Win+Alt+F9".to_string())
        );
        assert_eq!(keyboard.held(), ["Win+Alt+F9"]);
        assert!(set.rebind_palette("Win+Nothing").is_err());
        assert_eq!(set.palette_hotkey, Some((keys("Win+Alt+F9"), true)));

        // A palette hotkey another app holds is not freed when rebound.
        keyboard
            .0
            .borrow_mut()
            .taken_by_others
            .push(keys("Win+Alt+F8"));
        set.rebind_palette("Win+Alt+F8").unwrap();
        assert_eq!(set.palette_hotkey, Some((keys("Win+Alt+F8"), false)));
        let freed = keyboard.0.borrow().freed.len();
        set.rebind_palette("Win+Alt+F7").unwrap();
        assert_eq!(keyboard.0.borrow().freed.len(), freed);
    }

    #[test]
    fn a_palette_hotkey_that_does_not_parse_falls_back_to_the_default() {
        let data = Scratch::new("palette-default");
        let keyboard = FakeKeys::default();
        let mut set = plugin_set(Vec::new(), &keyboard, &data);
        set.register_palette("Ctrl+Nonsense");
        assert_eq!(
            set.palette_hotkey,
            Some((keys(DEFAULT_PALETTE_HOTKEY), true))
        );
    }

    #[test]
    fn a_rebound_hotkey_is_saved_and_the_plugin_restarted_with_it() {
        let data = Scratch::new("rebind");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![
                fake("fake_a", "Fake A", "Win+Alt+F1", &log),
                fake("fake_b", "Fake B", "Win+Alt+F2", &log),
            ],
            &keyboard,
            &data,
        );
        set.enable(0);
        read(&log);
        set.set_hotkey("fake_a", "go", "win+alt+f3");
        assert_eq!(data.saved("fake_a").hotkeys["go"], "Win+Alt+F3");
        assert_eq!(keyboard.held(), ["Win+Alt+F3"]);
        assert_eq!(read(&log), ["teardown fake_a", "init fake_a level 1"]);
        assert_eq!(set.hotkey_infos(0)[0].default_binding, "Win+Alt+F1");

        set.set_hotkey("fake_a", "go", "Win+Banana");
        assert_eq!(data.saved("fake_a").hotkeys["go"], "Win+Alt+F3");
        assert!(read(&log).is_empty(), "nothing restarted");

        // A plugin that is off only gets its file changed.
        set.set_hotkey("fake_b", "go", "Win+Alt+F5");
        assert_eq!(data.saved("fake_b").hotkeys["go"], "Win+Alt+F5");
        assert!(read(&log).is_empty());
        assert_eq!(set.hotkey_infos(1)[0].binding, "Win+Alt+F5");
    }

    #[test]
    fn a_saved_binding_that_does_not_parse_gives_the_plugin_default() {
        let data = Scratch::new("bad-binding");
        let mut file = PluginConfig::default();
        file.hotkeys.insert("go".to_string(), "Hyper+Q".to_string());
        file.save_in(&data.0, "fake_a").unwrap();
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        let infos = set.hotkey_infos(0);
        assert_eq!(
            (infos[0].binding.as_str(), infos[0].registered),
            ("Win+Alt+F1", true)
        );
        set.enable(0);
        assert_eq!(keyboard.held(), ["Win+Alt+F1"]);
        assert_eq!(
            data.saved("fake_a").hotkeys["go"],
            "Hyper+Q",
            "the file keeps it"
        );
    }

    #[test]
    fn a_setting_the_plugin_cannot_take_live_restarts_it() {
        let data = Scratch::new("settings");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut stubborn = fake("fake_b", "Fake B", "Win+Alt+F2", &log);
        stubborn.takes_settings_live = false;
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log), stubborn],
            &keyboard,
            &data,
        );
        set.enable(0);
        set.enable(1);
        read(&log);
        set.set_setting("fake_a", "level", serde_json::json!(2));
        assert_eq!(read(&log), [r#"settings fake_a {"level":2}"#]);
        set.set_setting("fake_b", "/monitors/LG", serde_json::json!(true));
        assert_eq!(
            read(&log),
            [
                r#"settings fake_b {"level":1,"monitors":{"LG":true}}"#,
                "teardown fake_b",
                "init fake_b level 1",
            ]
        );
        assert_eq!(data.saved("fake_b").settings["monitors"]["LG"], true);
        // A plugin that is off is not asked.
        set.disable(0);
        read(&log);
        set.set_setting("fake_a", "level", serde_json::json!(3));
        assert!(read(&log).is_empty());
        assert_eq!(data.saved("fake_a").settings["level"], 3);
    }

    #[test]
    fn a_reset_deletes_the_file_of_a_plugin_that_is_off_and_reads_its_defaults() {
        let data = Scratch::new("reset");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        set.set_setting("fake_a", "level", serde_json::json!(5));
        set.set_hotkey("fake_a", "go", "Win+Alt+F6");
        assert!(set.reset("fake_a"));
        let saved = data.saved("fake_a");
        assert_eq!(saved.settings["level"], 1);
        assert_eq!(saved.hotkeys["go"], "Win+Alt+F1");
        assert!(read(&log).is_empty(), "it stays off");
        assert!(!set.reset("nobody"), "no such plugin");
        assert!(!data.has_file("nobody"));
    }

    #[test]
    fn a_reset_of_a_running_plugin_restarts_it() {
        let data = Scratch::new("reset-on");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        set.enable(0);
        set.set_setting("fake_a", "level", serde_json::json!(5));
        read(&log);
        assert!(set.reset("fake_a"));
        assert!(set.slots[0].enabled && data.saved("fake_a").enabled);
        assert_eq!(read(&log)[0], "teardown fake_a");
    }

    #[test]
    fn a_reset_gives_a_running_plugin_its_defaults_in_memory_on_disk_and_on_the_keyboard() {
        let data = Scratch::new("reset-defaults");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        set.enable(0);
        set.set_setting("fake_a", "level", serde_json::json!(5));
        set.set_hotkey("fake_a", "go", "Win+Alt+F6");
        assert_eq!(keyboard.held(), ["Win+Alt+F6"]);
        read(&log);

        assert!(set.reset("fake_a"));

        assert_eq!(
            read(&log),
            ["teardown fake_a", "init fake_a level 1"],
            "the plugin starts again from the defaults, not from level 5"
        );
        assert_eq!(set.settings_of("fake_a")["level"], 1);
        assert_eq!(set.config("fake_a").unwrap().hotkeys["go"], "Win+Alt+F1");
        let saved = data.saved("fake_a");
        assert!(saved.enabled);
        assert_eq!(saved.settings["level"], 1);
        assert_eq!(saved.hotkeys["go"], "Win+Alt+F1");
        assert_eq!(keyboard.held(), ["Win+Alt+F1"]);
        assert_eq!(
            set.registered(),
            [("Fake A".to_string(), "Go".to_string(), keys("Win+Alt+F1"))]
        );
    }

    #[test]
    fn a_reset_of_a_plugin_that_is_off_leaves_it_off_with_defaults_on_disk() {
        let data = Scratch::new("reset-off-defaults");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        set.enable(0);
        set.set_setting("fake_a", "level", serde_json::json!(5));
        set.disable(0);
        read(&log);

        assert!(set.reset("fake_a"));

        assert!(read(&log).is_empty(), "it is not started or stopped");
        assert!(!set.slots[0].enabled);
        assert!(keyboard.held().is_empty());
        assert_eq!(set.settings_of("fake_a")["level"], 1);
        assert_eq!(data.saved("fake_a").settings["level"], 1);
        assert!(
            !data.saved("fake_a").enabled,
            "it stays off after the next start"
        );
    }

    #[test]
    fn the_palette_lists_each_plugins_commands_dimmed_while_it_is_off() {
        let data = Scratch::new("entries");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        let plugins = |set: &PluginSet| -> Vec<PluginInfo> {
            vec![PluginInfo {
                id: "fake_a".to_string(),
                name: "Fake A".to_string(),
                version: String::new(),
                author: String::new(),
                description: String::new(),
                readme: String::new(),
                category: String::new(),
                icon: None,
                enabled: set.slots[0].enabled,
                config_path: PathBuf::new(),
                hotkeys: set.hotkey_infos(0),
                fields: Vec::new(),
                status: None,
                page_action: None,
            }]
        };
        let off = set.palette_entries(&plugins(&set));
        let mine: Vec<(&str, &str, bool)> = off
            .iter()
            .filter(|entry| entry.group == "Fake A")
            .map(|entry| (entry.label.as_str(), entry.hint.as_str(), entry.disabled))
            .collect();
        // The tray's "Go" is the hotkey's "Go" already, so it is listed once.
        assert_eq!(
            mine,
            [
                ("Turn Fake A on", "", false),
                ("Go", "Win+Alt+F1", true),
                ("Open", "", true),
                ("Tidy up", "tidy", true),
            ]
        );
        assert_eq!(
            off.iter().filter(|entry| entry.group == "WinCraft").count(),
            8
        );
        set.enable(0);
        let on = set.palette_entries(&plugins(&set));
        let toggle = on.iter().find(|entry| entry.group == "Fake A").unwrap();
        assert_eq!(toggle.label, "Turn Fake A off");
        assert_eq!(toggle.id, CommandId::Host(HostCommand::TogglePlugin(0)));
        assert!(on
            .iter()
            .filter(|entry| entry.group == "Fake A")
            .all(|entry| !entry.disabled));
    }

    #[test]
    fn the_palette_has_one_update_command_whatever_is_known() {
        let data = Scratch::new("update-entries");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        let updates: Vec<PaletteEntry> = set
            .palette_entries(&[])
            .into_iter()
            .filter(|entry| entry.group == "WinCraft" && entry.label.contains("pdate"))
            .collect();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].label, "Update WinCraft");
        assert_eq!(updates[0].id, CommandId::Host(HostCommand::UpdateWinCraft));
        assert_eq!(
            updates[0].subtitle.as_deref(),
            Some("Checks now and installs a newer version at once")
        );
    }

    #[test]
    fn a_known_release_badges_the_update_entry_unless_updates_are_blocked() {
        let data = Scratch::new("update-badge");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let set = plugin_set(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        let update_of = |view: &UpdateView| {
            let mut entries = set.palette_entries(&[]);
            announce_update(&mut entries, view);
            entries
                .into_iter()
                .find(|entry| entry.id == CommandId::Host(HostCommand::UpdateWinCraft))
                .unwrap()
        };
        let plain = update_of(&UpdateView::default());
        assert!(plain.badge.is_none() && plain.quiet);

        let newer = UpdateView {
            available: Some(NewRelease {
                tag: "v0.20.20".to_string(),
                notes: String::new(),
                page: String::new(),
            }),
            ..UpdateView::default()
        };
        let badged = update_of(&newer);
        assert_eq!(badged.badge.as_deref(), Some("0.20.20 available"));
        assert_eq!(
            badged.subtitle.as_deref(),
            Some("Installs WinCraft 0.20.20 now")
        );
        assert!(!badged.quiet);

        let blocked = update_of(&UpdateView {
            blocked: Some("a development build".to_string()),
            ..newer
        });
        assert_eq!(blocked.badge, plain.badge);
        assert_eq!(blocked.subtitle, plain.subtitle);
        assert!(blocked.quiet);
    }

    #[test]
    fn update_wincraft_waits_for_a_running_update_and_says_why_it_cannot() {
        let data = Scratch::new("update-commands");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        host.updates.newer = Some(release("v0.17.9"));
        host.publish();
        let listed = snapshots(&ui).pop().unwrap().commands;
        assert!(listed.iter().any(|entry| entry.label == "Update WinCraft"));
        assert!(!listed
            .iter()
            .any(|entry| entry.label.starts_with("Update to")));

        // Busy: no second download and no second check.
        host.updates.busy = true;
        host.handle_request(HostRequest::RunCommand(CommandId::Host(
            HostCommand::UpdateWinCraft,
        )));
        assert!(!host.updates.checking && !host.updates.auto_update);
        assert_eq!(host.updates.status, UpdateStatus::Idle);

        // A copy that cannot update itself checks nothing.
        host.updates.busy = false;
        host.updates.blocked = Some("a development build".to_string());
        host.handle_request(HostRequest::RunCommand(CommandId::Host(
            HostCommand::UpdateWinCraft,
        )));
        assert!(!host.updates.checking && !host.updates.auto_update && !host.updates.report);
    }

    #[test]
    fn an_update_that_does_not_happen_leaves_nothing_to_reopen() {
        let data = Scratch::new("update-reopen");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, _ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        // About's Update now on a copy that cannot update itself.
        host.updates.blocked = Some("a development build".to_string());
        host.handle_request(HostRequest::UpdateNow);
        assert_eq!(host.updates.reopen, None);

        // A check that finds nothing newer forgets where it was asked from.
        host.updates.blocked = None;
        host.updates.reopen = Some(update::Reopen::Palette);
        let version = env!("CARGO_PKG_VERSION");
        host.checked(Ok(Checked::UpToDate(format!("v{version}"))));
        assert_eq!(host.updates.reopen, None);

        // So does a download that fails.
        host.updates.reopen = Some(update::Reopen::About);
        host.downloaded(Err("offline".to_string()));
        assert_eq!(host.updates.reopen, None);
    }

    #[test]
    fn update_wincraft_says_every_answer_and_installs_a_newer_one() {
        let asked = || Updates {
            manual: true,
            auto_update: true,
            report: true,
            ..Updates::default()
        };
        let mut config = UpdateConfig::default();
        let version = env!("CARGO_PKG_VERSION");

        let mut updates = asked();
        let after = updates.checked(
            Ok(Checked::UpToDate(format!("v{version}"))),
            &mut config,
            100,
        );
        assert_eq!(
            after.notice,
            Some(format!("WinCraft is up to date ({version})"))
        );
        assert!(!after.update_now);

        let mut updates = asked();
        let after = updates.checked(Ok(Checked::Newer(release("v9.9.9"))), &mut config, 100);
        assert!(after.update_now, "installed at once");
        assert_eq!(
            after.notice.as_deref(),
            Some("Updating WinCraft to 9.9.9. It restarts when the new version is in place.")
        );

        let mut updates = asked();
        let after = updates.checked(Err("no network".to_string()), &mut config, 100);
        assert_eq!(
            after.notice.as_deref(),
            Some("WinCraft could not check for updates. Is the PC online?")
        );

        let mut updates = asked();
        let after = updates.checked(Ok(Checked::NoRelease), &mut config, 100);
        assert_eq!(
            after.notice.as_deref(),
            Some("No WinCraft release is published yet")
        );
        assert!(!updates.report, "said once");

        // A daily check stays quiet when nothing is new.
        let mut daily = Updates::default();
        let after = daily.checked(
            Ok(Checked::UpToDate(format!("v{version}"))),
            &mut config,
            200,
        );
        assert_eq!(after.notice, None);
    }

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_string(),
            notes: String::new(),
            page: String::new(),
            assets: Vec::new(),
        }
    }

    #[test]
    fn a_failed_update_check_says_offline_only_when_asked_for() {
        let mut updates = Updates::default();
        let mut config = UpdateConfig::default();
        updates.checking = true;
        let after = updates.checked(Err("no network".to_string()), &mut config, 100);
        assert_eq!(after, AfterCheck::default());
        assert_eq!(updates.status, UpdateStatus::Idle);
        assert!(!updates.checking);
        assert_eq!(config.last_check, 0, "nothing answered");

        updates.manual = true;
        updates.auto_update = true;
        updates.checked(Err("no network".to_string()), &mut config, 100);
        assert_eq!(updates.status, UpdateStatus::Offline);
        assert!(!updates.manual && !updates.auto_update);
    }

    #[test]
    fn an_answered_check_records_what_github_said() {
        let mut updates = Updates::default();
        let mut config = UpdateConfig::default();
        let after = updates.checked(Ok(Checked::NoRelease), &mut config, 100);
        assert!(after.answered && after.notice.is_none() && !after.reload_notes);
        assert_eq!(updates.status, UpdateStatus::NoRelease);
        assert_eq!(config.last_check, 100);

        updates.newer = Some(release("v9.9.9"));
        let after = updates.checked(
            Ok(Checked::UpToDate("v0.16.6".to_string())),
            &mut config,
            200,
        );
        assert!(after.answered && after.reload_notes);
        assert_eq!(updates.status, UpdateStatus::UpToDate);
        assert!(updates.newer.is_none());
        assert_eq!(
            (config.last_check, config.latest.as_str()),
            (200, "v0.16.6")
        );
    }

    #[test]
    fn a_newer_release_is_announced_once_and_installed_only_after_update_now() {
        let mut updates = Updates::default();
        let mut config = UpdateConfig::default();
        let after = updates.checked(Ok(Checked::Newer(release("v0.17.0"))), &mut config, 100);
        assert_eq!(
            after.notice.as_deref(),
            Some("WinCraft 0.17.0 is available \u{2014} open About to update")
        );
        assert!(!after.update_now);
        assert_eq!(config.latest, "v0.17.0");
        assert_eq!(
            updates.newer.as_ref().map(|r| r.tag.as_str()),
            Some("v0.17.0")
        );

        updates.auto_update = true;
        let after = updates.checked(Ok(Checked::Newer(release("v0.17.0"))), &mut config, 200);
        assert_eq!(after.notice, None, "the same tag is not announced again");
        assert!(after.update_now && !updates.auto_update);
    }

    #[test]
    fn the_healthy_report_goes_once_and_tidies_only_a_copy_that_updates_itself() {
        let mut updates = Updates::default();
        assert_eq!(updates.healthy_report(false), None);
        updates.healthy_pending = Some("0.16.5".to_string());
        assert_eq!(
            updates.healthy_report(false),
            Some(("0.16.5".to_string(), true))
        );
        assert_eq!(updates.healthy_report(false), None, "told once");
        updates.healthy_pending = Some("0.16.5".to_string());
        assert_eq!(
            updates.healthy_report(true),
            Some(("0.16.5".to_string(), false))
        );
        updates.healthy_pending = Some("0.16.5".to_string());
        updates.blocked = Some("a development build".to_string());
        assert_eq!(
            updates.healthy_report(false),
            Some(("0.16.5".to_string(), false))
        );
    }

    /// A host with no window and no tray icon. What it sends the UI waits
    /// in the returned receiver.
    fn test_host(
        plugins: Vec<FakePlugin>,
        keyboard: &FakeKeys,
        data: &Scratch,
    ) -> (Host, Receiver<UiCommand>) {
        let bridge = ui_bridge::create();
        let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let mut host = Host {
            hwnd: std::ptr::null_mut(),
            config: Config::default(),
            plugins: plugin_set(plugins, keyboard, data),
            tray: Tray::new(std::ptr::null_mut(), hinstance),
            to_ui: bridge.to_ui,
            host_rx: bridge.host_rx,
            palette_hwnd: std::ptr::null_mut(),
            balloon_click: BalloonClick::Nothing,
            arrange_focus: None,
            peek_card: None,
            arrange_anchor: None,
            digits: taskbar_number::Digits::default(),
            buttons: Vec::new(),
            buttons_at: None,
            clock: std::time::Instant::now(),
            wait_hint: None,
            reveal: taskbar_number::Reveal::default(),
            reveal_line: String::new(),
            updates: Updates::default(),
            update_notice: None,
            autostart_note: None,
            shortcut_keys: Vec::new(),
            chord_wait: None,
            win_digit_opened: None,
        };
        for index in host.plugins.wanted() {
            host.enable_slot(index);
        }
        (host, bridge.ui_rx)
    }

    fn snapshots(ui: &Receiver<UiCommand>) -> Vec<UiSnapshot> {
        ui.try_iter()
            .filter_map(|command| match command {
                UiCommand::Snapshot(snapshot) => Some(*snapshot),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_settings_window_switches_plugins_and_gets_a_fresh_snapshot() {
        let data = Scratch::new("request-enable");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        host.handle_request(HostRequest::SetPluginEnabled {
            id: "fake_a".to_string(),
            enabled: false,
        });
        let sent = snapshots(&ui);
        assert_eq!(sent.len(), 1);
        assert!(!sent[0].plugins[0].enabled);
        assert!(sent[0]
            .commands
            .iter()
            .any(|entry| entry.label == "Turn Fake A on"));
        assert!(!data.saved("fake_a").enabled);

        host.handle_request(HostRequest::SetPluginEnabled {
            id: "nobody".to_string(),
            enabled: true,
        });
        assert!(snapshots(&ui).is_empty(), "nothing changed");
        host.handle_request(HostRequest::RunCommand(CommandId::Host(
            HostCommand::TogglePlugin(0),
        )));
        assert!(host.plugins.slots[0].enabled);
        assert_eq!(snapshots(&ui).len(), 1);
    }

    #[test]
    fn the_tray_menu_shows_the_first_action_and_the_ticked_toggles() {
        let data = Scratch::new("tray-toggles");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (host, _ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        let entries: Vec<(u32, String, bool)> = host
            .menu_items()
            .into_iter()
            .filter_map(|item| match item {
                MenuItem::Entry { id, label, checked } => Some((id, label, checked)),
                MenuItem::Separator => None,
            })
            .collect();
        let base = MENU_PLUGIN_ACTION_BASE;
        assert!(entries.contains(&(base + 1, "Go".to_string(), false)));
        assert!(entries.contains(&(base + 5, "Feature".to_string(), true)));
        assert!(
            !entries.iter().any(|(_, label, _)| label == "Open"),
            "only the first tray action"
        );
    }

    #[test]
    fn a_plugin_can_open_the_strip_on_a_button_close_it_and_save_a_setting() {
        open_arrange_at("Chrome.UserData.Profile3".to_string(), [1, 2, 3, 4]);
        close_arrange();
        save_setting("fake_a", "level", serde_json::json!(4));
        let requests = PluginRequests::take();
        assert!(requests.arrange);
        assert_eq!(
            requests.anchor,
            Some(("Chrome.UserData.Profile3".to_string(), [1, 2, 3, 4]))
        );
        assert!(requests.close);
        assert_eq!(
            requests.settings,
            [(
                "fake_a".to_string(),
                "level".to_string(),
                serde_json::json!(4)
            )]
        );
        let again = PluginRequests::take();
        assert!(!again.arrange && !again.close && again.anchor.is_none());
        assert!(again.settings.is_empty());
    }

    #[test]
    fn requests_reach_a_plugin_only_while_it_is_on() {
        let data = Scratch::new("request-off");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        read(&log);
        let ask = || HostRequest::PromptAnswer {
            plugin: "fake_a".to_string(),
            id: 3,
            target: 0,
            text: "yes".to_string(),
        };
        let run = || {
            HostRequest::RunCommand(CommandId::Plugin {
                index: 0,
                kind: ActionKind::Hotkey,
                action: 1,
            })
        };
        let layout = || HostRequest::Layout {
            plugin: "fake_a".to_string(),
            action: LayoutAction::SaveNow,
        };
        host.handle_request(ask());
        host.handle_request(run());
        host.handle_request(layout());
        host.handle_request(HostRequest::RunCommand(CommandId::Plugin {
            index: 0,
            kind: ActionKind::Tray,
            action: 2,
        }));
        assert_eq!(
            read(&log),
            [
                "answer fake_a 3 yes",
                "hotkey fake_a 1",
                "layout fake_a",
                "tray fake_a 2"
            ]
        );
        let layouts = ui
            .try_iter()
            .filter(|command| matches!(command, UiCommand::LayoutUpdate(None)))
            .count();
        assert_eq!(layouts, 1, "the Layout page hears there is no layout");

        host.plugins.disable(0);
        read(&log);
        host.handle_request(ask());
        host.handle_request(run());
        host.handle_request(layout());
        host.handle_request(HostRequest::Arrange {
            plugin: "fake_a".to_string(),
            action: ArrangeAction::Activate(0),
        });
        assert!(read(&log).is_empty());
        host.handle_request(HostRequest::RunCommand(CommandId::Plugin {
            index: 9,
            kind: ActionKind::Hotkey,
            action: 1,
        }));
    }

    #[test]
    fn settings_requests_save_and_publish() {
        let data = Scratch::new("request-settings");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        host.handle_request(HostRequest::SetHotkey {
            plugin: "fake_a".to_string(),
            action: "go".to_string(),
            binding: "Win+Alt+F2".to_string(),
        });
        host.handle_request(HostRequest::SetSetting {
            plugin: "fake_a".to_string(),
            key: "level".to_string(),
            value: serde_json::json!(4),
        });
        host.handle_request(HostRequest::ResetPlugin("fake_a".to_string()));
        assert_eq!(snapshots(&ui).len(), 3);

        host.handle_request(HostRequest::SetHostSetting(HostSetting::PaletteHotkey(
            "win+alt+f9".to_string(),
        )));
        assert_eq!(host.config.palette_hotkey, "Win+Alt+F9");
        assert!(keyboard.held().contains(&"Win+Alt+F9".to_string()));
        host.handle_request(HostRequest::SetHostSetting(HostSetting::PaletteHotkey(
            "nonsense".to_string(),
        )));
        assert_eq!(host.config.palette_hotkey, "Win+Alt+F9");
        host.handle_request(HostRequest::SetHostSetting(HostSetting::CheckUpdatesDaily(
            false,
        )));
        let saved = Config::load_in(&data.0);
        assert_eq!(saved.palette_hotkey, "Win+Alt+F9");
        assert!(!saved.updates.check_daily);
        host.handle_request(HostRequest::ClearPaletteUsage);
        assert!(ui
            .try_iter()
            .any(|command| matches!(command, UiCommand::ClearPaletteUsage)));
    }

    fn change(id: &str, binding: Option<&str>) -> ShortcutChange {
        ShortcutChange {
            id: id.to_string(),
            binding: binding.map(str::to_string),
        }
    }

    fn row<'a>(snapshot: &'a UiSnapshot, id: &str) -> &'a ShortcutInfo {
        snapshot
            .shortcuts
            .iter()
            .find(|info| info.id == id)
            .unwrap_or_else(|| panic!("no row {id}"))
    }

    #[test]
    fn every_palette_command_is_a_shortcut_row_with_a_stable_id() {
        let data = Scratch::new("shortcut-rows");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (host, _ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        let snapshot = host.snapshot();
        let ids: Vec<&str> = snapshot
            .shortcuts
            .iter()
            .map(|info| info.id.as_str())
            .collect();
        for id in [
            "wincraft.palette",
            "wincraft.open-settings",
            "wincraft.quit",
            "fake_a.turn-on-off",
            "fake_a.go",
            "fake_a.tray-2",
            "fake_a.command-7",
        ] {
            assert!(ids.contains(&id), "{id} in {ids:?}");
        }
        let go = row(&snapshot, "fake_a.go");
        assert_eq!(
            (go.binding.as_str(), go.default_binding.as_str()),
            ("Win+Alt+F1", "Win+Alt+F1")
        );
        assert_eq!(
            row(&snapshot, "wincraft.palette").binding,
            DEFAULT_PALETTE_HOTKEY
        );
        assert!(row(&snapshot, "wincraft.palette").one_press);
        assert_eq!(row(&snapshot, "fake_a.command-7").binding, "");
        let entry = snapshot
            .commands
            .iter()
            .find(|entry| entry.shortcut_id == "fake_a.go")
            .unwrap();
        assert_eq!(entry.shortcut, "Win+Alt+F1");
    }

    #[test]
    fn shortcuts_are_saved_as_overrides_and_reset_takes_them_away() {
        let data = Scratch::new("shortcut-overrides");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );

        host.handle_request(HostRequest::SetShortcuts(vec![change(
            "fake_a.command-7",
            Some("win+alt+k, t"),
        )]));
        assert_eq!(host.config.shortcuts["fake_a.command-7"], "Win+Alt+K, T");
        assert_eq!(
            Config::load_in(&data.0).shortcuts["fake_a.command-7"],
            "Win+Alt+K, T"
        );
        assert!(keyboard.held().contains(&"Win+Alt+K".to_string()));
        let last = snapshots(&ui).pop().unwrap();
        assert_eq!(row(&last, "fake_a.command-7").binding, "Win+Alt+K, T");

        // Removing a plugin's hotkey writes the empty text and frees the keys.
        host.handle_request(HostRequest::SetShortcuts(vec![change(
            "fake_a.go",
            Some(""),
        )]));
        assert_eq!(data.saved("fake_a").hotkeys["go"], "");
        assert!(!keyboard.held().contains(&"Win+Alt+F1".to_string()));
        assert_eq!(row(&host.snapshot(), "fake_a.go").binding, "");

        // Reset gives the default back, and takes an override away.
        host.handle_request(HostRequest::SetShortcuts(vec![
            change("fake_a.go", None),
            change("fake_a.command-7", None),
        ]));
        assert_eq!(data.saved("fake_a").hotkeys["go"], "Win+Alt+F1");
        assert!(keyboard.held().contains(&"Win+Alt+F1".to_string()));
        assert!(!host.config.shortcuts.contains_key("fake_a.command-7"));
        assert!(!keyboard.held().contains(&"Win+Alt+K".to_string()));
        assert!(Config::load_in(&data.0).shortcuts.is_empty());
    }

    #[test]
    fn replace_moves_a_plugin_hotkey_to_another_command() {
        let data = Scratch::new("shortcut-replace");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, _ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        let keys = hotkeys::parse_shortcut("Win+Alt+F1").unwrap();
        host.handle_request(HostRequest::SetShortcuts(shortcuts::replace(
            "wincraft.quit",
            &keys,
            "fake_a.go",
        )));
        let snapshot = host.snapshot();
        assert_eq!(row(&snapshot, "fake_a.go").binding, "");
        assert!(shortcuts::is_changed(row(&snapshot, "fake_a.go")));
        assert_eq!(row(&snapshot, "wincraft.quit").binding, "Win+Alt+F1");
        assert!(
            row(&snapshot, "wincraft.quit").registered,
            "the keys were free again"
        );
        assert_eq!(host.shortcut_keys.len(), 1);
        assert!(host.shortcut_keys[0].2);
    }

    #[test]
    fn a_plugin_chord_waits_for_its_second_key_and_runs_the_hotkey() {
        let data = Scratch::new("shortcut-chord");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (mut host, _ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        host.handle_request(HostRequest::SetShortcuts(vec![change(
            "fake_a.go",
            Some("Win+Alt+K, G"),
        )]));
        assert_eq!(keyboard.held(), ["Win+Alt+K"]);
        let press = host.shortcut_keys[0].1.clone();
        assert_eq!(press.seconds.len(), 1);
        read(&log);

        // What the hook would have taken: a key that is not the second
        // press is never taken, so only the matching one comes here.
        host.chord_wait = Some(press.clone());
        host.wait_news(chord::News {
            taken: vec![chord::Stroke {
                vk: 0x47,
                modifiers: 0,
            }],
            ended: false,
        });
        assert_eq!(read(&log), ["hotkey fake_a 1"]);
        assert!(host.chord_wait.is_none());

        host.chord_wait = Some(press.clone());
        host.wait_news(chord::News {
            taken: vec![chord::Stroke {
                vk: 0x1B,
                modifiers: 0,
            }],
            ended: false,
        });
        assert!(read(&log).is_empty(), "Esc runs nothing");
        assert!(host.chord_wait.is_none());

        host.chord_wait = Some(press);
        host.wait_news(chord::News {
            taken: Vec::new(),
            ended: true,
        });
        assert!(host.chord_wait.is_none(), "another key or the time ends it");

        // The plugin's page shows the chord as its binding.
        assert_eq!(host.plugins.hotkey_infos(0)[0].binding, "Win+Alt+K, G");
        // Turning the plugin off frees the chord's first press.
        host.plugins.disable(0);
        host.publish();
        assert!(keyboard.held().iter().all(|keys| keys != "Win+Alt+K"));
    }

    #[test]
    fn an_old_config_without_shortcuts_keeps_every_default_hotkey() {
        let data = Scratch::new("shortcut-old");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let (host, _ui) = test_host(
            vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
            &keyboard,
            &data,
        );
        assert_eq!(keyboard.held(), ["Win+Alt+F1"]);
        assert!(host.shortcut_keys.is_empty());
        assert!(host.config.shortcuts.is_empty());
    }

    #[test]
    fn a_failed_start_from_the_settings_window_warns_with_a_balloon_that_opens_the_detector() {
        let data = Scratch::new("request-crash");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut broken = fake("broken", "Broken", "Win+Alt+F1", &log);
        broken.fails_with = Some("no device");
        let (host, _ui) = test_host(vec![broken], &keyboard, &data);
        assert_eq!(host.balloon_click, BalloonClick::Detector);
        assert!(host.plugins.warnings.is_empty(), "the balloon was shown");
        assert!(!host.plugins.slots[0].enabled);
    }

    #[test]
    fn the_strip_opens_only_while_the_plugin_that_decides_its_look_is_on() {
        let data = Scratch::new("strip-look");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
        keeper.groups = true;
        let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
        deck.look = true;
        let (mut host, ui) = test_host(vec![keeper, deck], &keyboard, &data);
        let shown = |ui: &Receiver<UiCommand>| {
            ui.try_iter()
                .filter_map(|command| match command {
                    UiCommand::ShowArrange(snapshot) => Some(snapshot),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(host.send_arrange_for(true, false));
        let sent = shown(&ui);
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].plugin, "fake_keeper",
            "the windows come from the keeper"
        );
        assert_eq!(
            sent[0].look_plugin, "fake_deck",
            "a new view is saved there"
        );
        assert!(!sent[0].preview);
        assert_eq!(sent[0].view, ArrangeView::Flat);

        host.handle_request(HostRequest::SetPluginEnabled {
            id: "fake_deck".to_string(),
            enabled: false,
        });
        assert!(!host.send_arrange_for(true, false), "the deck is off");
        assert!(shown(&ui).is_empty());
    }

    #[test]
    fn the_keeper_gives_the_strip_its_windows_and_the_deck_lists_them_without_it() {
        let data = Scratch::new("strip-source");
        let (log, keyboard) = (diary(), FakeKeys::default());
        // The deck comes first, so the keeper does not win by its place.
        let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
        deck.look = true;
        deck.groups = true;
        let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
        keeper.groups = true;
        let (mut host, ui) = test_host(vec![deck, keeper], &keyboard, &data);
        let sent_by = |ui: &Receiver<UiCommand>| {
            ui.try_iter()
                .filter_map(|command| match command {
                    UiCommand::ShowArrange(snapshot) => {
                        Some((snapshot.plugin, snapshot.keeps_order))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(host.send_arrange_for(true, false));
        assert_eq!(
            sent_by(&ui),
            [("fake_keeper".to_string(), true)],
            "a saved order: the note on other desktops shows"
        );

        host.handle_request(HostRequest::SetPluginEnabled {
            id: "fake_keeper".to_string(),
            enabled: false,
        });
        assert!(host.send_arrange_for(true, false), "the deck lists them");
        assert_eq!(sent_by(&ui), [("fake_deck".to_string(), false)]);
        // The strip's requests go to the plugin that listed the windows.
        host.arrange_action("fake_deck", &ArrangeAction::Close(0x7fff_fff0));
        assert!(log.borrow().iter().any(|line| line == "arrange fake_deck"));
    }

    #[test]
    fn with_no_plugin_to_list_windows_the_strip_stays_shut_without_a_balloon() {
        let data = Scratch::new("strip-none");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
        deck.look = true;
        let (mut host, ui) = test_host(vec![deck], &keyboard, &data);
        let _ = PluginRequests::take();
        assert!(!host.send_arrange_for(true, false));
        assert!(ui
            .try_iter()
            .all(|command| !matches!(command, UiCommand::ShowArrange(_))));
        assert!(PluginRequests::take().notices.is_empty());
    }

    #[test]
    fn win_number_opens_the_strip_for_its_button_or_gives_the_keys_back() {
        let data = Scratch::new("win-number");
        let (log, keyboard) = (diary(), FakeKeys::default());
        let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
        keeper.groups = true;
        let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
        deck.look = true;
        let (mut host, ui) = test_host(vec![keeper, deck], &keyboard, &data);
        let button = |app: &str, left: i32| taskbar::Button {
            app: app.to_string(),
            rect: [left, 1040, left + 48, 1088],
        };
        // Read moments ago, so no test reads the real taskbar.
        host.buttons = vec![
            button("Microsoft.WindowsTerminal", 0),
            button("Chrome.UserData.Profile3", 48),
        ];
        host.buttons_at = Some(std::time::Instant::now());
        let opened = |ui: &Receiver<UiCommand>| {
            ui.try_iter()
                .filter_map(|command| match command {
                    UiCommand::ShowArrange(_) => Some(0),
                    UiCommand::ArrangeSelect(place) => Some(place),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Win+2: Chrome has a window, even only one, so the strip opens on it.
        assert_eq!(host.win_digit(2, false), WinDigit::Opened);
        assert_eq!(opened(&ui), [0, 1]);
        // The same digit again before the UI has the strip's window: the
        // next card, not a second strip.
        assert_eq!(host.win_digit(2, true), WinDigit::NextCard);
        assert!(opened(&ui).is_empty());
        // Win+1: a pinned app that is not running goes back to Windows.
        assert_eq!(host.win_digit(1, false), WinDigit::GiveBack);
        assert!(opened(&ui).is_empty());
        assert!(host.arrange_anchor.is_none());
        // Win+5: no such button.
        assert_eq!(host.win_digit(5, false), WinDigit::GiveBack);
        // With WindowDeck off nothing opens, and Windows keeps the keys.
        host.handle_request(HostRequest::SetPluginEnabled {
            id: "fake_deck".to_string(),
            enabled: false,
        });
        assert_eq!(host.win_digit(2, false), WinDigit::GiveBack);
        assert!(opened(&ui).is_empty());
    }

    #[test]
    fn a_win_number_posted_from_another_thread_reaches_the_queue() {
        let hwnd = 0x7fff_fff4 as HWND;
        unsafe { wnd_proc(hwnd, WM_APP_WIN_DIGIT, 4, 1) };
        assert_eq!(PluginRequests::take().win_digits, [(4, true)]);
    }

    #[test]
    fn a_hook_queues_win_number_for_the_host() {
        open_taskbar_button(3, false);
        open_taskbar_button(3, true);
        assert_eq!(PluginRequests::take().win_digits, [(3, false), (3, true)]);
        assert!(PluginRequests::take().win_digits.is_empty());
    }

    #[test]
    fn windowdeck_takes_the_strip_settings_over_from_layoutkeeper_once() {
        let data = Scratch::new("take-over");
        let keeper = PluginConfig {
            enabled: true,
            hotkeys: [
                ("arrange".to_string(), "Win+Alt+K, A".to_string()),
                ("go".to_string(), "Win+Alt+F1".to_string()),
            ]
            .into_iter()
            .collect(),
            settings: serde_json::json!({
                "level": 2,
                "taskbar_hover_strip": true,
                "hover_delay_seconds": 0.4,
                "arrange_view": "flat",
            }),
        };
        keeper.save_in(&data.0, "layout_keeper").unwrap();
        let log = diary();
        let keyboard = FakeKeys::default();
        let plugins = || -> Vec<Box<dyn WinCraftPlugin>> {
            vec![
                Box::new(fake("layout_keeper", "LayoutKeeper", "Win+Alt+F1", &log)),
                Box::new(crate::plugins::window_deck::WindowDeck::new()),
            ]
        };
        let set = boxed_set(plugins(), &keyboard, &data);
        let deck = data.saved("window_deck");
        assert!(deck.enabled);
        assert_eq!(deck.hotkeys["arrange"], "Win+Alt+K, A", "the chord stays");
        assert_eq!(deck.settings["taskbar_hover_strip"], true);
        assert_eq!(deck.settings["arrange_view"], "flat");
        assert_eq!(
            deck.settings["hover_delay_seconds"], 0.15,
            "its own migration runs after the take-over"
        );
        assert_eq!(
            deck.settings["preview_on_hover"], true,
            "a default filled in"
        );
        let left = data.saved("layout_keeper");
        assert!(left.settings.get("taskbar_hover_strip").is_none());
        assert!(left.settings.get("arrange_view").is_none());
        assert!(!left.hotkeys.contains_key("arrange"));
        assert_eq!(left.settings["level"], 2);
        assert_eq!(
            set.config("layout_keeper").map(|c| c.hotkeys.len()),
            Some(1)
        );
        drop(set);

        // A later start finds WindowDeck's own file and takes nothing again.
        let mut changed = data.saved("window_deck");
        changed.settings["arrange_view"] = serde_json::json!("by_monitor");
        changed.save_in(&data.0, "window_deck").unwrap();
        let _again = boxed_set(plugins(), &keyboard, &data);
        assert_eq!(
            data.saved("window_deck").settings["arrange_view"],
            "by_monitor"
        );
    }

    #[test]
    fn windowdeck_off_registers_no_hotkey() {
        let data = Scratch::new("deck-off");
        PluginConfig {
            enabled: false,
            ..PluginConfig::default()
        }
        .save_in(&data.0, "window_deck")
        .unwrap();
        let keyboard = FakeKeys::default();
        let mut set = boxed_set(
            vec![Box::new(crate::plugins::window_deck::WindowDeck::new())],
            &keyboard,
            &data,
        );
        assert!(set.wanted().is_empty());
        assert!(keyboard.held().is_empty());
        set.enable(0);
        assert_eq!(keyboard.held(), ["Win+Alt+A"]);
        set.disable(0);
        assert!(keyboard.held().is_empty(), "off frees Win+Alt+A");
    }

    #[test]
    fn a_busy_host_keeps_only_the_newest_display_change() {
        let mut queue = Vec::new();
        queue_again(&mut queue, WM_DISPLAYCHANGE, 32, 1);
        queue_again(&mut queue, WM_HOTKEY, 3, 0);
        queue_again(&mut queue, WM_DISPLAYCHANGE, 32, 2);
        assert_eq!(queue, [(WM_HOTKEY, 3, 0), (WM_DISPLAYCHANGE, 32, 2)]);
        for _ in 0..RETRY_LIMIT * 2 {
            queue_again(&mut queue, WM_HOTKEY, 1, 0);
        }
        assert_eq!(queue.len(), RETRY_LIMIT);
    }

    #[test]
    fn a_plugin_request_is_queued_once_and_survives_a_full_queue() {
        let mut queue = Vec::new();
        for _ in 0..RETRY_LIMIT * 2 {
            queue_again(&mut queue, WM_HOTKEY, 1, 0);
        }
        queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
        queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
        assert_eq!(queue.len(), RETRY_LIMIT + 1);
        assert_eq!(queue.last(), Some(&(WM_APP_PLUGIN, 0, 0)));
        let mut queue = Vec::new();
        queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
        queue_again(&mut queue, WM_HOTKEY, 3, 0);
        queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
        assert_eq!(queue, [(WM_APP_PLUGIN, 0, 0), (WM_HOTKEY, 3, 0)]);
    }

    /// The host stands in as a list of the notices it showed. The handler
    /// takes the notices inside the closure, as WM_APP_PLUGIN does.
    fn deliver_plugin_requests(host: &RefCell<Option<Vec<(String, String)>>>) -> bool {
        run_unless_busy(host, |shown| shown.extend(PluginRequests::take().notices))
    }

    #[test]
    fn a_notice_that_finds_the_host_busy_is_shown_once_after_it_is_free() {
        let host = RefCell::new(Some(Vec::new()));
        NOTICES.with(|queue| {
            let mut queue = queue.borrow_mut();
            queue.push(("first".to_string(), "1".to_string()));
            queue.push(("second".to_string(), "2".to_string()));
        });
        let mut retry = Vec::new();

        let busy_guard = host.borrow_mut();
        assert!(deliver_plugin_requests(&host));
        queue_again(&mut retry, WM_APP_PLUGIN, 0, 0);
        assert_eq!(NOTICES.with(|queue| queue.borrow().len()), 2);
        drop(busy_guard);

        // A third notice and its message arrive while the first two wait.
        NOTICES.with(|queue| {
            queue
                .borrow_mut()
                .push(("third".to_string(), "3".to_string()))
        });
        queue_again(&mut retry, WM_APP_PLUGIN, 0, 0);
        assert_eq!(retry.len(), 1);

        for _ in std::mem::take(&mut retry) {
            assert!(!deliver_plugin_requests(&host));
        }
        // The fresh message the third notice posted finds nothing left.
        assert!(!deliver_plugin_requests(&host));
        let titles: Vec<String> = host
            .borrow()
            .as_ref()
            .unwrap()
            .iter()
            .map(|(title, _)| title.clone())
            .collect();
        assert_eq!(titles, ["first", "second", "third"]);
    }

    #[test]
    fn a_tooltip_and_a_prompt_wait_for_a_free_host_too() {
        let host: RefCell<Option<Vec<(String, String)>>> = RefCell::new(Some(Vec::new()));
        TRAY_TIP.with(|cell| *cell.borrow_mut() = Some(Some("tip".to_string())));
        ARRANGE_WANTED.with(|cell| cell.set(true));
        let busy_guard = host.borrow_mut();
        assert!(deliver_plugin_requests(&host));
        drop(busy_guard);
        let requests = PluginRequests::take();
        assert_eq!(requests.tip, Some(Some("tip".to_string())));
        assert!(requests.arrange);
        assert!(PluginRequests::take().tip.is_none());
    }

    #[test]
    fn no_flags_means_a_shutdown_or_restart() {
        assert_eq!(end_session_words(0), "no flags (shutdown or restart)");
    }

    #[test]
    fn closeapp_means_an_installer_or_update_is_closing_apps() {
        let words = end_session_words(ENDSESSION_CLOSEAPP as LPARAM);
        assert!(
            words.starts_with("ENDSESSION_CLOSEAPP (an installer or update"),
            "{words}"
        );
        assert!(!words.contains("shutdown"), "{words}");
    }

    #[test]
    fn logoff_and_critical_are_told_apart_and_can_come_together() {
        assert_eq!(
            end_session_words(ENDSESSION_LOGOFF as LPARAM),
            "ENDSESSION_LOGOFF (the user is signing out)"
        );
        let words = end_session_words((ENDSESSION_LOGOFF | ENDSESSION_CRITICAL) as LPARAM);
        assert!(
            words.contains("ENDSESSION_CRITICAL (Windows ends the session now"),
            "{words}"
        );
        assert!(
            words.ends_with("ENDSESSION_LOGOFF (the user is signing out)"),
            "{words}"
        );
    }

    #[test]
    fn the_high_bit_reads_the_same_sign_extended_or_not() {
        let plain = end_session_words(0x8000_0000_u32 as LPARAM);
        let extended = end_session_words(0xFFFF_FFFF_8000_0000_u64 as LPARAM);
        assert_eq!(plain, extended);
        assert!(plain.contains("ENDSESSION_LOGOFF"));
    }
}
