//! A native layered popup, like the ScreenDimmer overlays, because eframe
//! cannot make a transparent secondary window. The on-screen panels (the
//! language switch, the restore progress) draw with it.
//!
//! The picture is a premultiplied 32-bit bitmap handed to
//! `UpdateLayeredWindow`, which gives the translucent panel per-pixel alpha
//! and smooth rounded corners. GDI writes no alpha, so the text is drawn white
//! on black into a mask first and its grey level used as coverage when the
//! panel and the tinted text are mixed.

use std::sync::OnceLock;

use egui::Color32;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC,
    DeleteObject, GdiFlush, GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW,
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, FW_NORMAL,
    FW_SEMIBOLD, HDC, HFONT, HGDIOBJ, OUT_TT_PRECIS, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, HTTRANSPARENT, HWND_TOPMOST, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::core::{theme, wide};

/// How much of the page shows through the panel.
pub const FILL_ALPHA: f32 = 0.80;
pub const BORDER_ALPHA: f32 = 0.90;

/// A click-through, non-activating, topmost popup that shows one picture.
pub struct Layered {
    hwnd: HWND,
}

impl Layered {
    /// `class_name` is registered once per process; a second call with the
    /// same name reuses the class.
    pub fn create(class_name: &str) -> Result<Self, String> {
        let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let name = wide(class_name);
        let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
        class.lpfnWndProc = Some(panel_proc);
        class.hInstance = hinstance;
        class.lpszClassName = name.as_ptr();
        // Fails harmlessly with "class already exists" after the plugin was
        // switched off and on.
        unsafe { RegisterClassW(&class) };
        let hwnd = unsafe {
            CreateWindowExW(
                // Layered gives the alpha, Transparent lets clicks through,
                // NoActivate and ToolWindow keep it out of the focus chain,
                // the taskbar and Alt+Tab.
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                name.as_ptr(),
                wide("").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("could not create its panel window".to_string());
        }
        Ok(Self { hwnd })
    }

    /// Copies the picture into a DIB and hands it to the window, at full
    /// strength.
    pub fn present(&self, image: &Image, at: POINT) -> bool {
        let screen = unsafe { GetDC(std::ptr::null_mut()) };
        let dc = unsafe { CreateCompatibleDC(screen) };
        let Some((bitmap, bits)) = dib(dc, image.width, image.height) else {
            unsafe {
                DeleteDC(dc);
                ReleaseDC(std::ptr::null_mut(), screen);
            }
            return false;
        };
        unsafe {
            std::ptr::copy_nonoverlapping(image.pixels.as_ptr(), bits, image.pixels.len());
        }
        let previous = unsafe { SelectObject(dc, bitmap) };
        let size = SIZE {
            cx: image.width,
            cy: image.height,
        };
        let origin = POINT { x: 0, y: 0 };
        let blend = blend(255);
        let ok = unsafe {
            UpdateLayeredWindow(
                self.hwnd, screen, &at, &size, dc, &origin, 0, &blend, ULW_ALPHA,
            )
        } != 0;
        unsafe {
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
            ReleaseDC(std::ptr::null_mut(), screen);
        }
        ok
    }

    /// Shows the window on top of the others without taking the focus.
    pub fn show(&self) {
        unsafe {
            ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    /// Changes only the strength of what is already shown, for the fade.
    pub fn set_alpha(&self, alpha: u8) {
        let blend = blend(alpha);
        unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &blend,
                ULW_ALPHA,
            )
        };
    }

    pub fn hide(&self) {
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    pub fn destroy(&mut self) {
        if !self.hwnd.is_null() {
            unsafe { DestroyWindow(self.hwnd) };
            self.hwnd = std::ptr::null_mut();
        }
    }
}

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

/// A top-down 32-bit DIB and a pointer to its pixels.
fn dib(dc: HDC, width: i32, height: i32) -> Option<(HGDIOBJ, *mut u32)> {
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..unsafe { std::mem::zeroed() }
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let bitmap = unsafe {
        CreateDIBSection(
            dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        )
    };
    if bitmap.is_null() || bits.is_null() {
        return None;
    }
    Some((bitmap, bits.cast()))
}

/// Premultiplied BGRA pixels, top row first.
pub struct Image {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
}

/// Segoe UI from the system, as the rest of WinCraft uses; when it is
/// missing, the bundled Selawik is loaded for this process alone. The pairs
/// are the GDI face names and weights: semibold first, then regular.
pub fn faces() -> &'static [(&'static str, u32); 2] {
    static FACES: OnceLock<[(&'static str, u32); 2]> = OnceLock::new();
    FACES.get_or_init(|| {
        let windows = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot"));
        let has_segoe = windows.is_some_and(|dir| {
            let fonts = std::path::PathBuf::from(dir).join("Fonts");
            fonts.join("segoeui.ttf").exists() && fonts.join("seguisb.ttf").exists()
        });
        if has_segoe {
            return [("Segoe UI Semibold", FW_SEMIBOLD), ("Segoe UI", FW_NORMAL)];
        }
        for font in [theme::SELAWIK_SEMIBOLD_TTF, theme::SELAWIK_REGULAR_TTF] {
            let mut count = 0u32;
            unsafe {
                AddFontMemResourceEx(
                    font.as_ptr().cast(),
                    font.len() as u32,
                    std::ptr::null(),
                    // The binding says *const, but GDI writes the count here,
                    // so the pointer keeps its right to write.
                    (&raw mut count).cast_const(),
                )
            };
        }
        log::info!("the on-screen panels use the bundled Selawik (Segoe UI not found)");
        [("Selawik Semibold", FW_SEMIBOLD), ("Selawik", FW_NORMAL)]
    })
}

pub fn font(face: &str, weight: u32, pixels: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -pixels,
            0,
            0,
            0,
            weight as i32,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            u32::from(OUT_TT_PRECIS),
            u32::from(CLIP_DEFAULT_PRECIS),
            u32::from(ANTIALIASED_QUALITY),
            u32::from(DEFAULT_PITCH),
            wide(face).as_ptr(),
        )
    }
}

