pub mod known;
pub mod probe;
mod window;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, IsWindow};

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PluginMetadata, SettingField, TrayAction,
    WinCraftPlugin,
};

const ACTION_OPEN: u32 = 1;

#[derive(Default)]
pub struct ShortcutDetector {
    window: Option<HWND>,
    show_free_by_default: bool,
}

impl ShortcutDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the window and asks it to scan later. It must stay this cheap:
    /// the host calls it with its own state mutably borrowed, so anything that
    /// reads the host back - the scan does - has to wait for its own message.
    fn open(&mut self) {
        let existing = self.live_window();
        match window::open(existing) {
            Some(hwnd) => {
                if existing.is_none() {
                    window::set_show_free_default(hwnd, self.show_free_by_default);
                }
                self.window = Some(hwnd);
            }
            None => self.window = None,
        }
    }

    fn live_window(&self) -> Option<HWND> {
        self.window.filter(|hwnd| unsafe { IsWindow(*hwnd) } != 0)
    }
}

impl WinCraftPlugin for ShortcutDetector {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "shortcut_detector",
            name: "ShortcutDetector",
            description: "Find taken and free global key combinations, and test a shortcut before you assign it.",
            author: "community",
            version: "1.1.0",
            readme: include_str!("README.md"),
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({ "show_free_by_default": false })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![SettingField {
            key: "show_free_by_default",
            label: "Show free combinations",
            help: "Start with every untaken combination already listed.",
            kind: FieldKind::Toggle,
        }]
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.show_free_by_default = settings
            .get("show_free_by_default")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if let Some(hwnd) = self.live_window() {
            window::set_show_free_default(hwnd, self.show_free_by_default);
        }
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.show_free_by_default = ctx
            .settings
            .get("show_free_by_default")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        self.window = None;
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![HotkeyAction {
            id: ACTION_OPEN,
            name: "open_window",
            label: "Open shortcut detector",
            default: Hotkey {
                modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
                vk: u32::from(b'Q'),
            },
        }]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        if action_id == ACTION_OPEN {
            self.open();
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: ACTION_OPEN,
            label: "Shortcut detector",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == ACTION_OPEN {
            self.open();
        }
    }

    fn teardown(&mut self) {
        if let Some(hwnd) = self.live_window() {
            unsafe { DestroyWindow(hwnd) };
        }
        self.window = None;
    }
}
