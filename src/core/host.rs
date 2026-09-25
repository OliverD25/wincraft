use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
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
    GetMessageW, GetWindowThreadProcessId, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TranslateMessage, MSG, SW_SHOWNORMAL, WM_APP,
    WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP,
    WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
    WS_OVERLAPPED,
};

use serde_json::Value;

use crate::core::config::{Config, PluginConfig, DEFAULT_PALETTE_HOTKEY};
use crate::core::traits::{HostContext, Hotkey, WinCraftPlugin};
use crate::core::tray::{show_menu, MenuItem, Tray, WM_TRAY_CALLBACK};
use crate::core::ui_bridge::{
    self, ActionKind, ArrangeAction, ArrangeSnapshot, CommandId, FieldInfo, HostCommand,
    HostRequest, HostSetting, HotkeyInfo, MonitorRect, Page, PaletteEntry, PluginInfo, UiChannel,
    UiCommand, UiSnapshot, WM_APP_UI,
};
use crate::core::{autostart, config, hotkeys, theme, wide};
use crate::search::{self, Router, SearchProvider};
use crate::ui;

const CLASS_NAME: &str = "WinCraftHost";

const MENU_QUIT: u32 = 5;
const MENU_PALETTE: u32 = 6;
const MENU_SETTINGS: u32 = 7;
const MENU_PLUGIN_ACTION_BASE: u32 = 1000;

const DETECTOR_ID: &str = "shortcut_detector";
const DETECTOR_OPEN_ACTION: u32 = 1;

/// The palette belongs to the host, not to a plugin, so it takes the one id
/// that plugin hotkeys never use.
const PALETTE_HOTKEY_ID: i32 = 0;

/// Posted by `notify` and `plugin_changed`, which plugins call while the host
/// is busy calling them; the work happens when the message comes round.
const WM_APP_PLUGIN: u32 = WM_APP + 21;

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
    /// Clicking a "hotkey is taken" balloon opens ShortcutDetector; clicking
    /// a plugin's own notice must not.
    balloon_opens_detector: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StartupFlags {
    pub open_detector: bool,
    pub open_palette: bool,
    pub open_arrange: bool,
    pub open_settings: bool,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// Kept outside Host so the WndProc can check it without borrowing. Windows
    /// re-enters a WndProc whenever it likes, and a borrow taken on every single
    /// message would turn that into a crash.
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static HOST_WINDOW: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
    static NOTICES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    static ARRANGE_WANTED: Cell<bool> = const { Cell::new(false) };
    static ARRANGE_STALE: Cell<bool> = const { Cell::new(false) };
}

