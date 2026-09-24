use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::core::config::ThemeChoice;
use crate::core::traits::FieldKind;

/// Posted to the host window when the UI thread has queued a request. The host
/// drains the channel inside its normal borrow, exactly like a tray click.
pub const WM_APP_UI: u32 = WM_APP + 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    General,
    Plugins,
    Store,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Hotkey,
    Tray,
    Palette,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostCommand {
    OpenSettings,
    OpenStore,
    OpenAbout,
    OpenLog,
    OpenConfig,
    TogglePlugin(usize),
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    Host(HostCommand),
    Plugin {
        index: usize,
        kind: ActionKind,
        action: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostSetting {
    StartWithWindows(bool),
    Theme(ThemeChoice),
    PaletteHotkey(String),
}

/// Screen rectangle in physical pixels, as Win32 reports it.
#[derive(Clone, Copy, Debug, Default)]
pub struct MonitorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub enum UiCommand {
    ShowPalette(MonitorRect),
    ShowSettings(Page),
    HideAll,
    Snapshot(Box<UiSnapshot>),
    ThemeChanged,
    Quit,
}

pub enum HostRequest {
    UiReady {
        palette_hwnd: isize,
    },
    RunCommand(CommandId),
    SetPluginEnabled {
        id: String,
        enabled: bool,
    },
    SetHotkey {
        plugin: String,
        action: String,
        binding: String,
    },
    SetSetting {
        plugin: String,
        key: String,
        value: Value,
    },
    ResetPlugin(String),
    SetHostSetting(HostSetting),
    OpenPath(PathBuf),
    Exit,
}

#[derive(Clone, Debug)]
pub struct HotkeyInfo {
    pub action: String,
    pub label: String,
    pub binding: String,
    pub default_binding: String,
    pub registered: bool,
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub key: String,
    pub label: String,
    pub help: String,
    pub kind: FieldKind,
    pub value: Value,
}

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub readme: String,
    pub enabled: bool,
    pub config_path: PathBuf,
    pub hotkeys: Vec<HotkeyInfo>,
    pub fields: Vec<FieldInfo>,
}

#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub id: CommandId,
    pub group: String,
    pub label: String,
    pub hint: String,
}

/// Everything the windows draw. Rebuilt whole after every change: a plugin's
/// worth of it is well under a kilobyte, so diffing would cost more code than
/// it saves.
#[derive(Clone, Debug, Default)]
pub struct UiSnapshot {
    pub version: String,
    pub start_with_windows: bool,
    pub theme: ThemeChoice,
    pub palette_hotkey: String,
    pub palette_hotkey_registered: bool,
    pub config_path: PathBuf,
    pub log_path: PathBuf,
    pub plugins: Vec<PluginInfo>,
    pub commands: Vec<PaletteEntry>,
}

/// Host → UI. The egui context arrives once the UI thread is up; until then
/// commands still queue, they are simply drawn on the next heartbeat.
pub struct UiChannel {
    tx: Sender<UiCommand>,
    ctx: Mutex<Option<egui::Context>>,
}

impl UiChannel {
    pub fn send(&self, command: UiCommand) {
        if self.tx.send(command).is_err() {
            return;
        }
        // A hidden eframe window only wakes every 100 ms, so without this the
        // palette would appear a tenth of a second after the hotkey.
        if let Ok(guard) = self.ctx.lock() {
            if let Some(ctx) = guard.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    pub fn attach(&self, ctx: egui::Context) {
        if let Ok(mut guard) = self.ctx.lock() {
            *guard = Some(ctx);
        }
    }
}

/// UI → host.
pub struct HostChannel {
    tx: Sender<HostRequest>,
    hwnd: AtomicIsize,
}

impl HostChannel {
    pub fn send(&self, request: HostRequest) {
        if self.tx.send(request).is_err() {
            return;
        }
        let hwnd = self.hwnd.load(Ordering::Relaxed);
        if hwnd != 0 {
            unsafe { PostMessageW(hwnd as HWND, WM_APP_UI, 0, 0) };
        }
    }

    pub fn set_host_window(&self, hwnd: HWND) {
        self.hwnd.store(hwnd as isize, Ordering::Relaxed);
    }
}

pub struct Bridge {
    pub to_ui: Arc<UiChannel>,
    pub to_host: Arc<HostChannel>,
    pub ui_rx: Receiver<UiCommand>,
    pub host_rx: Receiver<HostRequest>,
}

pub fn create() -> Bridge {
    let (ui_tx, ui_rx) = channel();
    let (host_tx, host_rx) = channel();
    Bridge {
        to_ui: Arc::new(UiChannel {
            tx: ui_tx,
            ctx: Mutex::new(None),
        }),
        to_host: Arc::new(HostChannel {
            tx: host_tx,
            hwnd: AtomicIsize::new(0),
        }),
        ui_rx,
        host_rx,
    }
}
