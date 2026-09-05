use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
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
    GetMessageW, GetWindowThreadProcessId, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TranslateMessage, MSG, SW_SHOWNORMAL,
    WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_HOTKEY, WM_LBUTTONUP, WM_POWERBROADCAST,
    WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use crate::core::about::{self, AboutHotkey, AboutModule};
use serde_json::Value;

use crate::core::config::{Config, PluginConfig, DEFAULT_PALETTE_HOTKEY};
use crate::core::traits::{HostContext, Hotkey, WinCraftModule};
use crate::core::tray::{show_menu, MenuItem, Tray, WM_TRAY_CALLBACK};
use crate::core::ui_bridge::{
    self, ActionKind, CommandId, FieldInfo, HostCommand, HostRequest, HostSetting,
    HotkeyInfo, MonitorRect, Page, PaletteEntry, PluginInfo, UiChannel, UiCommand, UiSnapshot,
    WM_APP_UI,
};
use crate::core::{autostart, config, hotkeys, wide};
use crate::ui;

const CLASS_NAME: &str = "WinCraftHost";

const MENU_ABOUT: u32 = 1;
const MENU_EDIT_CONFIG: u32 = 2;
const MENU_OPEN_LOG: u32 = 3;
const MENU_AUTOSTART: u32 = 4;
const MENU_EXIT: u32 = 5;
const MENU_PALETTE: u32 = 6;
const MENU_SETTINGS: u32 = 7;
const MENU_MODULE_BASE: u32 = 100;
const MENU_MODULE_ACTION_BASE: u32 = 1000;

const DETECTOR_ID: &str = "shortcut_detector";
const DETECTOR_OPEN_ACTION: u32 = 1;

/// The palette belongs to the host, not to a plugin, so it takes the one id
/// that module hotkeys never use.
const PALETTE_HOTKEY_ID: i32 = 0;

struct RegisteredHotkey {
    action_id: u32,
    global_id: i32,
    label: &'static str,
    hotkey: Hotkey,
    keys: String,
    ok: bool,
}

struct ModuleSlot {
    module: Box<dyn WinCraftModule>,
    enabled: bool,
    registered: Vec<RegisteredHotkey>,
}

