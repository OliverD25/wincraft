use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};

use egui::epaint::text::{FontTweak, VariationCoords};
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST};

use crate::core::config::{ThemeChoice, ThemeName};
use crate::core::wide;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

const SEGOE: &str = "SegoeUI";
const SEGOE_SEMIBOLD: &str = "SegoeUISemibold";
const SELAWIK: &str = "Selawik";
const SELAWIK_SEMIBOLD: &str = "SelawikSemibold";
const MONO: &str = "JetBrainsMono";
const TERMINAL: &str = "Terminal";
const TERMINAL_SEMIBOLD: &str = "TerminalSemibold";
const LORA: &str = "Lora";
const LORA_SEMIBOLD: &str = "LoraSemibold";
const CORMORANT_SEMIBOLD: &str = "CormorantSemibold";

/// The bundled Segoe UI substitute, shared with the LanguageIndicator's GDI
/// panel so the files are embedded once.
pub const SELAWIK_REGULAR_TTF: &[u8] = include_bytes!("../../assets/fonts/Selawik-Regular.ttf");
pub const SELAWIK_SEMIBOLD_TTF: &[u8] = include_bytes!("../../assets/fonts/Selawik-Semibold.ttf");
/// Paper's faces. Google Fonts ships both only as variable fonts, so one
/// file gives every weight.
const LORA_TTF: &[u8] = include_bytes!("../../assets/fonts/Lora-VariableFont_wght.ttf");
const CORMORANT_TTF: &[u8] = include_bytes!("../../assets/fonts/Cormorant-VariableFont_wght.ttf");

/// Family name for the semibold weight: egui picks faces by family, not by
/// weight, so the second weight is a family of its own.
pub const SEMIBOLD_FAMILY: &str = "semibold";
/// Titles and headings, 18 px and up. The same faces as semibold except in
/// Paper, where titles are Cormorant.
pub const HEAD_FAMILY: &str = "head";
const HEAD_SIZE: f32 = 18.0;

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

static CURRENT_MODE: AtomicU8 = AtomicU8::new(0);
static CURRENT_THEME: AtomicU8 = AtomicU8::new(0);
/// A theme the settings window shows while the mouse is over its tile;
/// `NO_PREVIEW` when none.
static PREVIEW: AtomicU8 = AtomicU8::new(NO_PREVIEW);
const NO_PREVIEW: u8 = u8::MAX;

fn theme_index(name: ThemeName) -> u8 {
    ThemeName::ALL
        .iter()
        .position(|each| *each == name)
        .unwrap_or(0) as u8
}

fn theme_at(index: u8) -> Option<ThemeName> {
    ThemeName::ALL.get(usize::from(index)).copied()
}

/// The theme and mode the user chose, for code outside the egui thread: the
/// ShortcutDetector's Win32 window and the notices take their colours from
/// them.
pub fn set_current(theme: ThemeName, mode: ThemeChoice) {
    let value = match mode {
        ThemeChoice::System => 0,
        ThemeChoice::Light => 1,
        ThemeChoice::Dark => 2,
    };
    CURRENT_MODE.store(value, Ordering::Relaxed);
    CURRENT_THEME.store(theme_index(theme), Ordering::Relaxed);
}

pub fn current() -> ThemeChoice {
    match CURRENT_MODE.load(Ordering::Relaxed) {
        1 => ThemeChoice::Light,
        2 => ThemeChoice::Dark,
        _ => ThemeChoice::System,
    }
}

pub fn current_theme() -> ThemeName {
    theme_at(CURRENT_THEME.load(Ordering::Relaxed)).unwrap_or_default()
}

/// The tokens a Win32 surface paints with right now: the chosen theme, or
/// the one being previewed, or High contrast while Windows has it on.
pub fn current_tokens() -> Tokens {
    let preview = theme_at(PREVIEW.load(Ordering::Relaxed));
    let theme = effective(current_theme(), preview, windows_high_contrast());
    Tokens::of(theme, is_dark(current()))
}

/// The theme WinCraft shows. Windows high contrast comes first, because the
/// person needs it to read the screen; then a preview; then the choice.
pub fn effective(chosen: ThemeName, preview: Option<ThemeName>, forced: bool) -> ThemeName {
    if forced {
        ThemeName::HighContrast
    } else {
        preview.unwrap_or(chosen)
    }
}

/// Whether the Windows high contrast setting is on.
pub fn windows_high_contrast() -> bool {
    let mut info = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            &mut info as *mut HIGHCONTRASTW as *mut core::ffi::c_void,
            0,
        )
    };
    read != 0 && info.dwFlags & HCF_HIGHCONTRASTON != 0
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

/// Which faces the UI text uses. Paths and code stay in JetBrains Mono in
/// every theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontSet {
    /// Segoe UI, or the bundled Selawik.
    System,
    /// Cascadia Mono from Windows, for Terminal.
    Mono,
    /// The bundled Lora for text and Cormorant for titles, for Paper.
    Book,
}

