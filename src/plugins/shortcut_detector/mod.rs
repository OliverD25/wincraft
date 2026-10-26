pub mod known;
pub mod probe;
mod window;

use std::collections::BTreeMap;

use serde_json::Value;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, IsWindow};

use crate::core::shortcuts;
use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PluginMetadata, SettingField, TrayAction,
    WinCraftPlugin,
};

const ACTION_OPEN: u32 = 1;
const OPEN_ACTION: &str = "open_window";
/// The default before 0.20.16, which QuickJump took over.
const OLD_OPEN_KEYS: &str = "Win+Alt+Q";
const OPEN_KEYS: &str = "Win+Alt+S";
/// Written once the old default was looked at, so a user who later picks
/// Win+Alt+Q again keeps it.
const MOVED_NOTE: &str = "open_window_moved";

/// A stored key equal to the old default was never a choice, so it takes
/// the new default, once. Returns whether it moved.
fn move_old_default(hotkeys: &mut BTreeMap<String, String>, settings: &mut Value) -> bool {
    if settings.get(MOVED_NOTE).and_then(Value::as_bool) == Some(true) {
        return false;
    }
    if let Some(object) = settings.as_object_mut() {
        object.insert(MOVED_NOTE.to_string(), Value::Bool(true));
    }
    let old = hotkeys
        .get(OPEN_ACTION)
        .is_some_and(|keys| shortcuts::same_text(keys, OLD_OPEN_KEYS));
    if old {
        hotkeys.insert(OPEN_ACTION.to_string(), OPEN_KEYS.to_string());
    }
    old
}

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
            category: "Keyboard",
            thumbnail: None,
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
            name: OPEN_ACTION,
            label: "Open shortcut detector",
            default: Hotkey {
                modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
                vk: u32::from(b'S'),
            },
        }]
    }

    fn migrate_hotkeys(&self, hotkeys: &mut BTreeMap<String, String>, settings: &mut Value) {
        if move_old_default(hotkeys, settings) {
            log::info!(
                "shortcut_detector open_window was the old default {OLD_OPEN_KEYS}; now {OPEN_KEYS}, so QuickJump can use {OLD_OPEN_KEYS}"
            );
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(keys: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(OPEN_ACTION.to_string(), keys.to_string())])
    }

    #[test]
    fn the_old_default_moves_once_and_a_chosen_key_stays() {
        let mut hotkeys = stored("Win+Alt+Q");
        let mut settings = serde_json::json!({});
        assert!(move_old_default(&mut hotkeys, &mut settings));
        assert_eq!(hotkeys[OPEN_ACTION], "Win+Alt+S");
        // Picked again later, it is a choice and stays.
        let mut again = stored("win+alt+q");
        assert!(!move_old_default(&mut again, &mut settings));
        assert_eq!(again[OPEN_ACTION], "win+alt+q");

        let mut custom = stored("Win+Alt+F12");
        assert!(!move_old_default(&mut custom, &mut serde_json::json!({})));
        assert_eq!(custom[OPEN_ACTION], "Win+Alt+F12");
        let mut none = stored("");
        assert!(!move_old_default(&mut none, &mut serde_json::json!({})));
        assert_eq!(none[OPEN_ACTION], "");
    }

    #[test]
    fn the_default_is_win_alt_s() {
        let action = &ShortcutDetector::new().hotkey_actions()[0];
        assert_eq!(crate::core::hotkeys::format(action.default), OPEN_KEYS);
    }
}
