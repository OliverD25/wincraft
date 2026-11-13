//! A plugin's own tray icon, beside WinCraft's: its picture with a small
//! count badge, drawn here from the plugin's RGBA picture, and turned into
//! an icon Windows can show. The host adds, changes and removes the icon
//! (`host::set_own_tray`); this file only makes the pictures.

use std::sync::Arc;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, GetSystemMetrics, HICON, LR_DEFAULTCOLOR, SM_CXSMICON,
};

/// What a plugin's own tray icon shows.
#[derive(Clone, Debug)]
pub struct OwnTray {
    /// The picture, square, in RGBA; it is scaled to the tray's size.
    pub icon: Arc<egui::IconData>,
    /// The tooltip. It should name the plugin, so the user knows who it is.
    pub tip: String,
    /// The number on the badge; 0 shows no badge.
    pub badge: usize,
}

impl PartialEq for OwnTray {
    fn eq(&self, other: &Self) -> bool {
        (Arc::ptr_eq(&self.icon, &other.icon) || self.icon == other.icon)
            && self.tip == other.tip
            && badge_text(self.badge) == badge_text(other.badge)
    }
}

/// The words on the badge: the number, "9+" above 9, None for 0.
pub fn badge_text(count: usize) -> Option<String> {
    match count {
        0 => None,
        1..=9 => Some(count.to_string()),
        _ => Some("9+".to_string()),
    }
}

/// The badge's colour: Windows 11's red for things that need you.
const BADGE: [u8; 3] = [196, 43, 28];
const BADGE_TEXT: [u8; 3] = [255, 255, 255];

/// Digits and "+" in 3 x 5 pixels, one row per entry, the top bit left.
fn glyph(ch: char) -> Option<[u8; 5]> {
    Some(match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        _ => return None,
    })
}