/// A theme's tokens for one mode. Stored in the egui context so every
/// painted widget reads the same values the theme applied. Components never
/// hard-code a colour, a corner, the row height, the border width or a
/// font; adding a theme means adding one more set here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub theme: ThemeName,
    pub dark: bool,
    pub window_bg: Color32,
    pub panel_bg: Color32,
    pub elevated_bg: Color32,
    pub border: Color32,
    /// The outline of a window or panel: the palette and the tiles.
    pub rim: Color32,
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
    /// The icon on a tile.
    pub tile_fg: Color32,
    /// Corner radius of windows and panels.
    pub radius_window: u8,
    /// Corner radius of rows, buttons, boxes and chips.
    pub radius_row: u8,
    /// Corner radius of tiles and cards.
    pub radius_tile: u8,
    /// Height of a palette result row.
    pub row_height: f32,
    /// Width of borders and rules, before snapping to whole pixels.
    pub border_width: f32,
    /// Width of the accent rule on the left of a selected row; 0 for none.
    pub pill_width: f32,
    /// Height of that rule; None for the whole row. Fluent will want 16.
    pub pill_height: Option<f32>,
    /// An accent rule under the palette's search field; 0 for none.
    pub search_line: f32,
    /// The selected row gets an accent outline instead of a tint.
    pub selection_ring: bool,
    /// Whether a tile's icon sits on a filled band.
    pub tile_fill: bool,
    pub shadows: bool,
    pub fonts: FontSet,
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

/// `amount` of the way from `from` to `to`, for the hover and pressed
/// shades of an accent the design gives no value for.
fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let channel =
        |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

/// The colours a theme sets. Everything else comes from Classic in the same
/// mode, or is worked out from these.
struct Palette {
    bg: Color32,
    glass: Color32,
    glass2: Color32,
    rim: Color32,
    border: Color32,
    text: Color32,
    text2: Color32,
    text3: Color32,
    accent: Color32,
    selection: Color32,
    success: Color32,
    tile_fg: Color32,
}

impl Tokens {
    /// Classic dark: the look WinCraft had before themes, value for value.
    pub fn dark() -> Self {
        let accent = hex(0xe1ad66);
        Self {
            theme: ThemeName::Classic,
            dark: true,
            window_bg: hex(0x1c1a19),
            panel_bg: hex(0x262322),
            elevated_bg: hex(0x2d2b2b),
            border: hex(0x444141),
            rim: hex(0x444141),
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
            tile_fg: accent,
            radius_window: 8,
            radius_row: 4,
            radius_tile: 6,
            row_height: 44.0,
            border_width: 1.0,
            pill_width: 2.0,
            pill_height: None,
            search_line: 0.0,
            selection_ring: false,
            tile_fill: true,
            shadows: true,
            fonts: FontSet::System,
        }
    }

    /// Classic light.
    pub fn light() -> Self {
        let accent = hex(0xb68235);
        Self {
            dark: false,
            window_bg: hex(0xf3f2f2),
            panel_bg: hex(0xeae9e9),
            elevated_bg: hex(0xfaf8f7),
            border: hex(0xd7d3d3),
            rim: hex(0xd7d3d3),
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
            tile_fg: accent,
            ..Self::dark()
        }
    }

