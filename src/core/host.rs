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
    /// About's Cancel was clicked: the download stops and its file is deleted.
    cancel: Arc<AtomicBool>,
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
            cancel: Arc::new(AtomicBool::new(false)),
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
            last_check: self.config.updates.last_check,
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
        if flags.scene.update_available {
            self.updates.newer = Some(Release {
                tag: "v9.9.9".to_string(),
                notes: "- A sample release note
- Another sample line

Nothing is downloaded."
                    .to_string(),
                page: github::repo_url().to_string(),
                assets: Vec::new(),
            });
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
        self.updates.cancel.store(false, Ordering::Relaxed);
        self.updates.status = UpdateStatus::Downloading(None);
        self.publish();
        let cancel = Arc::clone(&self.updates.cancel);
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
                    if cancel.load(Ordering::Relaxed) {
                        return false;
                    }
                    let percent = total
                        .filter(|total| *total > 0)
                        .map(|total| (done.saturating_mul(100) / total).min(100) as u8);
                    if shown != Some(percent) {
                        shown = Some(percent);
                        post(UpdateEvent::Progress(percent));
                    }
                    true
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
        // A click on Cancel that came too late to stop the download still
        // stops the update: the file is deleted and the old version stays.
        if self.updates.cancel.swap(false, Ordering::Relaxed) {
            if let Ok(path) = &result {
                let _ = std::fs::remove_file(path);
            }
            log::info!("the download of {tag} was cancelled; the old version stays");
            self.updates.busy = false;
            self.updates.reopen = None;
            self.updates.status = UpdateStatus::Idle;
            return;
        }
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
        if !chord::listen(
            self.hwnd as isize,
            WM_APP_WAIT_KEY,
            shortcuts::CHORD_WAIT,
            chord_rule(press.clone()),
        ) {
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
            hints: look.hints,
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
            let hotkey = WAIT_HOTKEY.with(|cell| cell.get());
            chord::listen(self.hwnd as isize, WM_APP_WAIT_KEY, wait, wait_rule(hotkey));
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
        if !chord::listen(
            self.hwnd as isize,
            WM_APP_WAIT_KEY,
            wait,
            wait_rule(Some(hotkey)),
        ) {
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
                if skipped_in_test_instance(&action, instance::is_test()) {
                    log::info!("read-only test instance: renaming a window skipped");
                    return;
                }
                let owner = if action.is_about_names() {
                    NAMER_ID
                } else {
                    plugin.as_str()
                };
                self.arrange_action(owner, &action);
                if !matches!(
                    action,
                    ArrangeAction::Activate(_) | ArrangeAction::NewWindow { .. }
                ) {
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
            HostRequest::CancelUpdate => {
                if self.updates.busy && matches!(self.updates.status, UpdateStatus::Downloading(_))
                {
                    self.updates.cancel.store(true, Ordering::Relaxed);
                }
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
    /// The Arrange strip's hotkey while it waits for a digit.
    static WAIT_HOTKEY: Cell<Option<Hotkey>> = const { Cell::new(None) };
    /// The hotkey that came last, as pressed and as the user names it, for
    /// a plugin that asks for `arrange_hotkey` from its `on_hotkey`.
    static LAST_HOTKEY: RefCell<Option<(Hotkey, String)>> = const { RefCell::new(None) };
}

/// The keyboard hook's rule while a chord with first press `press` waits
/// for its second press. It runs on the hook's thread, so it owns `press`.
fn chord_rule(press: FirstPress) -> impl Fn(chord::Stroke) -> chord::Verdict + Send {
    move |stroke| shortcuts::chord_verdict(shortcuts::chord_key(&press, stroke))
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

/// The keyboard hook's rule while the Arrange hotkey `hotkey` waits; see
/// `taskbar_number::wait_key`. It runs on the hook's thread, so it owns
/// the hotkey.
fn wait_rule(hotkey: Option<Hotkey>) -> impl Fn(chord::Stroke) -> chord::Verdict + Send {
    move |stroke| match hotkey {
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

/// A test instance only reads the user's real windows, so a rename typed in
/// the strip changes none of them. `--rename-window` and the palette's box
/// are the ways to rename a window the test owns.
fn skipped_in_test_instance(action: &ArrangeAction, test_instance: bool) -> bool {
    test_instance && matches!(action, ArrangeAction::Rename { .. })
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
mod tests;
