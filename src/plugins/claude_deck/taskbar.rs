//! ClaudeDeck's own taskbar button. Its main window gets an AppUserModelID
//! of its own, so Windows shows it as a button apart from WinCraft's other
//! windows. Mini mode's hover window stays out of the taskbar.

use std::sync::Arc;

use windows_sys::Win32::Foundation::HWND;

use crate::core::traits::WindowFrame;
use crate::core::{appid, ico, instance};

/// The taskbar's name for ClaudeDeck's window; a test copy adds its suffix,
/// so its button never joins the real ClaudeDeck's.
const APP_ID: &str = "WinCraft.ClaudeDeck";

pub fn app_id() -> String {
    instance::name(APP_ID)
}

/// The one place that picks the icon of ClaudeDeck's window and taskbar
/// button: the .ico file the taskbar takes it from, and the picture egui
/// gives the window when it is made. For now both are WinCraft's.
const ICON_FILE: &[u8] = include_bytes!("../../../assets/wincraft.ico");

pub fn window_icon(frame: &WindowFrame) -> Option<Arc<egui::IconData>> {
    frame.icon.clone()
}

/// Gives the window its own taskbar button, with its icon: a button of its
/// own takes the window's big icon, which egui does not set.
pub fn own_button(hwnd: isize) {
    ico::set_window_icons(hwnd, ICON_FILE);
    if !appid::set_window_id(hwnd as HWND, Some(&app_id())) {
        log::warn!("ClaudeDeck: could not give its window a taskbar button of its own");
    }
}

/// Takes the ID away before the window closes, as Windows asks.
pub fn release(hwnd: isize) {
    appid::set_window_id(hwnd as HWND, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_has_its_own_id_and_a_test_copy_adds_its_suffix() {
        assert!(app_id().starts_with("WinCraft.ClaudeDeck"));
        assert_eq!(
            instance::with_suffix(APP_ID, "dpitest"),
            "WinCraft.ClaudeDeck.dpitest"
        );
    }
}
