//! ClaudeDeck's own taskbar button. Its main window gets an AppUserModelID
//! of its own, so Windows shows it as a button apart from WinCraft's other
//! windows. Mini mode's hover window stays out of the taskbar.

use std::sync::{Arc, OnceLock};

use windows_sys::Win32::Foundation::HWND;

use crate::core::{appid, ico, instance};

/// The taskbar's name for ClaudeDeck's window; a test copy adds its suffix,
/// so its button never joins the real ClaudeDeck's.
const APP_ID: &str = "WinCraft.ClaudeDeck";

pub fn app_id() -> String {
    instance::name(APP_ID)
}

/// The one place that picks the icon of ClaudeDeck's window and taskbar
/// button: the .ico file the taskbar takes it from, and the picture egui
/// gives the window when it is made. Both are made from the design's
/// "Amber list" icon by `tools/claudedeck-icon.py`.
const ICON_FILE: &[u8] = include_bytes!("icon/claudedeck.ico");
const ICON_RGBA: &[u8] = include_bytes!("icon/claudedeck-64.rgba");
const ICON_SIDE: u32 = 64;

pub fn window_icon() -> Arc<egui::IconData> {
    static ICON: OnceLock<Arc<egui::IconData>> = OnceLock::new();
    Arc::clone(ICON.get_or_init(|| {
        Arc::new(egui::IconData {
            rgba: ICON_RGBA.to_vec(),
            width: ICON_SIDE,
            height: ICON_SIDE,
        })
    }))
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
    fn the_icon_files_hold_every_size_the_taskbar_and_the_title_use() {
        assert_eq!(ICON_RGBA.len(), (ICON_SIDE * ICON_SIDE * 4) as usize);
        assert_eq!(ico::widths(ICON_FILE), [16, 20, 24, 32, 48, 256]);
        for size in [16, 24, 32, 48, 256] {
            let id = ico::best(&ico::widths(ICON_FILE), size).expect("a picture");
            assert_eq!(
                ico::widths(ICON_FILE)[id - 1],
                size,
                "{size} px is in the file"
            );
            assert!(ico::entry(ICON_FILE, id).is_some());
        }
    }

    #[test]
    fn the_button_has_its_own_id_and_a_test_copy_adds_its_suffix() {
        assert!(app_id().starts_with("WinCraft.ClaudeDeck"));
        assert_eq!(
            instance::with_suffix(APP_ID, "dpitest"),
            "WinCraft.ClaudeDeck.dpitest"
        );
    }
}