pub fn run(config: Config, plugins: Vec<Box<dyn WinCraftPlugin>>, flags: StartupFlags) {
    theme::set_current(config.theme);
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };

    let class_name = wide(CLASS_NAME);
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
        balloon_opens_detector: true,
    };
    HOST_WINDOW.with(|cell| cell.set(hwnd));
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

    ui::start(
        bridge.ui_rx,
        Arc::clone(&bridge.to_ui),
        Arc::clone(&bridge.to_host),
        host.snapshot(),
        host.search_router(),
    );

    host.tray.add();
    log::info!("WinCraft {} is running", env!("CARGO_PKG_VERSION"));

    HOST.with(|cell| *cell.borrow_mut() = Some(host));

    if flags.open_detector {
        open_shortcut_detector();
    }
    if flags.open_palette {
        show_palette();
    }
    if flags.open_arrange {
        open_arrange();
    }
    if flags.open_settings {
        with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::General)));
    }

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    HOST.with(|cell| *cell.borrow_mut() = None);
    log::info!("WinCraft stopped");
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
            self.balloon_opens_detector = true;
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
            self.balloon_opens_detector = true;
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
                self.balloon_opens_detector = true;
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
        if !self.slots[index].enabled {
            return;
        }
        for entry in self.slots[index].registered.drain(..) {
            if entry.ok {
                unsafe { UnregisterHotKey(self.hwnd, entry.global_id) };
            }
        }
        self.slots[index].plugin.teardown();
        self.slots[index].enabled = false;
        let id = self.slots[index].plugin.metadata().id;
        self.plugin_mut(id).enabled = false;
        self.save_plugin(id);
        log::info!("{id} disabled");
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
            MenuItem::Separator,
        ];
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
                    fields: slot
                        .plugin
                        .settings_fields()
                        .into_iter()
                        .map(|field| FieldInfo {
                            key: field.key.to_string(),
                            label: field.label.to_string(),
                            help: field.help.to_string(),
                            kind: field.kind,
                            value: settings.get(field.key).cloned().unwrap_or(Value::Null),
                        })
                        .collect(),
                    status: slot.plugin.status(),
                    page_action: slot.plugin.page_action().and_then(|action| {
                        slot.plugin
                            .hotkey_actions()
                            .into_iter()
                            .find(|hotkey| hotkey.id == action)
                            .map(|hotkey| {
                                (
                                    hotkey.label.to_string(),
                                    CommandId::Plugin {
                                        index,
                                        kind: ActionKind::Hotkey,
                                        action,
                                    },
                                )
                            })
                    }),
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
        }
    }

    fn palette_entries(&self, plugins: &[PluginInfo]) -> Vec<PaletteEntry> {
        let mut entries = Vec::new();
        let host_group = "WinCraft".to_string();
        for (command, label) in [
            (HostCommand::OpenSettings, "Open settings"),
            (HostCommand::OpenStore, "Plugin store"),
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
        Router::new(providers, &self.config.search.prefixes)
    }

    fn publish(&self) {
        self.to_ui
            .send(UiCommand::Snapshot(Box::new(self.snapshot())));
    }

    /// Asks the first enabled plugin that keeps a window order for its groups
    /// and sends them to the Arrange window.
    fn send_arrange(&mut self, open: bool) {
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
        let snapshot = ArrangeSnapshot {
            plugin: id.to_string(),
            groups: groups.groups,
            desktops: groups.desktops,
            focus: groups.focus,
            watched: groups.watched,
        };
        self.to_ui.send(if open {
            UiCommand::ShowArrange(snapshot)
        } else {
            UiCommand::ArrangeUpdate(snapshot)
        });
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
            HostRequest::Arrange { plugin, action } => {
                if let Some(index) = self.index_of(&plugin) {
                    if self.slots[index].enabled {
                        self.slots[index].plugin.on_arrange_action(&action);
                    }
                }
                if !matches!(action, ArrangeAction::Activate(_)) {
                    self.send_arrange(false);
                }
            }
            HostRequest::FocusWindow(hwnd) => bring_to_front(hwnd as HWND),
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
            HostRequest::OpenPath(path) => open_in_notepad(&path),
            HostRequest::Exit => self.shutdown(),
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
            if let Some(object) = plugin.settings.as_object_mut() {
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

    fn shutdown(&mut self) {
        self.to_ui.send(UiCommand::Quit);
        for index in 0..self.slots.len() {
            self.disable_slot(index);
        }
        if let Some((_, true)) = self.palette_hotkey {
            unsafe { UnregisterHotKey(self.hwnd, PALETTE_HOTKEY_ID) };
        }
        self.tray.remove();
        unsafe { PostQuitMessage(0) };
    }
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
/// host window before the first call has returned; dropping those is right,
/// crashing the tray program is not.
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| match cell.try_borrow_mut() {
        Ok(mut borrowed) => borrowed.as_mut().map(f),
        Err(_) => {
            log::warn!("host message arrived while the host was busy, ignored");
            None
        }
    })
}

fn handle_menu_choice(choice: u32) {
    match choice {
        MENU_PALETTE => show_palette(),
        MENU_SETTINGS => {
            with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::General)));
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
    let taskbar_created = TASKBAR_CREATED.with(|cell| cell.get());
    if taskbar_created != 0 && msg == taskbar_created {
        with_host(|host| host.tray.add());
        return 0;
    }

    match msg {
        WM_HOTKEY => {
            let global_id = wparam as i32;
            if global_id == PALETTE_HOTKEY_ID {
                show_palette();
                return 0;
            }
            with_host(|host| {
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
            let event = (lparam as u32) & 0xFFFF;
            if event == NIN_BALLOONUSERCLICK {
                let opens = with_host(|host| host.balloon_opens_detector).unwrap_or(false);
                if opens {
                    open_shortcut_detector();
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
            let notices = NOTICES.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
            let arrange = ARRANGE_WANTED.with(|cell| cell.replace(false));
            let stale = ARRANGE_STALE.with(|cell| cell.replace(false));
            with_host(|host| {
                for (title, text) in &notices {
                    host.balloon_opens_detector = false;
                    host.tray.balloon(title, text);
                }
                if arrange {
                    host.send_arrange(true);
                } else if stale {
                    host.send_arrange(false);
                }
                host.publish();
            });
            0
        }
        WM_APP_UI => {
            with_host(|host| {
                while let Ok(request) = host.host_rx.try_recv() {
                    host.handle_request(request);
                }
            });
            0
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_POWERBROADCAST | WM_TIMER => {
            if msg == WM_SETTINGCHANGE && is_colour_change(lparam) {
                with_host(|host| {
                    host.tray.refresh_icon();
                    host.to_ui.send(UiCommand::ThemeChanged);
                });
            }
            if msg == WM_DISPLAYCHANGE {
                // The palette was placed against a monitor layout that no
                // longer exists, so it is hidden rather than left stranded.
                with_host(|host| host.to_ui.send(UiCommand::HideAll));
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