/// One run of text in one colour, drawn with the font at index `font` of
/// the list given to `text_mask`.
pub struct Run {
    pub text: Vec<u16>,
    pub colour: Color32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub font: usize,
}

/// Every run drawn white on black with grey anti-aliasing; the result's
/// green channel is each pixel's text coverage.
pub fn text_mask(
    dc: HDC,
    width: i32,
    height: i32,
    runs: &[Run],
    fonts: &[HFONT],
) -> Option<Vec<u8>> {
    let (bitmap, bits) = dib(dc, width, height)?;
    let previous = unsafe { SelectObject(dc, bitmap) };
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00FF_FFFF);
    }
    for run in runs {
        unsafe {
            SelectObject(dc, fonts[run.font]);
            TextOutW(dc, run.x, run.y, run.text.as_ptr(), run.text.len() as i32);
        }
    }
    unsafe { GdiFlush() };
    let count = (width * height) as usize;
    let pixels = unsafe { std::slice::from_raw_parts(bits, count) };
    let mask = pixels.iter().map(|pixel| (pixel >> 8) as u8).collect();
    unsafe {
        SelectObject(dc, previous);
        DeleteObject(bitmap);
    }
    Some(mask)
}

pub fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// How much of pixel (x, y) lies inside a rectangle with rounded corners
/// inset by `inset` from the image edges, from its signed distance.
pub fn rounded_coverage(x: i32, y: i32, width: i32, height: i32, inset: f32, radius: f32) -> f32 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (half_w, half_h) = (width as f32 / 2.0 - inset, height as f32 / 2.0 - inset);
    let radius = radius.min(half_w).min(half_h).max(0.0);
    let qx = (px - width as f32 / 2.0).abs() - (half_w - radius);
    let qy = (py - height as f32 / 2.0).abs() - (half_h - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let distance = outside + qx.max(qy).min(0.0) - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

/// The panel's pixels: a translucent rounded fill inside a 1 px border, and
/// on top of it the text, opaque, in the colour `text` gives with the
/// coverage it gives. Premultiplied BGRA.
pub fn compose(
    width: i32,
    height: i32,
    radius: f32,
    border: f32,
    fill: Color32,
    edge: Color32,
    text: impl Fn(i32, i32) -> (f32, Color32),
) -> Vec<u32> {
    let rgb = |colour: Color32| {
        [
            f32::from(colour.r()),
            f32::from(colour.g()),
            f32::from(colour.b()),
        ]
    };
    let (fill, edge) = (rgb(fill), rgb(edge));
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let outer = rounded_coverage(x, y, width, height, 0.0, radius);
            let inner = rounded_coverage(x, y, width, height, border, radius - border);
            let fill_alpha = inner * FILL_ALPHA;
            let edge_alpha = (outer - inner).max(0.0) * BORDER_ALPHA;
            let mut alpha = fill_alpha + edge_alpha;
            let mut colour = [0.0f32; 3];
            for channel in 0..3 {
                colour[channel] = fill[channel] * fill_alpha + edge[channel] * edge_alpha;
            }
            let (coverage, tint) = text(x, y);
            let coverage = coverage * inner;
            if coverage > 0.0 {
                let tint = rgb(tint);
                for channel in 0..3 {
                    colour[channel] = tint[channel] * coverage + colour[channel] * (1.0 - coverage);
                }
                alpha = coverage + alpha * (1.0 - coverage);
            }
            let byte = |value: f32| value.round().clamp(0.0, 255.0) as u32;
            pixels.push(
                (byte(alpha * 255.0) << 24)
                    | (byte(colour[0]) << 16)
                    | (byte(colour[1]) << 8)
                    | byte(colour[2]),
            );
        }
    }
    pixels
}

unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILL: Color32 = Color32::from_rgb(0x2d, 0x2b, 0x2b);
    const EDGE: Color32 = Color32::from_rgb(0x44, 0x41, 0x41);
    const TEXT: Color32 = Color32::from_rgb(0xf3, 0xf2, 0xf2);

    fn alpha(pixel: u32) -> u32 {
        pixel >> 24
    }

    #[test]
    fn the_corners_are_clear_and_the_middle_is_translucent() {
        let pixels = compose(100, 60, 12.0, 1.0, FILL, EDGE, |_, _| (0.0, TEXT));
        assert_eq!(alpha(pixels[0]), 0, "top-left corner");
        assert_eq!(alpha(pixels[99]), 0, "top-right corner");
        let middle = pixels[30 * 100 + 50];
        assert_eq!(alpha(middle), (255.0 * FILL_ALPHA).round() as u32);
        // Premultiplied: each channel is at most the alpha.
        let red = (middle >> 16) & 0xFF;
        assert_eq!(red, (0x2d as f32 * FILL_ALPHA).round() as u32);
        // The straight top edge is the border.
        let edge = pixels[50];
        assert_eq!(alpha(edge), (255.0 * BORDER_ALPHA).round() as u32);
    }

    #[test]
    fn text_is_opaque_and_keeps_its_colour() {
        let pixels = compose(100, 60, 12.0, 1.0, FILL, EDGE, |x, y| {
            if (x, y) == (50, 30) {
                (1.0, TEXT)
            } else {
                (0.0, TEXT)
            }
        });
        let pixel = pixels[30 * 100 + 50];
        assert_eq!(alpha(pixel), 255);
        assert_eq!(pixel & 0x00FF_FFFF, 0x00f3_f2f2);
    }

    #[test]
    fn the_rounded_edge_is_anti_aliased() {
        // Along the diagonal of a 12 px corner the coverage rises from none
        // to full over a pixel or two, never in one hard step.
        let steps: Vec<f32> = (0..8)
            .map(|i| rounded_coverage(i, i, 100, 60, 0.0, 12.0))
            .collect();
        assert_eq!(steps[0], 0.0);
        assert_eq!(steps[7], 1.0);
        assert!(
            steps.iter().any(|value| *value > 0.0 && *value < 1.0),
            "{steps:?}"
        );
    }
}
