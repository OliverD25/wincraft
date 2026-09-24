use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};

use egui::{ColorImage, TextureHandle, TextureOptions};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetClassLongPtrW, GetIconInfo, SendMessageTimeoutW, GCLP_HICON, GCLP_HICONSM,
    HICON, ICONINFO, ICON_BIG, SMTO_ABORTIFHUNG, WM_GETICON,
};

use crate::core::wide;
use crate::search::IconRef;

/// An icon as straight-alpha RGBA, square.
struct Pixels {
    side: usize,
    rgba: Vec<u8>,
}

/// Shell icons, turned into egui textures once per path. Asking the shell
/// can take several milliseconds per file, and far longer for a file on a
/// sleeping network drive, so a worker thread does it and the palette draws
/// the row without its picture until the answer arrives.
pub struct IconCache {
    textures: HashMap<IconRef, Option<TextureHandle>>,
    requests: Sender<IconRef>,
    done: Receiver<(IconRef, Option<Pixels>)>,
}

impl IconCache {
    pub fn new(ctx: &egui::Context) -> Self {
        let (requests, wanted) = channel::<IconRef>();
        let (finished, done) = channel();
        let repaint = ctx.clone();
        let started = std::thread::Builder::new()
            .name("wincraft-icons".to_string())
            .spawn(move || {
                // SHGetFileInfo reads shortcuts through COM.
                unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
                while let Ok(icon) = wanted.recv() {
                    let pixels = load(&icon);
                    if finished.send((icon, pixels)).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            });
        if let Err(err) = started {
            log::warn!("could not start the icon thread: {err}");
        }
        Self {
            textures: HashMap::new(),
            requests,
            done,
        }
    }

    /// Window handles are reused by Windows, so their pictures are asked for
    /// again each time the palette opens.
    pub fn forget_windows(&mut self) {
        self.textures
            .retain(|icon, _| !matches!(icon, IconRef::Window { .. }));
    }

    pub fn get(&mut self, ctx: &egui::Context, icon: &IconRef) -> Option<TextureHandle> {
        while let Ok((key, pixels)) = self.done.try_recv() {
            let texture = pixels.map(|pixels| {
                let image =
                    ColorImage::from_rgba_unmultiplied([pixels.side, pixels.side], &pixels.rgba);
                let options = TextureOptions {
                    mipmap_mode: Some(egui::TextureFilter::Linear),
                    ..TextureOptions::LINEAR
                };
                ctx.load_texture("palette-icon", image, options)
            });
            self.textures.insert(key, texture);
        }
        if !self.textures.contains_key(icon) {
            self.textures.insert(icon.clone(), None);
            let _ = self.requests.send(icon.clone());
            return None;
        }
        self.textures.get(icon).cloned().flatten()
    }
}

fn load(icon: &IconRef) -> Option<Pixels> {
    match icon {
        IconRef::Path(path) => shell_icon(path),
        IconRef::Window { hwnd, exe } => window_icon(*hwnd as HWND).or_else(|| shell_icon(exe)),
        IconRef::None | IconRef::Glyph(_) => None,
    }
}

/// The 32 px icon, drawn at 16 px: that is 1:1 at 200 % and still sharp at
/// 100 % thanks to mipmaps, where the 16 px icon would blur at 150 %.
fn shell_icon(path: &Path) -> Option<Pixels> {
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
    let found = unsafe {
        SHGetFileInfoW(
            wide(&path.to_string_lossy()).as_ptr(),
            0 as FILE_FLAGS_AND_ATTRIBUTES,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if found == 0 || info.hIcon.is_null() {
        return None;
    }
    let pixels = icon_pixels(info.hIcon);
    unsafe { DestroyIcon(info.hIcon) };
    pixels
}

/// The icon the window shows in its title bar, else its class icon. Both
/// belong to the other program and must not be destroyed here. The message
/// goes through a timeout so a hung window cannot stall the icon thread.
fn window_icon(hwnd: HWND) -> Option<Pixels> {
    let mut answer: usize = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            ICON_BIG as WPARAM,
            0 as LPARAM,
            SMTO_ABORTIFHUNG,
            100,
            &mut answer,
        )
    };
    let mut icon = if sent != 0 { answer } else { 0 };
    if icon == 0 {
        icon = unsafe { GetClassLongPtrW(hwnd, GCLP_HICON) };
    }
    if icon == 0 {
        icon = unsafe { GetClassLongPtrW(hwnd, GCLP_HICONSM) };
    }
    if icon == 0 {
        return None;
    }
    icon_pixels(icon as HICON)
}

fn icon_pixels(icon: HICON) -> Option<Pixels> {
    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    if unsafe { GetIconInfo(icon, &mut info) } == 0 {
        return None;
    }
    let pixels = if info.hbmColor.is_null() {
        None
    } else {
        let colour = bitmap_bgra(info.hbmColor);
        let mask = bitmap_bgra(info.hbmMask);
        colour.and_then(|(side, bgra)| {
            let mask = mask
                .filter(|(mask_side, _)| *mask_side == side)
                .map(|(_, bits)| bits);
            Some(Pixels {
                side,
                rgba: to_rgba(&bgra, mask.as_deref()),
            })
        })
    };
    unsafe {
        if !info.hbmColor.is_null() {
            DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_null() {
            DeleteObject(info.hbmMask);
        }
    }
    pixels
}

/// The bitmap's pixels as 32-bit top-down BGRA, for a square bitmap only.
fn bitmap_bgra(bitmap: HBITMAP) -> Option<(usize, Vec<u8>)> {
    if bitmap.is_null() {
        return None;
    }
    let mut header: BITMAP = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<BITMAP>() as i32;
    if unsafe { GetObjectW(bitmap, size, &mut header as *mut BITMAP as *mut _) } == 0 {
        return None;
    }
    let side = header.bmWidth;
    if side <= 0 || side > 256 || header.bmHeight < side {
        return None;
    }
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: side,
        biHeight: -side,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..unsafe { std::mem::zeroed() }
    };
    let mut bits = vec![0u8; side as usize * side as usize * 4];
    let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            side as u32,
            bits.as_mut_ptr() as *mut _,
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    unsafe { DeleteDC(dc) };
    (lines == side).then_some((side as usize, bits))
}

/// Icons without an alpha channel mark their transparent pixels in the mask
/// instead: a white mask pixel is see-through.
fn to_rgba(bgra: &[u8], mask: Option<&[u8]>) -> Vec<u8> {
    let has_alpha = bgra.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for (index, pixel) in bgra.chunks_exact(4).enumerate() {
        let alpha = if has_alpha {
            pixel[3]
        } else {
            match mask.and_then(|mask| mask.get(index * 4)) {
                Some(&shown) if shown != 0 => 0,
                _ => 255,
            }
        };
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blue_green_red_becomes_red_green_blue() {
        let bgra = [10, 20, 30, 200];
        assert_eq!(to_rgba(&bgra, None), vec![30, 20, 10, 200]);
    }

    #[test]
    fn an_icon_without_alpha_takes_it_from_the_mask() {
        let bgra = [1, 2, 3, 0, 4, 5, 6, 0];
        let mask = [0, 0, 0, 0, 255, 255, 255, 0];
        assert_eq!(to_rgba(&bgra, Some(&mask)), vec![3, 2, 1, 255, 6, 5, 4, 0]);
    }
}
