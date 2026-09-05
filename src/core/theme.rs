use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};

use crate::core::config::ThemeChoice;
use crate::core::wide;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

pub fn windows_prefers_dark() -> bool {
    let mut key: HKEY = std::ptr::null_mut();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(PERSONALIZE).as_ptr(),
            0,
            KEY_READ,
            &mut key,
        )
    };
    if opened != ERROR_SUCCESS {
        return false;
    }
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let read = unsafe {
        RegQueryValueExW(
            key,
            wide("AppsUseLightTheme").as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut u8,
            &mut size,
        )
    };
    unsafe { RegCloseKey(key) };
    read == ERROR_SUCCESS && value == 0
}

pub fn is_dark(choice: ThemeChoice) -> bool {
    match choice {
        ThemeChoice::System => windows_prefers_dark(),
        ThemeChoice::Light => false,
        ThemeChoice::Dark => true,
    }
}

/// One place decides how both windows look, so the palette and the settings
/// window can never drift apart.
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    let dark = is_dark(choice);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    let accent = if dark {
        egui::Color32::from_rgb(0x4C, 0x8B, 0xF5)
    } else {
        egui::Color32::from_rgb(0x16, 0x2D, 0x5C)
    };
    visuals.selection.bg_fill = accent.gamma_multiply(0.45);
    visuals.selection.stroke.color = if dark {
        egui::Color32::WHITE
    } else {
        egui::Color32::BLACK
    };
    visuals.hyperlink_color = accent;
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
    });
}
