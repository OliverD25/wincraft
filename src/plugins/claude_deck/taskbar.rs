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
    let icon = icon_on_disk();
    let command = relaunch_command();
    let relaunch = match (&icon, &command) {
        (Some(icon), Some(command)) => Some(appid::Relaunch {
            command,
            name: PIN_NAME,
            icon,
        }),
        _ => None,
    };
    let id = app_id();
    if !appid::set_window_identity(hwnd as HWND, Some((&id, relaunch.as_ref()))) {
        log::warn!("ClaudeDeck: could not give its window a taskbar button of its own");
    }
}

/// The name of ClaudeDeck's button when it is pinned to the taskbar.
const PIN_NAME: &str = "ClaudeDeck";

/// A pinned button starts WinCraft and opens ClaudeDeck.
fn relaunch_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(pin_command(&exe.to_string_lossy()))
}

fn pin_command(exe: &str) -> String {
    format!("\"{exe}\" --open-claude-deck")
}

/// A pinned button takes its picture from an icon file, not from the
/// window, so ClaudeDeck keeps a copy of its icon in WinCraft's data folder.
fn icon_on_disk() -> Option<String> {
    let path = crate::core::config::data_dir().join("claudedeck.ico");
    let same = std::fs::read(&path).is_ok_and(|bytes| bytes == ICON_FILE);
    if !same {
        if let Err(err) = std::fs::write(&path, ICON_FILE) {
            log::warn!("ClaudeDeck: could not save its icon for a pinned button: {err}");
            return None;
        }
    }
    Some(format!("{},0", path.display()))
}

/// Takes the ID away before the window closes, as Windows asks.
pub fn release(hwnd: isize) {
    appid::set_window_identity(hwnd as HWND, None);
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

    #[test]
    fn a_pinned_button_opens_claudedeck_with_its_own_name() {
        assert_eq!(
            pin_command(r"C:\Apps\WinCraft\wincraft.exe"),
            r#""C:\Apps\WinCraft\wincraft.exe" --open-claude-deck"#
        );
        assert_eq!(PIN_NAME, "ClaudeDeck");
    }
}
