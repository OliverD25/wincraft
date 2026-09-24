use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

use crate::core::{config, wide};

pub struct AboutHotkey {
    pub keys: String,
    pub label: String,
    pub registered: bool,
}

pub struct AboutPlugin {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub enabled: bool,
    pub hotkeys: Vec<AboutHotkey>,
}

pub fn text(plugins: &[AboutPlugin]) -> String {
    let mut out = format!("WinCraft {}\n", env!("CARGO_PKG_VERSION"));

    if plugins.is_empty() {
        out.push_str("\nNo plugins are built into this copy.\n");
    }
    for plugin in plugins {
        let state = if plugin.enabled {
            "enabled"
        } else {
            "disabled"
        };
        out.push_str(&format!(
            "\n{} {} \u{2014} {}   [{}]\n  {}\n",
            plugin.name, plugin.version, plugin.author, state, plugin.description
        ));
        let width = plugin
            .hotkeys
            .iter()
            .map(|h| h.keys.chars().count())
            .max()
            .unwrap_or(0);
        for hotkey in &plugin.hotkeys {
            let note = if hotkey.registered {
                ""
            } else {
                "   (NOT REGISTERED \u{2014} key in use)"
            };
            out.push_str(&format!(
                "  {:<width$}  {}{}\n",
                hotkey.keys,
                hotkey.label,
                note,
                width = width
            ));
        }
    }

    out.push_str(&format!("\nConfig: {}\n", config::config_path().display()));
    out
}

/// Runs a nested message loop, so the caller must not be holding a borrow of
/// the host state while this is on the stack.
pub fn show(hwnd: HWND, body: &str) {
    unsafe {
        MessageBoxW(
            hwnd,
            wide(body).as_ptr(),
            wide("About WinCraft").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        )
    };
}
