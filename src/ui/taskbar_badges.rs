//! What is on screen while Win+Alt+A waits for a digit: a number badge just
//! above each button of the main taskbar, and one line at the bottom of the
//! main monitor that names WinCraft (Principle 1). They are click-through
//! layered windows, like the other on-screen panels, made and removed on
//! the host thread; nothing moves while they show.

use egui::Color32;
use windows_sys::Win32::Foundation::{POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetMonitorInfoW, GetTextExtentPoint32W,
    MonitorFromPoint, MonitorFromWindow, SelectObject, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::core::layered::{compose, faces, font, text_mask, utf16, Image, Layered, Run};
use crate::core::taskbar::Button;
use crate::core::taskbar_number::badge_text;
use crate::core::theme;

const CLASS_NAME: &str = "WinCraftTaskbarBadge";

/// Sizes in logical pixels, scaled by the monitor's DPI.
const BADGE_HEIGHT: f32 = 20.0;
const BADGE_PAD_X: f32 = 6.0;
const BADGE_TEXT: f32 = 12.0;
/// Between a badge and the top of its button.
const BADGE_GAP: f32 = 4.0;
const LINE_TEXT: f32 = 14.0;
const LINE_PAD_X: f32 = 16.0;
const LINE_PAD_Y: f32 = 10.0;
/// Between the line and the bottom of the work area, clear of the badges.
const LINE_LIFT: f32 = 40.0;

/// The badges and the line on screen; dropping them takes them away.
pub struct WaitHint {
    windows: Vec<Layered>,
}

impl WaitHint {
    /// Shows a badge over each of `buttons`, numbered from 1, and `line`
    /// at the bottom of the main monitor.
    pub fn show(buttons: &[Button], line: &str) -> Self {
        let tokens = theme::current_tokens();
        let mut windows = Vec::new();
        for (index, button) in buttons.iter().enumerate() {
            let centre = POINT {
                x: (button.rect[0] + button.rect[2]) / 2,
                y: (button.rect[1] + button.rect[3]) / 2,
            };
            let scale = scale_at(centre);
            let look = Look {
                text: BADGE_TEXT,
                weight: 0,
                pad_x: BADGE_PAD_X,
                height: BADGE_HEIGHT,
                radius: f32::from(tokens.radius_row),
                fill: tokens.window_bg,
                edge: tokens.accent,
                colour: tokens.text_primary,
            };
            let Some(image) = render(&badge_text(index + 1), &look, scale) else {
                continue;
            };
            let at = badge_place(
                button.rect,
                (image.width, image.height),
                (BADGE_GAP * scale).round() as i32,
            );
            windows.extend(present(&image, at));
        }
        let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        windows.extend(line_on(primary, line));
        Self { windows }
    }

    /// Only `line`, at the bottom of the monitor of the window in front:
    /// what shows while a chord waits for its second key.
    pub fn line_only(line: &str) -> Self {
        let front = unsafe { GetForegroundWindow() };
        let monitor = unsafe { MonitorFromWindow(front, MONITOR_DEFAULTTOPRIMARY) };
        Self {
            windows: line_on(monitor, line).into_iter().collect(),
        }
    }
}

/// One line of text in a panel at the bottom centre of `monitor`.
fn line_on(monitor: HMONITOR, line: &str) -> Option<Layered> {
    let tokens = theme::current_tokens();
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let work = info.rcWork;
    let scale = scale_at(POINT {
        x: (work.left + work.right) / 2,
        y: (work.top + work.bottom) / 2,
    });
    let look = Look {
        text: LINE_TEXT,
        weight: 1,
        pad_x: LINE_PAD_X,
        height: LINE_TEXT * 1.4 + 2.0 * LINE_PAD_Y,
        radius: f32::from(tokens.radius_window),
        fill: tokens.elevated_bg,
        edge: tokens.rim,
        colour: tokens.text_primary,
    };
    let image = render(line, &look, scale)?;
    let at = POINT {
        x: (work.left + work.right - image.width) / 2,
        y: work.bottom - image.height - (LINE_LIFT * scale).round() as i32,
    };
    present(&image, at)
}

impl WaitHint {}

impl Drop for WaitHint {
    fn drop(&mut self) {
        for window in &mut self.windows {
            window.destroy();
        }
    }
}

/// Where a badge `size` big goes: centred just above `button`.
pub fn badge_place(button: [i32; 4], size: (i32, i32), gap: i32) -> POINT {
    POINT {
        x: (button[0] + button[2]) / 2 - size.0 / 2,
        y: button[1] - gap - size.1,
    }
}

fn present(image: &Image, at: POINT) -> Option<Layered> {
    let window = Layered::create(CLASS_NAME)
        .map_err(|reason| log::warn!("a taskbar number badge could not be made: {reason}"))
        .ok()?;
    if !window.present(image, at) {
        return None;
    }
    window.show();
    Some(window)
}

fn scale_at(point: POINT) -> f32 {
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    dpi_x.max(96) as f32 / 96.0
}

/// One rounded panel with one line of text in its middle.
struct Look {
    text: f32,
    /// 0 for semibold, 1 for regular: the order of `faces`.
    weight: usize,
    pad_x: f32,
    height: f32,
    radius: f32,
    fill: Color32,
    edge: Color32,
    colour: Color32,
}

fn render(text: &str, look: &Look, scale: f32) -> Option<Image> {
    let px = |logical: f32| (logical * scale).round() as i32;
    let (face, weight) = faces()[look.weight];
    let font = font(face, weight, px(look.text));
    let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
    let text = utf16(text);
    let mut size = SIZE { cx: 0, cy: 0 };
    unsafe {
        SelectObject(dc, font);
        GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut size);
    }
    let height = px(look.height);
    let width = (size.cx + 2 * px(look.pad_x)).max(height);
    let run = Run {
        text,
        colour: look.colour,
        x: (width - size.cx) / 2,
        y: (height - size.cy) / 2,
        width: size.cx,
        height: size.cy,
        font: 0,
    };
    let mask = text_mask(dc, width, height, std::slice::from_ref(&run), &[font]);
    unsafe {
        DeleteDC(dc);
        DeleteObject(font);
    }
    let mask = mask?;
    let pixels = compose(
        width,
        height,
        look.radius * scale,
        scale.round().max(1.0),
        look.fill,
        look.edge,
        |x, y| {
            let coverage = f32::from(mask[(y * width + x) as usize]) / 255.0;
            (coverage, look.colour)
        },
    );
    Some(Image {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_badge_sits_centred_just_above_its_button() {
        let at = badge_place([1000, 2040, 1048, 2088], (30, 30), 6);
        assert_eq!((at.x, at.y), (1009, 2004));
    }
}
