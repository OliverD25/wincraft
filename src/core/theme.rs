use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};

use crate::core::config::ThemeChoice;
use crate::core::wide;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

const LORA: &str = "Lora";
const CORMORANT: &str = "Cormorant";
const MONO: &str = "JetBrainsMono";

/// Family name for the Cormorant titles, wordmark and button labels.
pub const HEADING_FAMILY: &str = "heading";

pub const TITLE: &str = "title";
pub const SECTION: &str = "section";
pub const CAPTION: &str = "caption";
pub const KEYCAP: &str = "keycap";
pub const HINT: &str = "hint";
pub const QUERY: &str = "query";

fn read_personalize(value_name: &str) -> Option<u32> {
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
        return None;
    }
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let read = unsafe {
        RegQueryValueExW(
            key,
            wide(value_name).as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut u8,
            &mut size,
        )
    };
    unsafe { RegCloseKey(key) };
    (read == ERROR_SUCCESS).then_some(value)
}

pub fn windows_prefers_dark() -> bool {
    read_personalize("AppsUseLightTheme") == Some(0)
}

/// Windows keeps the taskbar's mode separately from the apps' mode.
pub fn taskbar_is_light() -> bool {
    read_personalize("SystemUsesLightTheme") == Some(1)
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The theme the user chose, for code outside the egui thread: the
/// ShortcutDetector's Win32 window takes its colours from it.
pub fn set_current(choice: ThemeChoice) {
    let value = match choice {
        ThemeChoice::System => 0,
        ThemeChoice::Light => 1,
        ThemeChoice::Dark => 2,
    };
    CURRENT.store(value, Ordering::Relaxed);
}

pub fn current() -> ThemeChoice {
    match CURRENT.load(Ordering::Relaxed) {
        1 => ThemeChoice::Light,
        2 => ThemeChoice::Dark,
        _ => ThemeChoice::System,
    }
}

/// GDI wants 0x00BBGGRR.
pub fn colorref(colour: Color32) -> u32 {
    u32::from(colour.r()) | (u32::from(colour.g()) << 8) | (u32::from(colour.b()) << 16)
}

pub fn is_dark(choice: ThemeChoice) -> bool {
    match choice {
        ThemeChoice::System => windows_prefers_dark(),
        ThemeChoice::Light => false,
        ThemeChoice::Dark => true,
    }
}

/// The handout's colour table, one instance per mode. Stored in the egui
/// context so every painted widget reads the same values the theme applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    pub window_bg: Color32,
    pub panel_bg: Color32,
    pub elevated_bg: Color32,
    pub border: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_disabled: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_pressed: Color32,
    pub accent_tint: Color32,
    pub selection_bg: Color32,
    pub hover_bg: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub info: Color32,
    pub focus_ring: Color32,
    pub palette_shadow: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub fn with_alpha(colour: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        colour.r(),
        colour.g(),
        colour.b(),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

impl Tokens {
    pub fn dark() -> Self {
        let accent = hex(0xe1ad66);
        Self {
            dark: true,
            window_bg: hex(0x1c1a19),
            panel_bg: hex(0x262322),
            elevated_bg: hex(0x2d2b2b),
            border: hex(0x444141),
            text_primary: hex(0xf3f2f2),
            text_secondary: hex(0xbab6b6),
            text_disabled: hex(0x7d7979),
            accent,
            accent_hover: hex(0xfacb8d),
            accent_pressed: hex(0xc28d41),
            accent_tint: with_alpha(accent, 0.10),
            selection_bg: with_alpha(accent, 0.18),
            hover_bg: with_alpha(hex(0xf3f2f2), 0.05),
            success: hex(0x8fbf7f),
            warning: hex(0xe88a4a),
            danger: hex(0xe07a6d),
            info: hex(0x8fb0d0),
            focus_ring: accent,
            palette_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 115),
        }
    }

    pub fn light() -> Self {
        let accent = hex(0xb68235);
        Self {
            dark: false,
            window_bg: hex(0xf3f2f2),
            panel_bg: hex(0xeae9e9),
            elevated_bg: hex(0xfaf8f7),
            border: hex(0xd7d3d3),
            text_primary: hex(0x201f1d),
            text_secondary: hex(0x605d5d),
            text_disabled: hex(0x9b9797),
            accent,
            accent_hover: hex(0xa06f24),
            accent_pressed: hex(0x7d5411),
            accent_tint: with_alpha(accent, 0.10),
            selection_bg: with_alpha(accent, 0.16),
            hover_bg: with_alpha(hex(0x201f1d), 0.05),
            success: hex(0x3f7a4a),
            warning: hex(0xb45f14),
            danger: hex(0x9f3a2f),
            info: hex(0x4a6a8a),
            focus_ring: accent,
            palette_shadow: Color32::from_rgba_unmultiplied(45, 43, 43, 56),
        }
    }

    pub fn for_choice(choice: ThemeChoice) -> Self {
        if is_dark(choice) {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// Falls back to the dark set before `apply` has run, which only happens
    /// in the first frame on startup.
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp::<Tokens>(tokens_id()))
            .unwrap_or_else(Self::dark)
    }
}

fn tokens_id() -> egui::Id {
    egui::Id::new("wincraft-tokens")
}

/// A 1 px line at 150 % would be 1.5 physical pixels and render blurred, so
/// every stroke is rounded up to a whole number of physical pixels.
pub fn snap(logical: f32, pixels_per_point: f32) -> f32 {
    let ppp = pixels_per_point.max(0.1);
    (logical * ppp).ceil() / ppp
}

pub fn stroke(ctx: &egui::Context, width: f32, colour: Color32) -> Stroke {
    Stroke::new(snap(width, ctx.pixels_per_point()), colour)
}

pub fn heading_family() -> FontFamily {
    FontFamily::Name(HEADING_FAMILY.into())
}

pub fn lora(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn cormorant(size: f32) -> FontId {
    FontId::new(size, heading_family())
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Three files, one weight each. egui's own fonts stay behind them as a
/// fallback for arrows and symbols the three typefaces do not contain.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            LORA,
            include_bytes!("../../assets/fonts/Lora-Regular.ttf").as_slice(),
        ),
        (
            CORMORANT,
            include_bytes!("../../assets/fonts/CormorantGaramond-SemiBold.ttf").as_slice(),
        ),
        (
            MONO,
            include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    }

    let fallback_proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let fallback_mono = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    let chain = |first: &[&str], rest: &[String]| -> Vec<String> {
        first
            .iter()
            .map(|name| name.to_string())
            .chain(rest.iter().cloned())
            .collect()
    };
    fonts.families.insert(
        FontFamily::Proportional,
        chain(&[LORA], &fallback_proportional),
    );
    fonts
        .families
        .insert(FontFamily::Monospace, chain(&[MONO], &fallback_mono));
    fonts.families.insert(
        heading_family(),
        chain(&[CORMORANT, LORA], &fallback_proportional),
    );
    ctx.set_fonts(fonts);
}

/// One place decides how both windows look, so the palette and the settings
/// window can never drift apart.
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    let tokens = Tokens::for_choice(choice);
    let ppp = ctx.pixels_per_point();
    let line = |colour: Color32| Stroke::new(snap(1.0, ppp), colour);
    let r4 = CornerRadius::same(4);

    let mut visuals = if tokens.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = None;
    visuals.weak_text_color = Some(tokens.text_secondary);
    visuals.disabled_alpha = 0.45;
    visuals.hyperlink_color = tokens.accent;
    visuals.panel_fill = tokens.window_bg;
    visuals.window_fill = tokens.elevated_bg;
    visuals.faint_bg_color = tokens.panel_bg;
    visuals.extreme_bg_color = tokens.window_bg;
    visuals.text_edit_bg_color = Some(tokens.window_bg);
    visuals.code_bg_color = tokens.panel_bg;
    visuals.warn_fg_color = tokens.warning;
    visuals.error_fg_color = tokens.danger;
    visuals.window_stroke = line(tokens.border);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = r4;
    visuals.window_shadow = Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: tokens.palette_shadow,
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 16,
        spread: 0,
        color: tokens.palette_shadow,
    };
    visuals.selection.bg_fill = tokens.selection_bg;
    visuals.selection.stroke = line(tokens.accent);
    visuals.text_cursor.stroke = line(tokens.accent);
    visuals.slider_trailing_fill = true;
    visuals.striped = false;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.noninteractive.bg_stroke = line(tokens.border);
    widgets.noninteractive.fg_stroke = line(tokens.text_primary);
    widgets.noninteractive.corner_radius = r4;
    widgets.noninteractive.expansion = 0.0;

    widgets.inactive.bg_fill = Color32::TRANSPARENT;
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = line(tokens.border);
    widgets.inactive.fg_stroke = line(tokens.text_primary);
    widgets.inactive.corner_radius = r4;
    widgets.inactive.expansion = 0.0;

    widgets.hovered.bg_fill = tokens.hover_bg;
    widgets.hovered.weak_bg_fill = tokens.hover_bg;
    widgets.hovered.bg_stroke = line(tokens.border);
    widgets.hovered.fg_stroke = line(tokens.text_primary);
    widgets.hovered.corner_radius = r4;
    widgets.hovered.expansion = 0.0;

    widgets.active.bg_fill = tokens.selection_bg;
    widgets.active.weak_bg_fill = tokens.selection_bg;
    widgets.active.bg_stroke = line(tokens.accent_pressed);
    widgets.active.fg_stroke = line(tokens.text_primary);
    widgets.active.corner_radius = r4;
    widgets.active.expansion = 0.0;

    widgets.open = widgets.hovered;

    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        use egui::TextStyle;
        style.text_styles = [
            (TextStyle::Name(TITLE.into()), cormorant(30.0)),
            (TextStyle::Heading, cormorant(26.0)),
            (TextStyle::Name(SECTION.into()), lora(11.0)),
            (TextStyle::Body, lora(14.0)),
            (TextStyle::Small, lora(12.0)),
            (TextStyle::Name(CAPTION.into()), lora(11.0)),
            (TextStyle::Button, cormorant(15.0)),
            (TextStyle::Name(KEYCAP.into()), mono(12.0)),
            (TextStyle::Name(HINT.into()), mono(11.0)),
            (TextStyle::Monospace, mono(13.0)),
            (TextStyle::Name(QUERY.into()), lora(15.0)),
        ]
        .into_iter()
        .collect();

        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(8.0, 8.0);
        spacing.button_padding = egui::vec2(12.0, 0.0);
        spacing.interact_size = egui::vec2(32.0, 32.0);
        spacing.indent = 16.0;
        spacing.window_margin = egui::Margin::same(16);
        spacing.menu_margin = egui::Margin::same(8);
        spacing.icon_width = 16.0;
        spacing.icon_spacing = 8.0;
        spacing.combo_height = 240.0;
    });
    ctx.data_mut(|data| data.insert_temp(tokens_id(), tokens));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_snap_up_to_whole_physical_pixels() {
        assert_eq!(snap(1.0, 1.0), 1.0);
        assert!((snap(1.0, 1.5) - 2.0 / 1.5).abs() < 1e-6);
        assert!((snap(2.0, 1.5) - 3.0 / 1.5).abs() < 1e-6);
        assert!((snap(1.0, 1.25) - 2.0 / 1.25).abs() < 1e-6);
        assert_eq!(snap(1.0, 2.0), 1.0);
    }

    /// Running passes proves the three files parse; the chains prove each one
    /// is what its family draws first. Glyph coverage is not asserted here:
    /// egui's has_glyph reports every character as missing once the first face
    /// can also draw the replacement glyph, which Cormorant can.
    #[test]
    fn the_bundled_fonts_load_first_in_their_families() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        apply(&ctx, ThemeChoice::Dark);
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label(egui::RichText::new("WinCraft").font(cormorant(26.0)));
                ui.label(egui::RichText::new("Toggle monitor 1").font(lora(14.0)));
                ui.label(egui::RichText::new("Win+Alt+F1 \u{21B5}").font(mono(12.0)));
            });
            // There is no renderer in a test to upload the font atlas to.
            output.textures_delta.clear();
        }
        let chains = ctx.fonts(|fonts| fonts.definitions().families.clone());
        let first = |family: FontFamily| chains.get(&family).and_then(|c| c.first()).cloned();
        assert_eq!(first(FontFamily::Proportional).as_deref(), Some(LORA));
        assert_eq!(first(FontFamily::Monospace).as_deref(), Some(MONO));
        assert_eq!(first(heading_family()).as_deref(), Some(CORMORANT));
    }

    #[test]
    fn gdi_colours_are_blue_green_red() {
        assert_eq!(colorref(hex(0x1c1a19)), 0x00191a1c);
    }

    #[test]
    fn the_current_theme_round_trips() {
        for choice in ThemeChoice::ALL {
            set_current(choice);
            assert_eq!(current(), choice);
        }
        set_current(ThemeChoice::System);
    }

    #[test]
    fn the_token_tables_match_the_handout() {
        let dark = Tokens::dark();
        assert_eq!(dark.window_bg, hex(0x1c1a19));
        assert_eq!(dark.accent, hex(0xe1ad66));
        assert_eq!(dark.focus_ring, dark.accent);
        assert_eq!(dark.selection_bg.a(), with_alpha(dark.accent, 0.18).a());

        let light = Tokens::light();
        assert_eq!(light.window_bg, hex(0xf3f2f2));
        assert_eq!(light.accent, hex(0xb68235));
        assert_eq!(light.text_primary, hex(0x201f1d));
        assert_ne!(dark, light);
    }
}