struct Host {
    hwnd: HWND,
    config: Config,
    plugins: BTreeMap<String, PluginConfig>,
    slots: Vec<ModuleSlot>,
    tray: Tray,
    next_hotkey_id: i32,
    palette_hotkey: Option<(Hotkey, bool)>,
    to_ui: Arc<UiChannel>,
    host_rx: Receiver<HostRequest>,
    palette_hwnd: HWND,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StartupFlags {
    pub open_detector: bool,
    pub open_palette: bool,
    pub open_settings: bool,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    /// Kept outside Host so the WndProc can check it without borrowing. Windows
    /// re-enters a WndProc whenever it likes, and a borrow taken on every single
    /// message would turn that into a crash.
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

pub fn run(config: Config, modules: Vec<Box<dyn WinCraftModule>>, flags: StartupFlags) {
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
    // see WM_DISPLAYCHANGE to tell modules that the monitor layout changed.
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
        slots: modules
            .into_iter()
            .map(|module| ModuleSlot {
                module,
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
    };
    TASKBAR_CREATED
        .with(|cell| cell.set(unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) }));

    host.load_plugin_configs();
    host.register_palette_hotkey();
    let wanted: Vec<usize> = (0..host.slots.len())
        .filter(|index| {
            let id = host.slots[*index].module.metadata().id;
            host.plugins.get(id).map(|plugin| plugin.enabled).unwrap_or(true)
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
        let meta = slot.module.metadata();
        let defaults: Vec<(&str, String)> = slot
            .module
            .hotkey_actions()
            .iter()
            .map(|action| (action.name, hotkeys::format(action.default)))
            .collect();
        let settings = slot.module.default_settings();

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

    fn resolve_hotkey(&self, module_id: &str, name: &str, default: Hotkey) -> Hotkey {
        let text = self
            .plugin(module_id)
            .and_then(|plugin| plugin.hotkeys.get(name));
        match text {
            Some(text) => match hotkeys::parse(text) {
                Ok(hotkey) => hotkey,
                Err(err) => {
                    log::warn!("{module_id}.{name}: {err}; using the module default");
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
        let meta = self.slots[index].module.metadata();
        let settings = self
            .plugin(meta.id)
            .map(|plugin| plugin.settings.clone())
            .unwrap_or_else(|| serde_json::json!({}));

        let init_result = {
            let ctx = HostContext {
                hwnd: self.hwnd,
                settings: &settings,
            };
            self.slots[index].module.init(&ctx)
        };
        if let Err(err) = init_result {
            log::error!("{}: init failed: {err}", meta.id);
            self.tray
                .balloon("WinCraft", &format!("{} failed to start: {err}", meta.name));
            return;
        }

        let actions: Vec<(u32, &'static str, &'static str, Hotkey)> = self.slots[index]
            .module
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
        self.slots[index].module.teardown();
        self.slots[index].enabled = false;
        let id = self.slots[index].module.metadata().id;
        self.plugin_mut(id).enabled = false;
        self.save_plugin(id);
        log::info!("{id} disabled");
    }

    fn menu_items(&self) -> Vec<MenuItem> {
        let mut items = vec![
            MenuItem::Entry {
                id: MENU_PALETTE,
                label: "Open palette".to_string(),
                checked: false,
            },
            MenuItem::Entry {
                id: MENU_SETTINGS,
                label: "Settings\u{2026}".to_string(),
                checked: false,
            },
            MenuItem::Separator,
        ];
        for (index, slot) in self.slots.iter().enumerate() {
            let meta = slot.module.metadata();
            items.push(MenuItem::Entry {
                id: MENU_MODULE_BASE + index as u32,
                label: meta.name.to_string(),
                checked: slot.enabled,
            });
            if slot.enabled {
                for action in slot.module.tray_actions() {
                    items.push(MenuItem::Entry {
                        id: MENU_MODULE_ACTION_BASE + index as u32 * 100 + action.id,
                        label: format!("    {}", action.label),
                        checked: false,
                    });
                }
            }
        }
        items.push(MenuItem::Separator);
        items.push(MenuItem::Entry {
            id: MENU_AUTOSTART,
            label: "Start with Windows".to_string(),
            checked: self.config.start_with_windows,
        });
        items.push(MenuItem::Entry {
            id: MENU_EDIT_CONFIG,
            label: "Edit config".to_string(),
            checked: false,
        });
        items.push(MenuItem::Entry {
            id: MENU_OPEN_LOG,
            label: "Open log".to_string(),
            checked: false,
        });
        items.push(MenuItem::Entry {
            id: MENU_ABOUT,
            label: "About WinCraft".to_string(),
            checked: false,
        });
        items.push(MenuItem::Separator);
        items.push(MenuItem::Entry {
            id: MENU_EXIT,
            label: "Exit".to_string(),
            checked: false,
        });
        items
    }

    fn about_text(&self) -> String {
        let modules: Vec<AboutModule> = self
            .slots
            .iter()
            .map(|slot| {
                let meta = slot.module.metadata();
                let keys = if slot.enabled {
                    slot.registered
                        .iter()
                        .map(|entry| AboutHotkey {
                            keys: entry.keys.clone(),
                            label: entry.label.to_string(),
                            registered: entry.ok,
                        })
                        .collect()
                } else {
                    slot.module
                        .hotkey_actions()
                        .iter()
                        .map(|action| AboutHotkey {
                            keys: hotkeys::format(self.resolve_hotkey(
                                meta.id,
                                action.name,
                                action.default,
                            )),
                            label: action.label.to_string(),
                            registered: true,
                        })
                        .collect()
                };
                AboutModule {
                    name: meta.name.to_string(),
                    version: meta.version.to_string(),
                    author: meta.author.to_string(),
                    description: meta.description.to_string(),
                    enabled: slot.enabled,
                    hotkeys: keys,
                }
            })
            .collect();
        about::text(&modules)
    }

    fn hotkey_infos(&self, index: usize) -> Vec<HotkeyInfo> {
        let slot = &self.slots[index];
        let meta = slot.module.metadata();
        if slot.enabled {
            return slot
                .registered
                .iter()
                .map(|entry| HotkeyInfo {
                    action: self.action_name(index, entry.action_id).to_string(),
                    label: entry.label.to_string(),
                    binding: entry.keys.clone(),
                    registered: entry.ok,
                })
                .collect();
        }
        slot.module
            .hotkey_actions()
            .iter()
            .map(|action| HotkeyInfo {
                action: action.name.to_string(),
                label: action.label.to_string(),
                binding: hotkeys::format(self.resolve_hotkey(meta.id, action.name, action.default)),
                registered: true,
            })
            .collect()
    }

    fn action_name(&self, index: usize, action_id: u32) -> &'static str {
        self.slots[index]
            .module
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
                let meta = slot.module.metadata();
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
                        .module
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
        }
    }

    fn palette_entries(&self, plugins: &[PluginInfo]) -> Vec<PaletteEntry> {
        let mut entries = Vec::new();
        let host_group = "WinCraft".to_string();
        for (command, label) in [
            (HostCommand::OpenSettings, "Settings"),
            (HostCommand::OpenStore, "Plugin store"),
            (HostCommand::OpenAbout, "About WinCraft"),
            (HostCommand::OpenConfig, "Open the config file"),
            (HostCommand::OpenLog, "Open the log file"),
            (HostCommand::Exit, "Exit WinCraft"),
        ] {
            entries.push(PaletteEntry {
                id: CommandId::Host(command),
                group: host_group.clone(),
                label: label.to_string(),
                hint: String::new(),
            });
        }

        for (index, slot) in self.slots.iter().enumerate() {
            let meta = slot.module.metadata();
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
            });
            if !slot.enabled {
                continue;
            }
            let bindings = plugins.get(index).map(|info| &info.hotkeys);
            for action in slot.module.hotkey_actions() {
                let hint = bindings
                    .and_then(|list| list.iter().find(|info| info.label == action.label))
                    .map(|info| info.binding.clone())
                    .unwrap_or_default();
                entries.push(PaletteEntry {
                    id: CommandId::Module {
                        index,
                        kind: ActionKind::Hotkey,
                        action: action.id,
                    },
                    group: group.clone(),
                    label: action.label.to_string(),
                    hint,
                });
            }
            for action in slot.module.tray_actions() {
                if entries.iter().any(|entry| {
                    entry.group == group && entry.label == action.label
                }) {
                    continue;
                }
                entries.push(PaletteEntry {
                    id: CommandId::Module {
                        index,
                        kind: ActionKind::Tray,
                        action: action.id,
                    },
                    group: group.clone(),
                    label: action.label.to_string(),
                    hint: String::new(),
                });
            }
            for command in slot.module.palette_commands() {
                entries.push(PaletteEntry {
                    id: CommandId::Module {
                        index,
                        kind: ActionKind::Palette,
                        action: command.id,
                    },
                    group: group.clone(),
                    label: command.label.to_string(),
                    hint: command.hint.to_string(),
                });
            }
        }
        entries
    }

    fn publish(&self) {
        self.to_ui.send(UiCommand::Snapshot(Box::new(self.snapshot())));
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.module.metadata().id == id)
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
            CommandId::Module {
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
                    ActionKind::Hotkey => slot.module.on_hotkey(action),
                    ActionKind::Tray => slot.module.on_tray_action(action),
                    ActionKind::Palette => slot.module.on_palette_command(action),
                }
            }
        }
    }

    fn handle_request(&mut self, request: HostRequest) {
        match request {
            HostRequest::UiReady { palette_hwnd } => {
                self.palette_hwnd = palette_hwnd as HWND;
                round_the_corners(self.palette_hwnd);
                self.publish();
            }
            HostRequest::RunCommand(id) => self.run_command(id),
            HostRequest::SetModuleEnabled { id, enabled } => {
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
                module,
                action,
                binding,
            } => self.set_hotkey(&module, &action, &binding),
            HostRequest::SetSetting {
                module,
                key,
                value,
            } => self.set_setting(&module, &key, value),
            HostRequest::ResetModule(id) => self.reset_plugin(&id),
            HostRequest::SetHostSetting(setting) => self.set_host_setting(setting),
            HostRequest::OpenPath(path) => open_in_notepad(&path),
            HostRequest::Exit => self.shutdown(),
        }
    }

    fn set_hotkey(&mut self, module: &str, action: &str, binding: &str) {
        let hotkey = match hotkeys::parse(binding) {
            Ok(hotkey) => hotkey,
            Err(err) => {
                log::warn!("{module}.{action}: {err}");
                return;
            }
        };
        self.plugin_mut(module)
            .hotkeys
            .insert(action.to_string(), hotkeys::format(hotkey));
        self.save_plugin(module);

        if let Some(index) = self.index_of(module) {
            if self.slots[index].enabled {
                self.disable_slot(index);
                self.enable_slot(index);
            }
        }
        log::info!("{module}.{action} rebound to {}", hotkeys::format(hotkey));
        self.publish();
    }

    fn set_setting(&mut self, module: &str, key: &str, value: Value) {
        {
            let plugin = self.plugin_mut(module);
            if !plugin.settings.is_object() {
                plugin.settings = serde_json::json!({});
            }
            if let Some(object) = plugin.settings.as_object_mut() {
                object.insert(key.to_string(), value);
            }
        }
        self.save_plugin(module);

        let settings = self
            .plugin(module)
            .map(|plugin| plugin.settings.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(index) = self.index_of(module) {
            if self.slots[index].enabled {
                let applied = self.slots[index].module.on_settings_changed(&settings);
                if !applied {
                    self.disable_slot(index);
                    self.enable_slot(index);
                }
            }
        }
        self.publish();
    }

    fn reset_plugin(&mut self, module: &str) {
        if let Err(err) = PluginConfig::reset(module) {
            log::error!("{err}");
            return;
        }
        let Some(index) = self.index_of(module) else {
            return;
        };
        let was_enabled = self.slots[index].enabled;
        if was_enabled {
            self.disable_slot(index);
        }
        self.plugins.remove(module);
        self.load_plugin_config(index);
        if was_enabled {
            self.enable_slot(index);
        }
        log::info!("{module} reset to defaults");
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
            HostSetting::Theme(choice) => {
                self.config.theme = choice;
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

/// The list the ShortcutDetector marks as WinCraft's own.
///
/// It takes an immutable borrow, so calling it while the host is mutably
/// borrowed panics instead of quietly misbehaving. That is the re-entrancy rule
/// with teeth: a module may call this from its own WndProc, never from inside
/// on_hotkey or on_tray_action.
pub fn registered_hotkeys() -> Vec<(String, String, Hotkey)> {
    HOST.with(|cell| {
        let borrowed = cell.borrow();
        let Some(host) = borrowed.as_ref() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for slot in host.slots.iter().filter(|slot| slot.enabled) {
            let module = slot.module.metadata().name;
            for entry in slot.registered.iter().filter(|entry| entry.ok) {
                found.push((module.to_string(), entry.label.to_string(), entry.hotkey));
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
fn bring_to_front(hwnd: HWND) {
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

fn round_the_corners(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let preference = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &preference as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&preference) as u32,
        )
    };
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
            .find(|slot| slot.enabled && slot.module.metadata().id == DETECTOR_ID);
        match slot {
            Some(slot) => slot.module.on_tray_action(DETECTOR_OPEN_ACTION),
            None => log::info!("no enabled {DETECTOR_ID} module to open"),
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
        MENU_ABOUT => {
            // About runs MessageBoxW, which pumps messages and re-enters the
            // WndProc, so the text is built and the borrow released first.
            let Some((text, hwnd)) = with_host(|host| (host.about_text(), host.hwnd)) else {
                return;
            };
            about::show(hwnd, &text);
        }
        MENU_EDIT_CONFIG => open_in_notepad(&config::config_path()),
        MENU_OPEN_LOG => open_in_notepad(&config::log_path()),
        MENU_AUTOSTART => {
            with_host(|host| {
                let wanted = !host.config.start_with_windows;
                match autostart::set(wanted) {
                    Ok(()) => {
                        host.config.start_with_windows = wanted;
                        host.save_config();
                        log::info!("start with Windows: {wanted}");
                    }
                    Err(err) => log::error!("could not change autostart: {err}"),
                }
            });
        }
        MENU_EXIT => {
            with_host(|host| host.shutdown());
        }
        MENU_PALETTE => show_palette(),
        MENU_SETTINGS => {
            with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::General)));
        }
        id if (MENU_MODULE_BASE..MENU_MODULE_ACTION_BASE).contains(&id) => {
            with_host(|host| {
                let index = (id - MENU_MODULE_BASE) as usize;
                if index >= host.slots.len() {
                    return;
                }
                if host.slots[index].enabled {
                    host.disable_slot(index);
                } else {
                    host.enable_slot(index);
                }
                host.publish();
            });
        }
        id if id >= MENU_MODULE_ACTION_BASE => {
            with_host(|host| {
                let offset = id - MENU_MODULE_ACTION_BASE;
                let index = (offset / 100) as usize;
                let action_id = offset % 100;
                if let Some(slot) = host.slots.get_mut(index) {
                    if slot.enabled {
                        slot.module.on_tray_action(action_id);
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
                    if host.slots[index].enabled {
                        host.slots[index].module.on_hotkey(action_id);
                    }
                }
            });
            0
        }
        WM_TRAY_CALLBACK => {
            let event = (lparam as u32) & 0xFFFF;
            if event == NIN_BALLOONUSERCLICK {
                open_shortcut_detector();
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
                with_host(|host| host.to_ui.send(UiCommand::ThemeChanged));
            }
            with_host(|host| {
                for slot in host.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.module.on_windows_message(msg, wparam, lparam);
                }
            });
            0
        }
        WM_DESTROY => {
            with_host(|host| host.shutdown());
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