/// `icon` scaled to `size` x `size` by averaging the pixels each new pixel
/// covers, in premultiplied colour so see-through edges do not go dark.
pub fn scaled(icon: &egui::IconData, size: u32) -> Vec<u8> {
    let (width, height) = (icon.width.max(1), icon.height.max(1));
    let mut out = vec![0u8; (size * size * 4) as usize];
    if icon.rgba.len() < (width * height * 4) as usize || size == 0 {
        return out;
    }
    let (step_x, step_y) = (width as f32 / size as f32, height as f32 / size as f32);
    for y in 0..size {
        for x in 0..size {
            let (x0, x1) = (x as f32 * step_x, (x + 1) as f32 * step_x);
            let (y0, y1) = (y as f32 * step_y, (y + 1) as f32 * step_y);
            let mut sum = [0f32; 4];
            let mut area = 0f32;
            for sy in y0.floor() as u32..(y1.ceil() as u32).min(height) {
                let cover_y = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                for sx in x0.floor() as u32..(x1.ceil() as u32).min(width) {
                    let cover = cover_y * (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                    let at = ((sy * width + sx) * 4) as usize;
                    let alpha = f32::from(icon.rgba[at + 3]) / 255.0;
                    for (part, channel) in sum.iter_mut().zip(0..3) {
                        *part += f32::from(icon.rgba[at + channel]) * alpha * cover;
                    }
                    sum[3] += alpha * cover;
                    area += cover;
                }
            }
            let at = ((y * size + x) * 4) as usize;
            if area > 0.0 && sum[3] > 0.0 {
                for channel in 0..3 {
                    out[at + channel] = (sum[channel] / sum[3]).round().clamp(0.0, 255.0) as u8;
                }
                out[at + 3] = (sum[3] / area * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Puts `colour` over the pixel at `at` with coverage `cover` (0 to 1).
fn blend(pixels: &mut [u8], at: usize, colour: [u8; 3], cover: f32) {
    if cover <= 0.0 {
        return;
    }
    let under = f32::from(pixels[at + 3]) / 255.0;
    let alpha = cover + under * (1.0 - cover);
    for (channel, value) in colour.iter().enumerate() {
        let mixed =
            f32::from(*value) * cover + f32::from(pixels[at + channel]) * under * (1.0 - cover);
        pixels[at + channel] = (mixed / alpha).round().clamp(0.0, 255.0) as u8;
    }
    pixels[at + 3] = (alpha * 255.0).round() as u8;
}

/// Draws the badge with `text` on the bottom right of a `size` picture: a
/// red circle with white digits, 3 x 5 pixels each, doubled from 32 px up.
pub fn draw_badge(pixels: &mut [u8], size: u32, text: &str) {
    let scale = (size / 16).clamp(1, 2);
    let glyphs: Vec<[u8; 5]> = text.chars().filter_map(glyph).collect();
    if glyphs.is_empty() || pixels.len() < (size * size * 4) as usize {
        return;
    }
    let text_width = (glyphs.len() as u32 * 4 - 1) * scale;
    let diameter = (size as f32 * 0.7).max(text_width as f32 + 2.0 * scale as f32);
    let radius = diameter / 2.0;
    let (cx, cy) = (size as f32 - radius, size as f32 - radius);
    for y in 0..size {
        for x in 0..size {
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let cover = (radius - distance + 0.5).clamp(0.0, 1.0);
            blend(pixels, ((y * size + x) * 4) as usize, BADGE, cover);
        }
    }
    let left = (cx - text_width as f32 / 2.0).round() as i64;
    let top = (cy - (5 * scale) as f32 / 2.0).round() as i64;
    for (index, rows) in glyphs.iter().enumerate() {
        let start = left + i64::from(index as u32 * 4 * scale);
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3u32 {
                if bits & (0b100 >> column) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = start + i64::from(column * scale + dx);
                        let y = top + i64::from(row as u32 * scale + dy);
                        if (0..i64::from(size)).contains(&x) && (0..i64::from(size)).contains(&y) {
                            let at = ((y as u32 * size + x as u32) * 4) as usize;
                            blend(pixels, at, BADGE_TEXT, 1.0);
                        }
                    }
                }
            }
        }
    }
}

/// The picture of the icon at `size`, with the badge when there is one.
pub fn picture(icon: &egui::IconData, size: u32, badge: usize) -> Vec<u8> {
    let mut pixels = scaled(icon, size);
    if let Some(text) = badge_text(badge) {
        draw_badge(&mut pixels, size, &text);
    }
    pixels
}

/// The bytes Windows makes an icon from: a 32-bit bitmap header, the
/// pixels bottom row first in BGRA, then an empty mask (the alpha decides).
pub fn icon_resource(rgba: &[u8], size: u32) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let pixels = size * size * 4;
    let mut out = Vec::with_capacity((40 + pixels + mask_row * size) as usize);
    for value in [40u32, size, size * 2] {
        out.extend(value.to_le_bytes());
    }
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((pixels + mask_row * size).to_le_bytes());
    out.extend([0u8; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let at = ((y * size + x) * 4) as usize;
            let pixel = rgba.get(at..at + 4).unwrap_or(&[0, 0, 0, 0]);
            out.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    out.resize(out.len() + (mask_row * size) as usize, 0);
    out
}

/// The size of a tray icon now, in pixels.
pub fn tray_size() -> u32 {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    u32::try_from(size).ok().filter(|s| *s > 0).unwrap_or(16)
}

/// A Windows icon of what `tray` shows, at the tray's size. Free it with
/// `free` once the tray no longer shows it.
pub fn make_icon(tray: &OwnTray) -> HICON {
    let size = tray_size();
    let bytes = icon_resource(&picture(&tray.icon, size, tray.badge), size);
    unsafe {
        CreateIconFromResourceEx(
            bytes.as_ptr(),
            bytes.len() as u32,
            1,
            0x0003_0000,
            size as i32,
            size as i32,
            LR_DEFAULTCOLOR,
        )
    }
}

pub fn free(icon: HICON) {
    if !icon.is_null() {
        unsafe { DestroyIcon(icon) };
    }
}

#[cfg(test)]
mod tests;
