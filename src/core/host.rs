use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
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
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetForegroundWindow,
    GetMessageW, GetWindowThreadProcessId, KillTimer, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, SetTimer, TranslateMessage,
    ENDSESSION_CLOSEAPP, ENDSESSION_CRITICAL, ENDSESSION_LOGOFF, MSG, SW_SHOWNORMAL, WM_APP,
    WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP,
    WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
    WS_OVERLAPPED,
};

use serde_json::Value;

use crate::core::config::{Config, PluginConfig, DEFAULT_PALETTE_HOTKEY};
use crate::core::traits::{HostContext, Hotkey, WinCraftPlugin};
use crate::core::tray::{show_menu, MenuItem, Tray, WM_TRAY_CALLBACK};
use crate::core::ui_bridge::{
    self, ActionKind, ArrangeAction, ArrangeGroup, ArrangeSnapshot, CommandId, FieldInfo,
    HostCommand, HostRequest, HostSetting, HotkeyInfo, MonitorRect, NewRelease, Page, PaletteEntry,
    PeekCard, PluginInfo, Prompt, SettingsTarget, UiChannel, UiCommand, UiSnapshot, UpdateStatus,
    UpdateView, WM_APP_UI,
};
use crate::core::update::{self, Checked, Handover};
use crate::core::update_notice::{self, UpdateNotice};
use crate::core::{
    autostart, config, hotkeys, install, instance, release_notes, taskbar, theme, wide,
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
const WM_APP_NOTICE_CLICK: u32 = WM_APP + 23;

struct RegisteredHotkey {
    action_id: u32,
    global_id: i32,
    label: &'static str,
    hotkey: Hotkey,
    keys: String,
    ok: bool,
}

struct PluginSlot {
    plugin: Box<dyn WinCraftPlugin>,
    enabled: bool,
    registered: Vec<RegisteredHotkey>,
}

struct Host {
    hwnd: HWND,
    config: Config,
    plugins: BTreeMap<String, PluginConfig>,
    slots: Vec<PluginSlot>,
    tray: Tray,
    next_hotkey_id: i32,
    palette_hotkey: Option<(Hotkey, bool)>,
    to_ui: Arc<UiChannel>,
    host_rx: Receiver<HostRequest>,
    palette_hwnd: HWND,
    balloon_click: BalloonClick,
    /// A test scene's group and peek for the next time the strip opens.
    arrange_focus: Option<String>,
    peek_card: Option<PeekCard>,
    updates: Updates,
    /// The "Updated to" panel while it is on screen.
    update_notice: Option<UpdateNotice>,
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
    /// A test scene's `--update-now`: update as soon as the check finds one.
    auto_update: bool,
    /// What About shows under "What's new": the notes of this version.
    installed_notes: Option<String>,
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
            installed_notes: None,
            tx,
            rx,
        }
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
    /// A folder, file or address to open the palette's bookmark box for.
    pub ask_bookmark: Option<String>,
    /// Check for updates at once and install what the check finds.
    pub update_now: bool,
    /// Show the "Updated to" notice, as after an update.
    pub update_notice: bool,
}

static DEMO_RESTORE_PANEL: AtomicBool = AtomicBool::new(false);
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
    static HOST_WINDOW: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
    static NOTICES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// A tooltip a plugin asked for and the host has not applied yet; the
    /// inner None means the plain name.
    static TRAY_TIP: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    static ARRANGE_WANTED: Cell<bool> = const { Cell::new(false) };
    static PROMPT: RefCell<Option<Prompt>> = const { RefCell::new(None) };
    static ARRANGE_STALE: Cell<bool> = const { Cell::new(false) };
    static RETRY: RefCell<Vec<(u32, WPARAM, LPARAM)>> = const { RefCell::new(Vec::new()) };
    /// The message the WndProc is working on, so the log can name a message
    /// that found the host busy.
    static CURRENT_MESSAGE: Cell<Option<(u32, WPARAM)>> = const { Cell::new(None) };
}