    fn classic(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// A theme's colours over Classic's shape for the mode.
    fn painted(theme: ThemeName, dark: bool, colours: Palette) -> Self {
        let (lighter, darker) = if dark {
            (Color32::WHITE, Color32::BLACK)
        } else {
            (Color32::BLACK, Color32::WHITE)
        };
        Self {
            theme,
            window_bg: colours.bg,
            panel_bg: colours.glass2,
            elevated_bg: colours.glass,
            border: colours.border,
            rim: colours.rim,
            text_primary: colours.text,
            text_secondary: colours.text2,
            text_disabled: colours.text3,
            accent: colours.accent,
            accent_hover: mix(colours.accent, lighter, 0.25),
            accent_pressed: mix(colours.accent, darker, 0.2),
            accent_tint: with_alpha(colours.accent, 0.10),
            selection_bg: colours.selection,
            hover_bg: with_alpha(colours.text, 0.05),
            success: colours.success,
            focus_ring: colours.accent,
            tile_fg: colours.tile_fg,
            ..Self::classic(dark)
        }
    }

    fn terminal(dark: bool) -> Self {
        let colours = if dark {
            Palette {
                bg: hex(0x0f0e0d),
                glass: hex(0x141210),
                glass2: hex(0x0f0e0d),
                rim: hex(0x2c2926),
                border: hex(0x2c2926),
                text: hex(0xe8e3dc),
                text2: hex(0xa39d94),
                text3: hex(0x6b665f),
                accent: hex(0xf0a640),
                selection: with_alpha(hex(0xf0a640), 0.12),
                success: hex(0x7fc77a),
                tile_fg: hex(0xf0a640),
            }
        } else {
            Palette {
                bg: hex(0xfbfaf7),
                glass: hex(0xfbfaf7),
                glass2: hex(0xf3f1ec),
                rim: hex(0xd9d4cb),
                border: hex(0xe3ded5),
                text: hex(0x1a1815),
                text2: hex(0x5c574f),
                text3: hex(0x8f897f),
                accent: hex(0xa86a12),
                selection: with_alpha(hex(0xa86a12), 0.10),
                success: hex(0x2f7a3a),
                tile_fg: hex(0xa86a12),
            }
        };
        let painted = Self::painted(ThemeName::Terminal, dark, colours);
        let palette_shadow = if dark {
            Color32::from_rgba_unmultiplied(0, 0, 0, 128)
        } else {
            painted.palette_shadow
        };
        Self {
            palette_shadow,
            radius_window: 4,
            radius_row: 0,
            radius_tile: 0,
            row_height: 34.0,
            search_line: 1.0,
            tile_fill: false,
            fonts: FontSet::Mono,
            ..painted
        }
    }

    fn paper(dark: bool) -> Self {
        let colours = if dark {
            Palette {
                bg: hex(0x1f1b17),
                glass: hex(0x24201b),
                glass2: hex(0x1f1b17),
                rim: hex(0x4a4238),
                border: hex(0x3a342c),
                text: hex(0xefe9df),
                text2: hex(0xbdb3a4),
                text3: hex(0x857b6d),
                accent: hex(0xd0a060),
                selection: with_alpha(hex(0xd0a060), 0.10),
                success: hex(0x9cbf86),
                tile_fg: hex(0xd0a060),
            }
        } else {
            // The Classical design system's own values: --color-bg,
            // --color-text, neutral-700 and neutral-500, --color-accent,
            // accent-100 and neutral-300.
            Palette {
                bg: hex(0xf3f2f2),
                glass: hex(0xf7f5f2),
                glass2: hex(0xf3f2f2),
                rim: hex(0xd7d3d3),
                border: hex(0xdcd8d3),
                text: hex(0x201f1d),
                text2: hex(0x605d5d),
                text3: hex(0x9b9797),
                accent: hex(0xb68235),
                selection: hex(0xfff3e4),
                success: hex(0x3f7a4a),
                tile_fg: hex(0xb68235),
            }
        };
        Self {
            radius_window: 4,
            radius_row: 2,
            radius_tile: 0,
            pill_width: 1.0,
            search_line: 1.0,
            tile_fill: false,
            fonts: FontSet::Book,
            ..Self::painted(ThemeName::Paper, dark, colours)
        }
    }

    /// Black in dark mode, white in light mode.
    fn high_contrast(dark: bool) -> Self {
        let (colours, warning, danger, info) = if dark {
            (
                Palette {
                    bg: Color32::BLACK,
                    glass: Color32::BLACK,
                    glass2: Color32::BLACK,
                    rim: Color32::WHITE,
                    border: Color32::WHITE,
                    text: Color32::WHITE,
                    text2: Color32::WHITE,
                    text3: hex(0xc8c8c8),
                    accent: hex(0xffff00),
                    selection: Color32::TRANSPARENT,
                    success: hex(0x3ff23f),
                    tile_fg: hex(0xffff00),
                },
                hex(0xffb000),
                hex(0xff6b6b),
                hex(0x8fd0ff),
            )
        } else {
            (
                Palette {
                    bg: Color32::WHITE,
                    glass: Color32::WHITE,
                    glass2: Color32::WHITE,
                    rim: Color32::BLACK,
                    border: Color32::BLACK,
                    text: Color32::BLACK,
                    text2: Color32::BLACK,
                    text3: hex(0x3a3a3a),
                    accent: hex(0x5a00b0),
                    selection: Color32::TRANSPARENT,
                    success: hex(0x006b00),
                    tile_fg: hex(0x5a00b0),
                },
                hex(0x8a4000),
                hex(0xb00000),
                hex(0x00508a),
            )
        };
        let painted = Self::painted(ThemeName::HighContrast, dark, colours);
        Self {
            warning,
            danger,
            info,
            // A 5 % tint cannot be seen on pure black or white.
            hover_bg: with_alpha(painted.text_primary, 0.15),
            palette_shadow: Color32::TRANSPARENT,
            border_width: 2.0,
            radius_window: 4,
            radius_row: 2,
            radius_tile: 0,
            pill_width: 0.0,
            search_line: 2.0,
            selection_ring: true,
            shadows: false,
            ..painted
        }
    }

    pub fn of(theme: ThemeName, dark: bool) -> Self {
        match theme {
            ThemeName::Classic => Self::classic(dark),
            ThemeName::Terminal => Self::terminal(dark),
            ThemeName::Paper => Self::paper(dark),
            ThemeName::HighContrast => Self::high_contrast(dark),
        }
    }

    /// Falls back to Classic dark before `apply` has run, which only happens
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

/// A border or rule in the theme's border width.
pub fn line(ctx: &egui::Context, colour: Color32) -> Stroke {
    stroke(ctx, Tokens::get(ctx).border_width, colour)
}

pub fn semibold_family() -> FontFamily {
    FontFamily::Name(SEMIBOLD_FAMILY.into())
}

pub fn head_family() -> FontFamily {
    FontFamily::Name(HEAD_FAMILY.into())
}

/// The UI's regular weight, for everything readable.
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// The UI's semibold weight: titles, the wordmark and section headings.
/// From 18 px up it is a title, which Paper sets in its title face.
pub fn semibold(size: f32) -> FontId {
    if size >= HEAD_SIZE {
        FontId::new(size, head_family())
    } else {
        FontId::new(size, semibold_family())
    }
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Segoe UI Regular and Semibold as every Windows 10 and 11 installation
/// ships them. The static files, not the variable Segoe UI Variable, so
/// Classic keeps the faces it always had. Read from the user's own system at
/// start-up; they are never copied into WinCraft.
fn system_fonts() -> Option<(Vec<u8>, Vec<u8>)> {
    let fonts = windows_fonts()?;
    let regular = std::fs::read(fonts.join("segoeui.ttf")).ok()?;
    let semibold = std::fs::read(fonts.join("seguisb.ttf")).ok()?;
    Some((regular, semibold))
}

fn windows_fonts() -> Option<std::path::PathBuf> {
    let windows = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot"))?;
    Some(std::path::PathBuf::from(windows).join("Fonts"))
}

/// The regular file and, when the face is not variable, its bold file.
type TerminalFiles = (Vec<u8>, Option<Vec<u8>>);

/// Terminal's regular and semibold faces from the system: Cascadia Mono,
/// then Cascadia Code, then Consolas, which Windows 11 all ship. Cascadia
/// is a variable font, so its semibold is the same file at weight 600;
/// Consolas has a separate bold file. Read once, the first time Terminal is
/// shown, and kept for the rest of the run.
fn terminal_faces() -> Option<(FontData, FontData)> {
    static FACES: OnceLock<Option<TerminalFiles>> = OnceLock::new();
    let (regular, bold) = FACES
        .get_or_init(|| {
            let fonts = windows_fonts()?;
            for name in ["CascadiaMono.ttf", "CascadiaCode.ttf"] {
                if let Ok(bytes) = std::fs::read(fonts.join(name)) {
                    log::info!(target: "theme", "Terminal font: {name}");
                    return Some((bytes, None));
                }
            }
            let regular = std::fs::read(fonts.join("consola.ttf")).ok()?;
            log::info!(target: "theme", "Terminal font: Consolas");
            Some((regular, std::fs::read(fonts.join("consolab.ttf")).ok()))
        })
        .as_ref()?;
    let semibold = match bold {
        Some(bold) => FontData::from_static(bold),
        None => weighted(regular, 600.0),
    };
    Some((FontData::from_static(regular), semibold))
}

fn weighted(bytes: &'static [u8], weight: f32) -> FontData {
    FontData::from_static(bytes).tweak(FontTweak {
        coords: VariationCoords::new([(b"wght", weight)]),
        ..FontTweak::default()
    })
}

/// Segoe UI from the system when it is there, the bundled Selawik (made to
/// Segoe UI's metrics) when not, and JetBrains Mono for paths and code.
/// egui's own fonts stay last in every chain for symbols and scripts the
/// others lack.
pub fn install_fonts(ctx: &egui::Context) {
    install_fonts_from(ctx, system_fonts());
}

/// The fonts Classic uses, kept so a theme with other faces can be built
/// on them and Classic can be put back exactly.
#[derive(Clone)]
struct BaseFonts(Arc<FontDefinitions>);

fn base_fonts_id() -> egui::Id {
    egui::Id::new("wincraft-base-fonts")
}

fn font_set_id() -> egui::Id {
    egui::Id::new("wincraft-font-set")
}

fn install_fonts_from(ctx: &egui::Context, system: Option<(Vec<u8>, Vec<u8>)>) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, data: FontData| {
        fonts.font_data.insert(name.to_string(), Arc::new(data));
    };
    add(SELAWIK, FontData::from_static(SELAWIK_REGULAR_TTF));
    add(
        SELAWIK_SEMIBOLD,
        FontData::from_static(SELAWIK_SEMIBOLD_TTF),
    );
    add(
        MONO,
        FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        )),
    );
    let (regular, semibold): (Vec<&str>, Vec<&str>) = match system {
        Some((regular_bytes, semibold_bytes)) => {
            add(SEGOE, FontData::from_owned(regular_bytes));
            add(SEGOE_SEMIBOLD, FontData::from_owned(semibold_bytes));
            log::info!(target: "theme", "UI font: Segoe UI from the system");
            (vec![SEGOE, SELAWIK], vec![SEGOE_SEMIBOLD, SELAWIK_SEMIBOLD])
        }
        None => {
            log::info!(target: "theme", "UI font: bundled Selawik (Segoe UI not found)");
            (vec![SELAWIK], vec![SELAWIK_SEMIBOLD])
        }
    };

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
        chain(&regular, &fallback_proportional),
    );
    fonts
        .families
        .insert(FontFamily::Monospace, chain(&[MONO], &fallback_mono));
    let mut semibold_chain = semibold;
    semibold_chain.extend(regular);
    let semibold_chain = chain(&semibold_chain, &fallback_proportional);
    fonts.families.insert(head_family(), semibold_chain.clone());
    fonts.families.insert(semibold_family(), semibold_chain);
    ctx.data_mut(|data| {
        data.insert_temp(base_fonts_id(), BaseFonts(Arc::new(fonts.clone())));
        data.insert_temp(font_set_id(), FontSet::System);
    });
    ctx.set_fonts(fonts);
}

