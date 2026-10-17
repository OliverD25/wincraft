//! The on-screen panel: a native layered popup, like the ScreenDimmer
//! overlays, because eframe cannot make a transparent secondary window.
//!
//! The picture is a premultiplied 32-bit bitmap handed to
//! `UpdateLayeredWindow`, which gives the translucent panel per-pixel alpha
//! and smooth rounded corners. GDI writes no alpha, so the text is drawn white
//! on black into a mask first and its grey level used as coverage when the
//! panel and the tinted text are mixed.

use egui::Color32;
use windows_sys::Win32::Foundation::{POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetMonitorInfoW, GetTextExtentPoint32W,
    GetTextMetricsW, MonitorFromWindow, SelectObject, HFONT, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    TEXTMETRICW,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::core::layered::{compose, faces, font, text_mask, utf16, Image, Layered, Run};
use crate::core::theme;

const CLASS_NAME: &str = "WinCraftLanguagePanel";

/// Sizes in logical pixels, scaled by the monitor's DPI.
const PAD_X: f32 = 24.0;
const PAD_Y: f32 = 18.0;
const LINE_GAP: f32 = 2.0;
const RADIUS: f32 = 12.0;
const BIG_TEXT: f32 = 44.0;
const DETAIL_TEXT: f32 = 14.0;
const MIN_WIDTH: f32 = 200.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Position {
    #[default]
    Centre,
    UpperThird,
    LowerThird,
}

pub struct Content {
    /// The code switched from, such as "UK"; None shows the target alone.
    pub from: Option<String>,
    pub to: String,
    /// The target's own name, with the layout when it is shown.
    pub detail: String,
}

pub struct Panel {
    window: Layered,
}

impl Panel {
    pub fn create() -> Result<Self, String> {
        Ok(Self {
            window: Layered::create(CLASS_NAME)?,
        })
    }

    /// Draws `content` on the monitor of the foreground window and shows it
    /// at full strength, without taking the focus.
    pub fn show(&mut self, content: &Content, position: Position) {
        let monitor = unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let scale = dpi_x.max(96) as f32 / 96.0;
        let tokens = theme::Tokens::for_choice(theme::current());
        let Some(image) = render(content, scale, &tokens) else {
            log::warn!("could not draw the language panel");
            return;
        };

        let work = info.rcWork;
        let (width, height) = (work.right - work.left, work.bottom - work.top);
        let centre_y = match position {
            Position::Centre => height / 2,
            Position::UpperThird => height / 3,
            Position::LowerThird => height * 2 / 3,
        };
        let at = POINT {
            x: work.left + (width - image.width) / 2,
            y: work.top + centre_y - image.height / 2,
        };
        if !self.window.present(&image, at) {
            log::warn!("could not show the language panel");
            return;
        }
        self.window.show();
    }

    /// Changes only the strength of what is already shown, for the fade.
    pub fn set_alpha(&mut self, alpha: u8) {
        self.window.set_alpha(alpha);
    }

    pub fn hide(&mut self) {
        self.window.hide();
    }

    pub fn destroy(&mut self) {
        self.window.destroy();
    }
}

fn render(content: &Content, scale: f32, tokens: &theme::Tokens) -> Option<Image> {
    let px = |logical: f32| (logical * scale).round() as i32;
    let [(big_face, big_weight), (detail_face, detail_weight)] = *faces();
    let big = font(big_face, big_weight, px(BIG_TEXT));
    let detail = font(detail_face, detail_weight, px(DETAIL_TEXT));
    let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };

    let measure = |text: &[u16], font: HFONT| -> SIZE {
        let mut size = SIZE { cx: 0, cy: 0 };
        unsafe {
            SelectObject(dc, font);
            GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut size);
        }
        size
    };
    let mut big_runs: Vec<(Vec<u16>, Color32)> = Vec::new();
    if let Some(from) = &content.from {
        big_runs.push((utf16(from), tokens.text_primary));
        big_runs.push((utf16(" \u{2192} "), tokens.accent));
    }
    big_runs.push((utf16(&content.to), tokens.text_primary));
    let big_sizes: Vec<SIZE> = big_runs
        .iter()
        .map(|(text, _)| measure(text, big))
        .collect();
    let big_width: i32 = big_sizes.iter().map(|size| size.cx).sum();
    let big_height = big_sizes.iter().map(|size| size.cy).max().unwrap_or(0);
    // The line box keeps room above the capitals for accents, which the codes
    // never use; without trimming it the panel looks bottom-heavy.
    let mut metrics: TEXTMETRICW = unsafe { std::mem::zeroed() };
    unsafe {
        SelectObject(dc, big);
        GetTextMetricsW(dc, &mut metrics);
    }
    let leading = metrics.tmInternalLeading.clamp(0, big_height);
    let detail_text = utf16(&content.detail);
    let detail_size = measure(&detail_text, detail);

    let width = (big_width.max(detail_size.cx) + 2 * px(PAD_X)).max(px(MIN_WIDTH));
    let height = 2 * px(PAD_Y) + big_height - leading + px(LINE_GAP) + detail_size.cy;

    let mut runs = Vec::new();
    let mut x = (width - big_width) / 2;
    for ((text, colour), size) in big_runs.into_iter().zip(&big_sizes) {
        runs.push(Run {
            text,
            colour,
            x,
            y: px(PAD_Y) - leading,
            width: size.cx,
            height: big_height,
            font: 0,
        });
        x += size.cx;
    }
    runs.push(Run {
        text: detail_text,
        colour: tokens.text_secondary,
        x: (width - detail_size.cx) / 2,
        y: px(PAD_Y) + big_height - leading + px(LINE_GAP),
        width: detail_size.cx,
        height: detail_size.cy,
        font: 1,
    });

    let mask = text_mask(dc, width, height, &runs, &[big, detail]);
    unsafe {
        DeleteDC(dc);
        DeleteObject(big);
        DeleteObject(detail);
    }
    let mask = mask?;
    let pixels = compose(
        width,
        height,
        RADIUS * scale,
        scale.round().max(1.0),
        tokens.elevated_bg,
        tokens.border,
        |x, y| {
            let coverage = f32::from(mask[(y * width + x) as usize]) / 255.0;
            let colour = runs
                .iter()
                .find(|run| {
                    x >= run.x && x < run.x + run.width && y >= run.y && y < run.y + run.height
                })
                .map(|run| run.colour)
                .unwrap_or(tokens.text_primary);
            (coverage, colour)
        },
    );
    Some(Image {
        width,
        height,
        pixels,
    })
}
