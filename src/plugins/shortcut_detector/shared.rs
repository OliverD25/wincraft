//! What the plugin and its window on the UI thread share. The scan runs on
//! the host thread, because it reads WinCraft's own hotkeys from the host;
//! the window asks for one with a message to the plugin's window.

use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::core::traits::Hotkey;

use super::probe::ScanResult;

/// Posted to the plugin's window when a scan is wanted.
pub const WM_APP_SCAN: u32 = WM_APP + 11;

#[derive(Default)]
pub struct Shared {
    /// The plugin's own window while the plugin is on; 0 when off.
    pub plugin_window: isize,
    /// The window is on screen, or was asked for.
    pub open: bool,
    /// A scan was asked for and has not finished yet.
    pub scanning: bool,
    pub scan: Option<Arc<ScanResult>>,
    /// WinCraft's own hotkeys at the last scan, for the Check a shortcut box.
    pub mine: Vec<(String, String, Hotkey)>,
    /// The Show free switch.
    pub show_free: bool,
}

pub type Handle = Arc<Mutex<Shared>>;

impl Shared {
    /// Asks the plugin for a scan; nothing happens while it is off.
    pub fn ask_scan(&mut self) {
        if self.plugin_window == 0 {
            return;
        }
        self.scanning = true;
        unsafe { PostMessageW(self.plugin_window as HWND, WM_APP_SCAN, 0, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_is_asked_for_only_while_the_plugin_is_on() {
        let mut shared = Shared::default();
        shared.ask_scan();
        assert!(!shared.scanning, "off: nothing to ask");
        // A window handle no window has: the post fails quietly.
        shared.plugin_window = 1;
        shared.ask_scan();
        assert!(shared.scanning);
    }
}