/// Classic's fonts with a theme's faces put first in each chain, so a
/// character those faces lack still comes from Segoe UI.
fn fonts_for(base: &FontDefinitions, set: FontSet) -> FontDefinitions {
    let mut fonts = base.clone();
    let faces: Vec<(&str, FontData)> = match set {
        FontSet::System => Vec::new(),
        FontSet::Mono => match terminal_faces() {
            Some((regular, semibold)) => vec![(TERMINAL, regular), (TERMINAL_SEMIBOLD, semibold)],
            None => Vec::new(),
        },
        FontSet::Book => vec![
            (LORA, FontData::from_static(LORA_TTF)),
            (LORA_SEMIBOLD, weighted(LORA_TTF, 600.0)),
            (CORMORANT_SEMIBOLD, weighted(CORMORANT_TTF, 600.0)),
        ],
    };
    if faces.is_empty() {
        return fonts;
    }
    let (regular, semibold, head) = match set {
        FontSet::Book => (LORA, LORA_SEMIBOLD, CORMORANT_SEMIBOLD),
        _ => (TERMINAL, TERMINAL_SEMIBOLD, TERMINAL_SEMIBOLD),
    };
    for (name, data) in faces {
        fonts.font_data.insert(name.to_string(), Arc::new(data));
    }
    for (family, first) in [
        (FontFamily::Proportional, regular),
        (semibold_family(), semibold),
        (head_family(), head),
    ] {
        if let Some(chain) = fonts.families.get_mut(&family) {
            chain.insert(0, first.to_string());
        }
    }
    fonts
}