pub fn run(config: Config, plugins: Vec<Box<dyn WinCraftPlugin>>, flags: StartupFlags) {
    DEMO_RESTORE_PANEL.store(flags.scene.restore_panel, Ordering::Relaxed);
    STARTED_AGAIN.store(
        flags.restarted || flags.updated_from.is_some() || flags.update_failed.is_some(),
        Ordering::Relaxed,
    );
    theme::set_current(config.theme);
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
        slots: plugins
            .into_iter()
            .map(|plugin| PluginSlot {
                plugin,
                enabled: false,
                registered: Vec::new(),
            })
            .collect(),
        plugins: BTreeMap::new(),
        tray: Tray::new(hwnd, hinstance),
        next_hotkey_id: 1,
        palette_hotkey: None,
        to_ui: Arc::clone(&bridge.to_ui),
        host_rx: bridge.host_rx,
        palette_hwnd: std::ptr::null_mut(),
        balloon_click: BalloonClick::Detector,
        arrange_focus: flags.scene.arrange_focus.clone(),
        peek_card: flags.scene.peek_card,
        updates: Updates::default(),
        update_notice: None,
    };
    HOST_WINDOW.with(|cell| cell.set(hwnd));
    // Notices queued before the window existed have nobody to post them.
    plugin_changed();
    TASKBAR_CREATED
        .with(|cell| cell.set(unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) }));

    host.load_plugin_configs();
    host.register_palette_hotkey();
    let wanted: Vec<usize> = (0..host.slots.len())
        .filter(|index| {
            let id = host.slots[*index].plugin.metadata().id;
            host.plugins
                .get(id)
                .map(|plugin| plugin.enabled)
                .unwrap_or(true)
        })
        .collect();
    for index in wanted {
        host.enable_slot(index);
    }
    host.save_config();

    let ui_thread = ui::start(
        bridge.ui_rx,
        Arc::clone(&bridge.to_ui),
        Arc::clone(&bridge.to_host),
        host.snapshot(),
        host.search_router(),
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
    if flags.open_palette || flags.scene.palette_query.is_some() {
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
    fn load_plugin_configs(&mut self) {
        for index in 0..self.slots.len() {
            self.load_plugin_config(index);
        }
    }

    fn load_plugin_config(&mut self, index: usize) {
        let slot = &self.slots[index];
        let meta = slot.plugin.metadata();
        let defaults: Vec<(&str, String)> = slot
            .plugin
            .hotkey_actions()
            .iter()
            .map(|action| (action.name, hotkeys::format(action.default)))
            .collect();
        let settings = slot.plugin.default_settings();

        let mut plugin = PluginConfig::load(meta.id);
        plugin.merge_defaults(&defaults, &settings);
        slot.plugin.migrate_settings(&mut plugin.settings);
        self.plugins.insert(meta.id.to_string(), plugin);
        self.save_plugin(meta.id);
    }

    fn register_palette_hotkey(&mut self) {
        let text = self.config.palette_hotkey.clone();
        let hotkey = match hotkeys::parse(&text) {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("palette_hotkey \"{text}\": {err}; using {DEFAULT_PALETTE_HOTKEY}");
                hotkeys::parse(DEFAULT_PALETTE_HOTKEY).expect("the built-in default parses")
            }
        };
        let ok = unsafe {
            RegisterHotKey(self.hwnd, PALETTE_HOTKEY_ID, hotkey.modifiers, hotkey.vk) != 0
        };
        let keys = hotkeys::format(hotkey);
        if ok {
            log::info!("registered {keys} for host.palette");
        } else {
            log::warn!("could not register {keys} for host.palette; another app holds it");
            self.balloon_click = BalloonClick::Detector;
            self.tray
                .balloon("WinCraft", &format!("{keys} is used by another app"));
        }
        self.palette_hotkey = Some((hotkey, ok));
    }

    fn plugin(&self, id: &str) -> Option<&PluginConfig> {
        self.plugins.get(id)
    }

    fn plugin_mut(&mut self, id: &str) -> &mut PluginConfig {
        self.plugins.entry(id.to_string()).or_default()
    }

    fn resolve_hotkey(&self, plugin_id: &str, name: &str, default: Hotkey) -> Hotkey {
        let text = self
            .plugin(plugin_id)
            .and_then(|plugin| plugin.hotkeys.get(name));
        match text {
            Some(text) => match hotkeys::parse(text) {
                Ok(hotkey) => hotkey,
                Err(err) => {
                    log::warn!("{plugin_id}.{name}: {err}; using the plugin default");
                    default
                }
            },
            None => default,
        }
    }

    fn enable_slot(&mut self, index: usize) {
        if self.slots[index].enabled {
            return;
        }
        let meta = self.slots[index].plugin.metadata();
        let settings = self
            .plugin(meta.id)
            .map(|plugin| plugin.settings.clone())
            .unwrap_or_else(|| serde_json::json!({}));

        let init_result = {
            let ctx = HostContext {
                hwnd: self.hwnd,
                settings: &settings,
            };
            self.slots[index].plugin.init(&ctx)
        };
        if let Err(err) = init_result {
            log::error!("{}: init failed: {err}", meta.id);
            self.balloon_click = BalloonClick::Detector;
            self.tray
                .balloon("WinCraft", &format!("{} failed to start: {err}", meta.name));
            return;
        }

        let actions: Vec<(u32, &'static str, &'static str, Hotkey)> = self.slots[index]
            .plugin
            .hotkey_actions()
            .iter()
            .map(|action| (action.id, action.name, action.label, action.default))
            .collect();

        for (action_id, name, label, default) in actions {
            let hotkey = self.resolve_hotkey(meta.id, name, default);
            let global_id = self.next_hotkey_id;
            self.next_hotkey_id += 1;
            let keys = hotkeys::format(hotkey);
            let ok =
                unsafe { RegisterHotKey(self.hwnd, global_id, hotkey.modifiers, hotkey.vk) != 0 };
            if ok {
                log::info!("registered {keys} for {}.{name}", meta.id);
            } else {
                log::warn!(
                    "could not register {keys} for {}.{name}; another app holds it",
                    meta.id
                );
                self.balloon_click = BalloonClick::Detector;
                self.tray
                    .balloon("WinCraft", &format!("{keys} is used by another app"));
            }
            self.slots[index].registered.push(RegisteredHotkey {
                action_id,
                global_id,
                label,
                hotkey,
                keys,
                ok,
            });
        }

        self.slots[index].enabled = true;
        self.plugin_mut(meta.id).enabled = true;
        self.save_plugin(meta.id);
        log::info!("{} enabled", meta.id);
    }

    fn disable_slot(&mut self, index: usize) {
        if !self.stop_slot(index) {
            return;
        }
        let id = self.slots[index].plugin.metadata().id;
        self.plugin_mut(id).enabled = false;
        self.save_plugin(id);
        log::info!("{id} disabled");
    }

    /// Frees the plugin's hotkeys and tears it down, leaving its saved
    /// enabled flag alone. False when it was not running.
    fn stop_slot(&mut self, index: usize) -> bool {
        if !self.slots[index].enabled {
            return false;
        }
        for entry in self.slots[index].registered.drain(..) {
            if entry.ok {
                unsafe { UnregisterHotKey(self.hwnd, entry.global_id) };
            }
        }
        self.slots[index].plugin.teardown();
        self.slots[index].enabled = false;
        true
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
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.enabled)
            .filter_map(|(index, slot)| {
                let action = slot.plugin.tray_actions().into_iter().next()?;
                Some(entry(
                    MENU_PLUGIN_ACTION_BASE + index as u32 * 100 + action.id,
                    action.label,
                ))
            })
            .collect();
        if !primary.is_empty() {
            items.extend(primary);
            items.push(MenuItem::Separator);
        }
        items.push(entry(MENU_QUIT, "Quit WinCraft"));
        items
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
                binding: hotkeys::format(self.resolve_hotkey(meta.id, action.name, action.default)),
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

    fn snapshot(&self) -> UiSnapshot {
        let plugins: Vec<PluginInfo> = (0..self.slots.len())
            .map(|index| {
                let slot = &self.slots[index];
                let meta = slot.plugin.metadata();
                let settings = self
                    .plugin(meta.id)
                    .map(|plugin| plugin.settings.clone())
                    .unwrap_or_else(|| serde_json::json!({}));
                PluginInfo {
                    id: meta.id.to_string(),
                    name: meta.name.to_string(),
                    version: meta.version.to_string(),
                    author: meta.author.to_string(),
                    description: meta.description.to_string(),
                    readme: meta.readme.to_string(),
                    enabled: slot.enabled,
                    config_path: config::plugin_path(meta.id),
                    hotkeys: self.hotkey_infos(index),
                    fields: field_infos(slot.plugin.as_ref(), &settings),
                    status: slot.plugin.status(),
                    page_action: page_button(slot.plugin.as_ref(), index),
                }
            })
            .collect();

        UiSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            start_with_windows: self.config.start_with_windows,
            theme: self.config.theme,
            palette_hotkey: self.config.palette_hotkey.clone(),
            palette_hotkey_registered: self.palette_hotkey.map(|(_, ok)| ok).unwrap_or(false),
            config_path: config::config_path(),
            log_path: config::log_path(),
            commands: self.palette_entries(&plugins),
            plugins,
            search: self.config.search.clone(),
            update: self.update_view(),
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
        if flags.scene.update_now {
            self.updates.auto_update = true;
            self.check_for_updates(true);
        }
    }

    /// The version that started this one waits for this; a clean quit
    /// before the timer counts too, since nothing crashed.
    fn report_healthy(&mut self, quitting: bool) {
        unsafe { KillTimer(self.hwnd, HEALTHY_TIMER) };
        let Some(from) = self.updates.healthy_pending.take() else {
            return;
        };
        if update::report_healthy() {
            log::info!("told {from} that this version is healthy");
        } else {
            log::info!("no WinCraft {from} was waiting to hear that this version is healthy");
        }
        if quitting {
            return;
        }
        if self.updates.blocked.is_none() {
            update::remove_leftovers_soon(install::dir());
        }
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
            self.updates.status =
                UpdateStatus::Error(format!("Could not put the new version in place: {err}"));
            return;
        }
        log::info!("{tag} is in place; WinCraft closes and starts it");
        update::hand_over_at_exit(Handover {
            dir,
            from: env!("CARGO_PKG_VERSION").to_string(),
            to: tag,
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
        self.updates.checking = false;
        let manual = std::mem::take(&mut self.updates.manual);
        let auto_update = std::mem::take(&mut self.updates.auto_update);
        let checked = match result {
            Ok(checked) => checked,
            Err(err) => {
                log::info!("update check failed: {err}");
                if manual {
                    self.updates.status = UpdateStatus::Offline;
                }
                return;
            }
        };
        self.config.updates.last_check = update::now_secs();
        match checked {
            Checked::NoRelease => {
                log::info!("update check: no release is published yet");
                self.updates.status = UpdateStatus::NoRelease;
                self.updates.newer = None;
            }
            Checked::UpToDate(tag) => {
                log::info!("update check: {tag} is the newest release, nothing to update");
                self.updates.status = UpdateStatus::UpToDate;
                self.updates.newer = None;
                // The check keeps the notes when the newest release is this one.
                self.updates.installed_notes = release_notes::load(env!("CARGO_PKG_VERSION"));
                self.config.updates.latest = tag;
            }
            Checked::Newer(release) => {
                log::info!("update check: {} is available", release.tag);
                self.updates.status = UpdateStatus::Idle;
                if self.config.updates.latest != release.tag {
                    self.update_notice(&format!(
                        "WinCraft {} is available \u{2014} open About to update",
                        release.tag.trim_start_matches(['v', 'V'])
                    ));
                    self.config.updates.latest = release.tag.clone();
                }
                self.updates.newer = Some(release);
                if auto_update {
                    self.update_now();
                }
            }
        }
        self.save_config();
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
            (HostCommand::Exit, "Quit WinCraft"),
        ] {
            entries.push(PaletteEntry {
                id: CommandId::Host(command),
                group: host_group.clone(),
                label: label.to_string(),
                hint: String::new(),
                disabled: false,
                plugin: None,
                subtitle: None,
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
                });
            }
        }
        entries
    }

    fn search_router(&self) -> Router {
        let mut providers: Vec<(Option<String>, Box<dyn SearchProvider>)> =
            search::providers::built_in()
                .into_iter()
                .map(|provider| (None, provider))
                .collect();
        for slot in &self.slots {
            let id = slot.plugin.metadata().id;
            for provider in slot.plugin.search_providers() {
                providers.push((Some(id.to_string()), provider));
            }
        }
        let mut router = Router::new(providers, &self.config.search.prefixes);
        router.load_usage(config::data_dir().join("palette_usage.json"));
        router
    }

    fn publish(&self) {
        self.to_ui
            .send(UiCommand::Snapshot(Box::new(self.snapshot())));
    }

    /// Asks the first enabled plugin that keeps a window order for its groups
    /// and sends them to the Arrange window.
    fn send_arrange(&mut self, open: bool) {
        // Read before the plugin builds its groups, so the mouse is still
        // where it was when the hotkey came.
        let hovered = if open {
            taskbar::app_under_cursor()
        } else {
            None
        };
        let found = self.slots.iter_mut().find_map(|slot| {
            if !slot.enabled {
                return None;
            }
            let groups = slot.plugin.window_groups()?;
            Some((slot.plugin.metadata().id, groups))
        });
        let Some((id, groups)) = found else {
            log::info!("no enabled plugin keeps a window order");
            return;
        };
        let mut focus = groups.focus;
        if open {
            let under_cursor = hovered.as_deref().and_then(|app| {
                groups
                    .groups
                    .iter()
                    .position(|group| taskbar::same_app(app, &group.key))
            });
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
                    "taskbar button under cursor"
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
            preview: groups.preview,
            can_rename: self
                .index_of(NAMER_ID)
                .is_some_and(|index| self.slots[index].enabled),
            monitors: groups.monitors,
            view: groups.view,
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
    }

    /// Asks the first enabled plugin that keeps a layout for its saved
    /// windows and sends them to the Layout page.
    fn send_layout(&mut self) {
        let can_rename = self
            .index_of(NAMER_ID)
            .is_some_and(|index| self.slots[index].enabled);
        let snapshot = self.slots.iter_mut().find_map(|slot| {
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

    fn index_of(&self, id: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.plugin.metadata().id == id)
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
            CommandId::Host(HostCommand::Exit) => self.shutdown(),
            CommandId::Host(HostCommand::TogglePlugin(index)) => {
                if index < self.slots.len() {
                    if self.slots[index].enabled {
                        self.disable_slot(index);
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
                let Some(slot) = self.slots.get_mut(index) else {
                    return;
                };
                if !slot.enabled {
                    return;
                }
                match kind {
                    ActionKind::Hotkey => slot.plugin.on_hotkey(action),
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
            HostRequest::RunAction(action) => search::actions::perform(&action, self.hwnd),
            HostRequest::PromptAnswer {
                plugin,
                id,
                target,
                text,
            } => match self.index_of(&plugin) {
                Some(index) if self.slots[index].enabled => {
                    self.slots[index].plugin.on_prompt_answer(id, target, &text)
                }
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
                match self.index_of(&plugin) {
                    Some(index) if self.slots[index].enabled => {
                        self.slots[index].plugin.on_layout_action(&action)
                    }
                    _ => log::info!("{plugin} is off; the Layout page's request was dropped"),
                }
                self.send_layout();
            }
            HostRequest::FocusWindow(hwnd) => bring_to_front(hwnd as HWND),
            HostRequest::ClearPaletteUsage => self.to_ui.send(UiCommand::ClearPaletteUsage),
            HostRequest::SetPluginEnabled { id, enabled } => {
                if let Some(index) = self.index_of(&id) {
                    if enabled {
                        self.enable_slot(index);
                    } else {
                        self.disable_slot(index);
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
            HostRequest::ResetPlugin(id) => self.reset_plugin(&id),
            HostRequest::SetHostSetting(setting) => self.set_host_setting(setting),
            HostRequest::CheckForUpdates => self.check_for_updates(true),
            HostRequest::UpdateNow => self.update_now(),
            HostRequest::OpenPath(path) => open_in_notepad(&path),
            HostRequest::Exit => self.shutdown(),
        }
    }

    fn arrange_action(&mut self, plugin: &str, action: &ArrangeAction) {
        match self.index_of(plugin) {
            Some(index) if self.slots[index].enabled => {
                self.slots[index].plugin.on_arrange_action(action)
            }
            _ => log::info!("{plugin} is off; the strip's request was dropped"),
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
        self.plugin_mut(plugin_id)
            .hotkeys
            .insert(action.to_string(), hotkeys::format(hotkey));
        self.save_plugin(plugin_id);

        if let Some(index) = self.index_of(plugin_id) {
            if self.slots[index].enabled {
                self.disable_slot(index);
                self.enable_slot(index);
            }
        }
        log::info!(
            "{plugin_id}.{action} rebound to {}",
            hotkeys::format(hotkey)
        );
        self.publish();
    }

    fn set_setting(&mut self, plugin_id: &str, key: &str, value: Value) {
        {
            let plugin = self.plugin_mut(plugin_id);
            if !plugin.settings.is_object() {
                plugin.settings = serde_json::json!({});
            }
            if key.starts_with('/') {
                config::set_pointer(&mut plugin.settings, key, value);
            } else if let Some(object) = plugin.settings.as_object_mut() {
                object.insert(key.to_string(), value);
            }
        }
        self.save_plugin(plugin_id);

        let settings = self
            .plugin(plugin_id)
            .map(|plugin| plugin.settings.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(index) = self.index_of(plugin_id) {
            if self.slots[index].enabled {
                let applied = self.slots[index].plugin.on_settings_changed(&settings);
                if !applied {
                    self.disable_slot(index);
                    self.enable_slot(index);
                }
            }
        }
        self.publish();
    }

    fn reset_plugin(&mut self, plugin_id: &str) {
        if let Err(err) = PluginConfig::reset(plugin_id) {
            log::error!("{err}");
            return;
        }
        let Some(index) = self.index_of(plugin_id) else {
            return;
        };
        let was_enabled = self.slots[index].enabled;
        if was_enabled {
            self.disable_slot(index);
        }
        self.plugins.remove(plugin_id);
        self.load_plugin_config(index);
        if was_enabled {
            self.enable_slot(index);
        }
        log::info!("{plugin_id} reset to defaults");
        self.publish();
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
            HostSetting::CheckUpdatesDaily(on) => {
                self.config.updates.check_daily = on;
                log::info!("check for updates daily: {on}");
            }
            HostSetting::Theme(choice) => {
                self.config.theme = choice;
                theme::set_current(choice);
                self.to_ui.send(UiCommand::ThemeChanged);
            }
            HostSetting::PaletteHotkey(binding) => match hotkeys::parse(&binding) {
                Ok(hotkey) => {
                    if let Some((_, true)) = self.palette_hotkey {
                        unsafe { UnregisterHotKey(self.hwnd, PALETTE_HOTKEY_ID) };
                    }
                    self.config.palette_hotkey = hotkeys::format(hotkey);
                    self.register_palette_hotkey();
                }
                Err(err) => log::warn!("palette hotkey: {err}"),
            },
        }
        self.save_config();
        self.publish();
    }

    fn save_config(&self) {
        if let Err(err) = self.config.save() {
            log::error!("could not save config.json: {err}");
        }
    }

    fn save_plugin(&self, id: &str) {
        let Some(plugin) = self.plugins.get(id) else {
            return;
        };
        if let Err(err) = plugin.save(id) {
            log::error!("could not save the settings for {id}: {err}");
        }
    }

    /// Quitting stops every plugin but must not save it as disabled, or
    /// the next start would come up with every plugin switched off.
    fn shutdown(&mut self) {
        log::info!("WinCraft is shutting down");
        self.report_healthy(true);
        self.to_ui.send(UiCommand::Quit);
        for index in 0..self.slots.len() {
            if self.stop_slot(index) {
                log::info!("{} stopped", self.slots[index].plugin.metadata().id);
            }
        }
        if let Some((_, true)) = self.palette_hotkey {
            unsafe { UnregisterHotKey(self.hwnd, PALETTE_HOTKEY_ID) };
        }
        self.tray.remove();
        unsafe { PostQuitMessage(0) };
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

/// Opens the Arrange windows list. Safe to call from any plugin callback.
pub fn open_arrange() {
    ARRANGE_WANTED.with(|cell| cell.set(true));
    plugin_changed();
}

/// Sends the Arrange strip fresh data if it is open, for changes that show
/// a moment after the plugin made them. Safe to call from any callback.
pub fn refresh_arrange() {
    ARRANGE_STALE.with(|cell| cell.set(true));
    plugin_changed();
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
        let Some(host) = borrowed.as_ref() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for slot in host.slots.iter().filter(|slot| slot.enabled) {
            let plugin = slot.plugin.metadata().name;
            for entry in slot.registered.iter().filter(|entry| entry.ok) {
                found.push((plugin.to_string(), entry.label.to_string(), entry.hotkey));
            }
        }
        found
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
    stale: bool,
    prompt: Option<Prompt>,
}

impl PluginRequests {
    fn take() -> Self {
        PluginRequests {
            notices: NOTICES.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
            tip: TRAY_TIP.with(|cell| cell.borrow_mut().take()),
            arrange: ARRANGE_WANTED.with(|cell| cell.replace(false)),
            stale: ARRANGE_STALE.with(|cell| cell.replace(false)),
            prompt: PROMPT.with(|cell| cell.borrow_mut().take()),
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
                if let Some(slot) = host.slots.get_mut(index) {
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
        with_host_or_later(hwnd, msg, wparam, lparam, |host| host.tray.add());
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
                let target = host.slots.iter().enumerate().find_map(|(index, slot)| {
                    slot.registered
                        .iter()
                        .find(|entry| entry.global_id == global_id)
                        .map(|entry| (index, entry.action_id))
                });
                if let Some((index, action_id)) = target {
                    log::debug!(
                        "hotkey {global_id} goes to {}, action {action_id}",
                        host.slots[index].plugin.metadata().id
                    );
                    if host.slots[index].enabled {
                        host.slots[index].plugin.on_hotkey(action_id);
                    }
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
                if requests.arrange {
                    host.send_arrange(true);
                } else if requests.stale {
                    host.send_arrange(false);
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
                for slot in host.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.plugin.on_windows_message(msg, wparam, lparam);
                }
            });
            0
        }
        WM_SETTINGCHANGE | WM_POWERBROADCAST | WM_TIMER => {
            if msg == WM_SETTINGCHANGE && is_colour_change(lparam) {
                with_host(|host| {
                    host.tray.refresh_icon();
                    host.to_ui.send(UiCommand::ThemeChanged);
                });
            }
            with_host(|host| {
                for slot in host.slots.iter_mut().filter(|slot| slot.enabled) {
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
                for slot in host.slots.iter_mut().filter(|slot| slot.enabled) {
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
