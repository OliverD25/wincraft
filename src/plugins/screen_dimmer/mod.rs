mod overlay;

use windows_sys::Win32::Foundation::{LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PluginMetadata, SettingField, TrayAction,
    WinCraftPlugin,
};
use crate::core::ui_bridge::ActionKind;
use crate::core::{host, monitors};
use overlay::Overlay;

const ACTION_TOGGLE_BASE: u32 = 1;
const MONITOR_ACTIONS: u32 = 3;
const ACTION_WAKE_ALL: u32 = 100;
const TRAY_WAKE_ALL: u32 = 1;

#[derive(Default)]
pub struct ScreenDimmer {
    overlays: Vec<Overlay>,
    /// Where each overlay's monitor sits, for the palette: "left" and so on.
    positions: Vec<Option<String>>,
    idle_alpha: u8,
    hover_alpha: u8,
}

impl ScreenDimmer {
    pub fn new() -> Self {
        Self::default()
    }

    fn build_overlays(&mut self, dimmed: &[bool]) {
        let monitors = overlay::monitors();
        let lefts: Vec<i32> = monitors.iter().map(|(_, rect)| rect.left).collect();
        self.positions = monitors::position_names(&lefts)
            .into_iter()
            .map(|name| name.map(|name| name.to_lowercase()))
            .collect();
        for (index, (monitor, rect)) in monitors.into_iter().enumerate() {
            let Some(mut item) = Overlay::create(monitor, rect, self.idle_alpha, self.hover_alpha)
            else {
                continue;
            };
            if dimmed.get(index).copied().unwrap_or(false) {
                item.show();
            }
            self.overlays.push(item);
        }
        log::info!("screen_dimmer covers {} monitor(s)", self.overlays.len());
    }

    fn destroy_overlays(&mut self) {
        for item in &mut self.overlays {
            item.destroy();
        }
        self.overlays.clear();
    }

    fn toggle(&mut self, index: usize) {
        match self.overlays.get_mut(index) {
            Some(item) => {
                if item.visible {
                    item.hide();
                    log::info!("monitor {} back to normal", index + 1);
                } else {
                    item.show();
                    log::info!("monitor {} dimmed", index + 1);
                }
            }
            None => log::info!("no monitor {} to toggle", index + 1),
        }
        host::plugin_changed();
    }

    fn wake_all(&mut self) {
        for item in &mut self.overlays {
            item.hide();
        }
        log::info!("all monitors back to normal");
        host::plugin_changed();
    }
}

fn alpha_from(settings: &serde_json::Value, key: &str, fallback: f64) -> u8 {
    let value = settings
        .get(key)
        .and_then(|v| v.as_f64())
        .unwrap_or(fallback);
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl WinCraftPlugin for ScreenDimmer {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "screen_dimmer",
            name: "ScreenDimmer",
            description: "Dim or blank monitors with click-through overlays.",
            author: "community",
            version: "1.1.0",
            readme: include_str!("README.md"),
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({ "idle_opacity": 1.0, "hover_opacity": 0.7 })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: "idle_opacity",
                label: "Darkness",
                help: "How solid the overlay is normally. 1.0 is fully black.",
                kind: FieldKind::Slider {
                    min: 0.0,
                    max: 1.0,
                    step: 0.05,
                },
            },
            SettingField {
                key: "hover_opacity",
                label: "Darkness under the pointer",
                help: "How solid it is while your pointer is on that monitor.",
                kind: FieldKind::Slider {
                    min: 0.0,
                    max: 1.0,
                    step: 0.05,
                },
            },
        ]
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.idle_alpha = alpha_from(settings, "idle_opacity", 1.0);
        self.hover_alpha = alpha_from(settings, "hover_opacity", 0.7);
        for item in &mut self.overlays {
            item.set_alphas(self.idle_alpha, self.hover_alpha);
        }
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        // WM_DISPLAYCHANGE reaches this plugin only through the host window, so
        // its handle is worth having in the log when message routing misbehaves.
        log::debug!("screen_dimmer attached to host window {:p}", ctx.hwnd);
        self.idle_alpha = alpha_from(ctx.settings, "idle_opacity", 1.0);
        self.hover_alpha = alpha_from(ctx.settings, "hover_opacity", 0.7);
        self.build_overlays(&[]);
        if self.overlays.is_empty() {
            return Err("no monitors could be covered".to_string());
        }
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        // Adding a fourth monitor later is one more entry in this list.
        vec![
            HotkeyAction {
                id: ACTION_TOGGLE_BASE,
                name: "toggle_monitor_1",
                label: "Toggle monitor 1",
                default: win_alt(0x70),
            },
            HotkeyAction {
                id: ACTION_TOGGLE_BASE + 1,
                name: "toggle_monitor_2",
                label: "Toggle monitor 2",
                default: win_alt(0x71),
            },
            HotkeyAction {
                id: ACTION_TOGGLE_BASE + 2,
                name: "toggle_monitor_3",
                label: "Toggle monitor 3",
                default: win_alt(0x72),
            },
            HotkeyAction {
                id: ACTION_WAKE_ALL,
                name: "wake_all",
                label: "Wake all monitors",
                default: win_alt(0x7B),
            },
        ]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        if action_id == ACTION_WAKE_ALL {
            self.wake_all();
        } else if (ACTION_TOGGLE_BASE..ACTION_TOGGLE_BASE + MONITOR_ACTIONS).contains(&action_id) {
            self.toggle((action_id - ACTION_TOGGLE_BASE) as usize);
        }
    }

    fn on_windows_message(&mut self, msg: u32, _wparam: WPARAM, _lparam: LPARAM) {
        if msg != WM_DISPLAYCHANGE {
            return;
        }
        // Monitor handles and rectangles are stale after a layout change, so the
        // overlays are rebuilt and the ones that were dimmed come back dimmed.
        let dimmed: Vec<bool> = self.overlays.iter().map(|item| item.visible).collect();
        self.destroy_overlays();
        self.build_overlays(&dimmed);
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: TRAY_WAKE_ALL,
            label: "Wake all monitors",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == TRAY_WAKE_ALL {
            self.wake_all();
        }
    }

    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        if kind != ActionKind::Hotkey || action_id < ACTION_TOGGLE_BASE {
            return None;
        }
        let index = (action_id - ACTION_TOGGLE_BASE) as usize;
        let overlay = self.overlays.get(index)?;
        let state = if overlay.visible { "dimmed" } else { "on" };
        Some(match self.positions.get(index).cloned().flatten() {
            Some(position) => format!("{state} \u{b7} {position}"),
            None => state.to_string(),
        })
    }

    fn teardown(&mut self) {
        self.destroy_overlays();
    }
}

fn win_alt(vk: u32) -> Hotkey {
    Hotkey {
        modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
        vk,
    }
}