/// Swaps the faces only when the theme's set differs from the one showing:
/// egui rebuilds its glyph atlas on every change of fonts.
fn apply_fonts(ctx: &egui::Context, set: FontSet) {
    let (base, showing) = ctx.data(|data| {
        (
            data.get_temp::<BaseFonts>(base_fonts_id()),
            data.get_temp::<FontSet>(font_set_id()),
        )
    });
    let Some(BaseFonts(base)) = base else {
        return;
    };
    if showing == Some(set) {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(font_set_id(), set));
    ctx.set_fonts(fonts_for(&base, set));
}

/// What the user chose and what the settings window previews, so a
/// re-apply (a new monitor scale, a Windows colour change) keeps both.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    chosen: ThemeName,
    mode: ThemeChoice,
    preview: Option<ThemeName>,
}

fn look_id() -> egui::Id {
    egui::Id::new("wincraft-look")
}

/// Shows `preview` in every WinCraft window until it is called with None,
/// which puts the chosen theme back.
pub fn preview(ctx: &egui::Context, preview: Option<ThemeName>) {
    PREVIEW.store(preview.map_or(NO_PREVIEW, theme_index), Ordering::Relaxed);
    let look = ctx.data(|data| data.get_temp::<Look>(look_id()));
    let Some(look) = look else {
        return;
    };
    if look.preview != preview {
        show(ctx, Look { preview, ..look });
    }
}

/// The theme being previewed, if any.
pub fn previewing(ctx: &egui::Context) -> Option<ThemeName> {
    ctx.data(|data| data.get_temp::<Look>(look_id()))
        .and_then(|look| look.preview)
}

/// The shadow under the palette and other floating panels.
pub fn window_shadow(tokens: &Tokens) -> Shadow {
    if tokens.shadows {
        Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: tokens.palette_shadow,
        }
    } else {
        Shadow::NONE
    }
}

/// One place decides how every window looks, so the palette, the settings
/// window and the strip can never drift apart.
pub fn apply(ctx: &egui::Context, theme: ThemeName, mode: ThemeChoice) {
    let preview = previewing(ctx);
    show(
        ctx,
        Look {
            chosen: theme,
            mode,
            preview,
        },
    );
}

