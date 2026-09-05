use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, TranslateMessage, MSG, SW_SHOWNORMAL, WM_CONTEXTMENU,
    WM_DESTROY, WM_DISPLAYCHANGE, WM_HOTKEY, WM_LBUTTONUP, WM_POWERBROADCAST, WM_RBUTTONUP,
    WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use crate::core::about::{self, AboutHotkey, AboutModule};
use crate::core::config::Config;
use crate::core::traits::{HostContext, Hotkey, WinCraftModule};
use crate::core::tray::{show_menu, MenuItem, Tray, WM_TRAY_CALLBACK};
use crate::core::{autostart, config, hotkeys, wide};

const CLASS_NAME: &str = "WinCraftHost";

const MENU_ABOUT: u32 = 1;
const MENU_EDIT_CONFIG: u32 = 2;
const MENU_OPEN_LOG: u32 = 3;
const MENU_AUTOSTART: u32 = 4;
const MENU_EXIT: u32 = 5;
const MENU_MODULE_BASE: u32 = 100;
const MENU_MODULE_ACTION_BASE: u32 = 1000;

struct RegisteredHotkey {
    action_id: u32,
    global_id: i32,
    label: &'static str,
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
    slots: Vec<ModuleSlot>,
    tray: Tray,
    next_hotkey_id: i32,
    taskbar_created: u32,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

pub fn run(config: Config, modules: Vec<Box<dyn WinCraftModule>>) {
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
        tray: Tray::new(hwnd, hinstance),
        next_hotkey_id: 1,
        taskbar_created: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
    };

    host.merge_module_defaults();
    let wanted: Vec<usize> = (0..host.slots.len())
        .filter(|index| {
            let id = host.slots[*index].module.metadata().id;
            host.config
                .modules
                .get(id)
                .map(|module| module.enabled)
                .unwrap_or(true)
        })
        .collect();
    for index in wanted {
        host.enable_slot(index);
    }
    host.save_config();
    host.tray.add();
    log::info!("WinCraft {} is running", env!("CARGO_PKG_VERSION"));

    HOST.with(|cell| *cell.borrow_mut() = Some(host));

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
    fn merge_module_defaults(&mut self) {
        for index in 0..self.slots.len() {
            let slot = &self.slots[index];
            let meta = slot.module.metadata();
            let defaults: Vec<(&str, String)> = slot
                .module
                .hotkey_actions()
                .iter()
                .map(|action| (action.name, hotkeys::format(action.default)))
                .collect();
            let settings = slot.module.default_settings();
            self.config
                .module_mut(meta.id)
                .merge_defaults(&defaults, &settings);
        }
    }

    fn resolve_hotkey(&self, module_id: &str, name: &str, default: Hotkey) -> Hotkey {
        let text = self
            .config
            .modules
            .get(module_id)
            .and_then(|module| module.hotkeys.get(name));
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
            .config
            .modules
            .get(meta.id)
            .map(|module| module.settings.clone())
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
                keys,
                ok,
            });
        }

        self.slots[index].enabled = true;
        self.config.module_mut(meta.id).enabled = true;
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
        self.config.module_mut(id).enabled = false;
        log::info!("{id} disabled");
    }

    fn menu_items(&self) -> Vec<MenuItem> {
        let mut items = Vec::new();
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
        if !self.slots.is_empty() {
            items.push(MenuItem::Separator);
        }
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

    fn save_config(&self) {
        if let Err(err) = self.config.save() {
            log::error!("could not save config.json: {err}");
        }
    }

    fn shutdown(&mut self) {
        for index in 0..self.slots.len() {
            self.disable_slot(index);
        }
        self.tray.remove();
        unsafe { PostQuitMessage(0) };
    }
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

fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| cell.borrow_mut().as_mut().map(f))
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
                host.save_config();
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

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = with_host(|host| host.taskbar_created).unwrap_or(0);
    if taskbar_created != 0 && msg == taskbar_created {
        with_host(|host| host.tray.add());
        return 0;
    }

    match msg {
        WM_HOTKEY => {
            let global_id = wparam as i32;
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
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_POWERBROADCAST | WM_TIMER => {
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