fn show(ctx: &egui::Context, look: Look) {
    ctx.data_mut(|data| data.insert_temp(look_id(), look));
    let theme = effective(look.chosen, look.preview, windows_high_contrast());
    let tokens = Tokens::of(theme, is_dark(look.mode));
    apply_fonts(ctx, tokens.fonts);
    let ppp = ctx.pixels_per_point();
    let line = |colour: Color32| Stroke::new(snap(tokens.border_width, ppp), colour);
    let r4 = CornerRadius::same(tokens.radius_row);

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
    visuals.window_corner_radius = CornerRadius::same(tokens.radius_window);
    visuals.menu_corner_radius = r4;
    visuals.window_shadow = window_shadow(&tokens);
    visuals.popup_shadow = if tokens.shadows {
        Shadow {
            offset: [0, 6],
            blur: 16,
            spread: 0,
            color: tokens.palette_shadow,
        }
    } else {
        Shadow::NONE
    };
    // Selected text still needs a fill where a selected row has none.
    visuals.selection.bg_fill = if tokens.selection_ring {
        with_alpha(tokens.accent, 0.35)
    } else {
        tokens.selection_bg
    };
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
            (TextStyle::Name(TITLE.into()), semibold(28.0)),
            (TextStyle::Heading, semibold(20.0)),
            (TextStyle::Name(SECTION.into()), semibold(13.0)),
            (TextStyle::Body, regular(14.0)),
            (TextStyle::Small, regular(13.0)),
            (TextStyle::Name(CAPTION.into()), regular(12.0)),
            (TextStyle::Button, regular(14.0)),
            (TextStyle::Name(KEYCAP.into()), regular(13.0)),
            (TextStyle::Name(HINT.into()), regular(12.0)),
            (TextStyle::Monospace, mono(13.0)),
            (TextStyle::Name(QUERY.into()), regular(16.0)),
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
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn strokes_snap_up_to_whole_physical_pixels() {
        assert_eq!(snap(1.0, 1.0), 1.0);
        assert!((snap(1.0, 1.5) - 2.0 / 1.5).abs() < 1e-6);
        assert!((snap(2.0, 1.5) - 3.0 / 1.5).abs() < 1e-6);
        assert!((snap(1.0, 1.25) - 2.0 / 1.25).abs() < 1e-6);
        assert_eq!(snap(1.0, 2.0), 1.0);
    }

    fn first_faces(ctx: &egui::Context) -> [Option<String>; 3] {
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label(egui::RichText::new("WinCraft").font(semibold(20.0)));
                ui.label(egui::RichText::new("Toggle monitor 1").font(regular(14.0)));
                ui.label(egui::RichText::new("Win + Alt + F1").font(mono(12.0)));
            });
            // There is no renderer in a test to upload the font atlas to.
            output.textures_delta.clear();
        }
        let chains = ctx.fonts(|fonts| fonts.definitions().families.clone());
        let first = |family: FontFamily| chains.get(&family).and_then(|c| c.first()).cloned();
        [
            first(FontFamily::Proportional),
            first(semibold_family()),
            first(FontFamily::Monospace),
        ]
    }

    /// Glyph coverage is not asserted: egui's has_glyph reports every
    /// character as missing once the first face can draw the replacement
    /// glyph.
    #[test]
    fn without_segoe_ui_the_bundled_selawik_leads() {
        let ctx = egui::Context::default();
        install_fonts_from(&ctx, None);
        apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let [regular, semibold, mono] = first_faces(&ctx);
        assert_eq!(regular.as_deref(), Some(SELAWIK));
        assert_eq!(semibold.as_deref(), Some(SELAWIK_SEMIBOLD));
        assert_eq!(mono.as_deref(), Some(MONO));
    }

    #[test]
    fn system_fonts_lead_when_windows_has_them() {
        let bundled = |bytes: &[u8]| bytes.to_vec();
        let ctx = egui::Context::default();
        install_fonts_from(
            &ctx,
            Some((bundled(SELAWIK_REGULAR_TTF), bundled(SELAWIK_SEMIBOLD_TTF))),
        );
        apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let [regular, semibold, _] = first_faces(&ctx);
        assert_eq!(regular.as_deref(), Some(SEGOE));
        assert_eq!(semibold.as_deref(), Some(SEGOE_SEMIBOLD));
    }

    #[test]
    fn gdi_colours_are_blue_green_red() {
        assert_eq!(colorref(hex(0x1c1a19)), 0x00191a1c);
    }

    #[test]
    fn the_current_theme_round_trips() {
        for name in ThemeName::ALL {
            for choice in ThemeChoice::ALL {
                set_current(name, choice);
                assert_eq!(current(), choice);
                assert_eq!(current_theme(), name);
            }
        }
        set_current(ThemeName::Classic, ThemeChoice::System);
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

    /// Every value WinCraft drew with before themes, written out again, so
    /// a change to Classic cannot slip in through a shared default.
    #[test]
    fn classic_is_exactly_the_look_from_before_themes() {
        let shape = |tokens: &Tokens| {
            assert_eq!(tokens.theme, ThemeName::Classic);
            assert_eq!(tokens.radius_window, 8);
            assert_eq!(tokens.radius_row, 4);
            assert_eq!(tokens.radius_tile, 6);
            assert_eq!(tokens.row_height, 44.0);
            assert_eq!(tokens.border_width, 1.0);
            assert_eq!(tokens.pill_width, 2.0);
            assert_eq!(tokens.pill_height, None);
            assert_eq!(tokens.search_line, 0.0);
            assert!(!tokens.selection_ring);
            assert!(tokens.tile_fill);
            assert!(tokens.shadows);
            assert_eq!(tokens.fonts, FontSet::System);
            assert_eq!(tokens.rim, tokens.border);
            assert_eq!(tokens.tile_fg, tokens.accent);
            assert_eq!(tokens.focus_ring, tokens.accent);
        };

        let dark = Tokens::of(ThemeName::Classic, true);
        assert_eq!(dark, Tokens::dark());
        shape(&dark);
        let colours = [
            (dark.window_bg, hex(0x1c1a19)),
            (dark.panel_bg, hex(0x262322)),
            (dark.elevated_bg, hex(0x2d2b2b)),
            (dark.border, hex(0x444141)),
            (dark.text_primary, hex(0xf3f2f2)),
            (dark.text_secondary, hex(0xbab6b6)),
            (dark.text_disabled, hex(0x7d7979)),
            (dark.accent, hex(0xe1ad66)),
            (dark.accent_hover, hex(0xfacb8d)),
            (dark.accent_pressed, hex(0xc28d41)),
            (dark.accent_tint, with_alpha(hex(0xe1ad66), 0.10)),
            (dark.selection_bg, with_alpha(hex(0xe1ad66), 0.18)),
            (dark.hover_bg, with_alpha(hex(0xf3f2f2), 0.05)),
            (dark.success, hex(0x8fbf7f)),
            (dark.warning, hex(0xe88a4a)),
            (dark.danger, hex(0xe07a6d)),
            (dark.info, hex(0x8fb0d0)),
            (
                dark.palette_shadow,
                Color32::from_rgba_unmultiplied(0, 0, 0, 115),
            ),
        ];
        for (index, (got, want)) in colours.into_iter().enumerate() {
            assert_eq!(got, want, "dark colour {index}");
        }

        let light = Tokens::of(ThemeName::Classic, false);
        assert_eq!(light, Tokens::light());
        shape(&light);
        let colours = [
            (light.window_bg, hex(0xf3f2f2)),
            (light.panel_bg, hex(0xeae9e9)),
            (light.elevated_bg, hex(0xfaf8f7)),
            (light.border, hex(0xd7d3d3)),
            (light.text_primary, hex(0x201f1d)),
            (light.text_secondary, hex(0x605d5d)),
            (light.text_disabled, hex(0x9b9797)),
            (light.accent, hex(0xb68235)),
            (light.accent_hover, hex(0xa06f24)),
            (light.accent_pressed, hex(0x7d5411)),
            (light.accent_tint, with_alpha(hex(0xb68235), 0.10)),
            (light.selection_bg, with_alpha(hex(0xb68235), 0.16)),
            (light.hover_bg, with_alpha(hex(0x201f1d), 0.05)),
            (light.success, hex(0x3f7a4a)),
            (light.warning, hex(0xb45f14)),
            (light.danger, hex(0x9f3a2f)),
            (light.info, hex(0x4a6a8a)),
            (
                light.palette_shadow,
                Color32::from_rgba_unmultiplied(45, 43, 43, 56),
            ),
        ];
        for (index, (got, want)) in colours.into_iter().enumerate() {
            assert_eq!(got, want, "light colour {index}");
        }
    }

    #[test]
    fn every_theme_has_a_full_token_set_in_both_modes() {
        for name in ThemeName::ALL {
            for dark in [true, false] {
                let tokens = Tokens::of(name, dark);
                let what = format!("{name:?} dark={dark}");
                assert_eq!(tokens.theme, name, "{what}");
                assert_eq!(tokens.dark, dark, "{what}");
                for colour in [
                    tokens.window_bg,
                    tokens.panel_bg,
                    tokens.elevated_bg,
                    tokens.border,
                    tokens.rim,
                    tokens.text_primary,
                    tokens.text_secondary,
                    tokens.text_disabled,
                    tokens.accent,
                    tokens.accent_hover,
                    tokens.accent_pressed,
                    tokens.success,
                    tokens.warning,
                    tokens.danger,
                    tokens.info,
                    tokens.focus_ring,
                    tokens.tile_fg,
                ] {
                    assert_eq!(colour.a(), 255, "{what}: solid colours are solid");
                }
                assert_ne!(tokens.text_primary, tokens.window_bg, "{what}");
                assert_ne!(tokens.accent, tokens.window_bg, "{what}");
                assert!(tokens.hover_bg.a() > 0, "{what}");
                assert!(tokens.row_height >= 34.0, "{what}");
                assert!(tokens.border_width >= 1.0, "{what}");
                assert!(tokens.radius_window <= 12, "{what}");
                assert!(
                    tokens.selection_ring || tokens.selection_bg.a() > 0,
                    "{what}: a selected row must show"
                );
                assert!(
                    tokens.pill_width > 0.0 || tokens.selection_ring,
                    "{what}: a selected row must show without colour too"
                );
            }
        }
    }

    #[test]
    fn the_new_themes_have_the_values_from_the_design_page() {
        let terminal = Tokens::of(ThemeName::Terminal, true);
        assert_eq!(terminal.window_bg, hex(0x0f0e0d));
        assert_eq!(terminal.accent, hex(0xf0a640));
        assert_eq!(terminal.selection_bg, with_alpha(hex(0xf0a640), 0.12));
        assert_eq!((terminal.radius_window, terminal.radius_row), (4, 0));
        assert_eq!(terminal.row_height, 34.0);
        assert_eq!(terminal.fonts, FontSet::Mono);
        assert!(!terminal.tile_fill);
        let terminal_light = Tokens::of(ThemeName::Terminal, false);
        assert_eq!(terminal_light.rim, hex(0xd9d4cb));
        assert_eq!(terminal_light.border, hex(0xe3ded5));

        let paper = Tokens::of(ThemeName::Paper, true);
        assert_eq!(paper.window_bg, hex(0x1f1b17));
        assert_eq!(paper.accent, hex(0xd0a060));
        assert_eq!(paper.pill_width, 1.0);
        assert_eq!(paper.fonts, FontSet::Book);
        let paper_light = Tokens::of(ThemeName::Paper, false);
        assert_eq!(paper_light.elevated_bg, hex(0xf7f5f2));
        assert_eq!(paper_light.selection_bg, hex(0xfff3e4));

        let black = Tokens::of(ThemeName::HighContrast, true);
        assert_eq!(black.window_bg, Color32::BLACK);
        assert_eq!(black.accent, hex(0xffff00));
        assert_eq!(black.border_width, 2.0);
        assert_eq!(black.search_line, 2.0);
        assert!(black.selection_ring && !black.shadows);
        assert_eq!(black.selection_bg, Color32::TRANSPARENT);
        let white = Tokens::of(ThemeName::HighContrast, false);
        assert_eq!(white.window_bg, Color32::WHITE);
        assert_eq!(white.accent, hex(0x5a00b0));
        assert_eq!(white.text_disabled, hex(0x3a3a3a));
    }

    #[test]
    fn windows_high_contrast_wins_over_the_choice_and_a_preview() {
        assert_eq!(effective(ThemeName::Paper, None, false), ThemeName::Paper);
        assert_eq!(
            effective(ThemeName::Paper, Some(ThemeName::Terminal), false),
            ThemeName::Terminal
        );
        assert_eq!(
            effective(ThemeName::Paper, None, true),
            ThemeName::HighContrast
        );
        assert_eq!(
            effective(ThemeName::Classic, Some(ThemeName::Terminal), true),
            ThemeName::HighContrast
        );
    }

    fn families(ctx: &egui::Context) -> BTreeMap<FontFamily, Vec<String>> {
        // set_fonts takes effect in the next pass.
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        ctx.fonts(|fonts| fonts.definitions().families.clone())
    }

    /// The tests run on PCs with Windows high contrast both on and off, so
    /// they only look at a theme when it is not forced.
    fn shown(ctx: &egui::Context) -> ThemeName {
        Tokens::get(ctx).theme
    }

    #[test]
    fn paper_puts_its_faces_first_and_classic_puts_the_old_chains_back() {
        let ctx = egui::Context::default();
        install_fonts_from(&ctx, None);
        apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let classic = families(&ctx);
        assert_eq!(
            classic.get(&head_family()),
            classic.get(&semibold_family()),
            "Classic titles use the same faces as before"
        );
        if windows_high_contrast() {
            return;
        }

        apply(&ctx, ThemeName::Paper, ThemeChoice::Dark);
        assert_eq!(shown(&ctx), ThemeName::Paper);
        let paper = families(&ctx);
        let first = |chains: &BTreeMap<FontFamily, Vec<String>>, family: FontFamily| {
            chains.get(&family).and_then(|c| c.first()).cloned()
        };
        assert_eq!(
            first(&paper, FontFamily::Proportional).as_deref(),
            Some(LORA)
        );
        assert_eq!(
            first(&paper, semibold_family()).as_deref(),
            Some(LORA_SEMIBOLD)
        );
        assert_eq!(
            first(&paper, head_family()).as_deref(),
            Some(CORMORANT_SEMIBOLD)
        );
        assert_eq!(
            first(&paper, FontFamily::Monospace).as_deref(),
            Some(MONO),
            "paths stay in JetBrains Mono"
        );

        apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        assert_eq!(families(&ctx), classic);
    }

    #[test]
    fn terminal_uses_a_windows_monospace_face_when_there_is_one() {
        let ctx = egui::Context::default();
        install_fonts_from(&ctx, None);
        apply(&ctx, ThemeName::Terminal, ThemeChoice::Light);
        if windows_high_contrast() {
            return;
        }
        let chains = families(&ctx);
        let first = chains
            .get(&FontFamily::Proportional)
            .and_then(|c| c.first())
            .cloned();
        if terminal_faces().is_some() {
            assert_eq!(first.as_deref(), Some(TERMINAL));
        } else {
            assert_eq!(first.as_deref(), Some(SELAWIK));
        }
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label(egui::RichText::new("Settings").font(semibold(20.0)));
            ui.label(egui::RichText::new("Toggle monitor 1").font(regular(14.0)));
        });
        output.textures_delta.clear();
    }

    #[test]
    fn a_preview_shows_until_it_is_cleared_and_survives_a_re_apply() {
        let ctx = egui::Context::default();
        install_fonts_from(&ctx, None);
        apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        preview(&ctx, Some(ThemeName::Paper));
        assert_eq!(previewing(&ctx), Some(ThemeName::Paper));
        // A new monitor scale or a Windows colour change re-applies.
        apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        assert_eq!(previewing(&ctx), Some(ThemeName::Paper));
        if !windows_high_contrast() {
            assert_eq!(shown(&ctx), ThemeName::Paper);
        }
        preview(&ctx, None);
        assert_eq!(previewing(&ctx), None);
        if !windows_high_contrast() {
            assert_eq!(shown(&ctx), ThemeName::Classic);
            assert_eq!(Tokens::get(&ctx), Tokens::dark());
        }
    }

    #[test]
    fn every_theme_draws_text_in_both_modes() {
        let ctx = egui::Context::default();
        install_fonts_from(&ctx, None);
        for name in ThemeName::ALL {
            for mode in [ThemeChoice::Dark, ThemeChoice::Light] {
                apply(&ctx, name, mode);
                for _ in 0..2 {
                    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                        ui.label(egui::RichText::new("WinCraft").font(semibold(28.0)));
                        ui.label(egui::RichText::new("General").font(semibold(13.0)));
                        ui.label(egui::RichText::new("Start with Windows").font(regular(14.0)));
                        ui.label(egui::RichText::new("C:\\Windows").font(mono(13.0)));
                    });
                    output.textures_delta.clear();
                }
            }
        }
    }
}
