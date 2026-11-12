//! Where ClaudeDeck's window was and how big, kept in its own small file so
//! it comes back the same after a restart. Physical pixels, with the DPI the
//! window had, so a size kept on a monitor with another scale comes back
//! the same size to the eye.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    MonitorFromRect, MonitorFromWindow, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
};

use crate::core::{config, dpi_follow, window_place};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The window's DPI when it was kept. A file from an older WinCraft
    /// has none (0); the DPI of the monitor the place is on counts then.
    #[serde(default)]
    pub dpi: u32,
}

/// The corner Windows gives a minimised window.
const MINIMISED: i32 = -32000;
/// Smaller than this is no place worth keeping.
const SMALLEST: i32 = 60;

pub fn path() -> PathBuf {
    config::plugins_dir().join("claude_deck.place.json")
}

/// Whether a place can be kept and used again: not minimised, on a
/// monitor, and not shrunk to nothing.
pub fn worth_keeping(iconic: bool, on_monitor: bool, place: Place) -> bool {
    !iconic
        && on_monitor
        && place.x != MINIMISED
        && place.width >= SMALLEST
        && place.height >= SMALLEST
}

pub fn load(path: &Path) -> Option<Place> {
    let text = std::fs::read_to_string(path).ok()?;
    let place: Place = serde_json::from_str(&text).ok()?;
    (place.width >= SMALLEST && place.height >= SMALLEST).then_some(place)
}

pub fn save(path: &Path, place: Place) {
    let Ok(text) = serde_json::to_string(&place) else {
        return;
    };
    if let Err(err) = config::write_atomic(path, &text) {
        log::warn!("ClaudeDeck: could not save where its window was: {err}");
    }
}

/// Where the window is now, if that is worth keeping.
pub fn read(hwnd: isize) -> Option<Place> {
    let handle = hwnd as HWND;
    let [left, top, right, bottom] = window_place::rect(handle)?;
    let place = Place {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
        dpi: unsafe { GetDpiForWindow(handle) },
    };
    let iconic = unsafe { IsIconic(handle) } != 0;
    let on_monitor = !unsafe { MonitorFromWindow(handle, MONITOR_DEFAULTTONULL) }.is_null();
    worth_keeping(iconic, on_monitor, place).then_some(place)
}

/// The kept size for a window that now has `dpi_now`. A place kept
/// without a DPI counts as kept at `dpi_there`, the DPI of the monitor it
/// is on.
pub fn size_now(place: Place, dpi_there: u32, dpi_now: u32) -> (i32, i32) {
    let kept_at = if place.dpi == 0 { dpi_there } else { place.dpi };
    (
        dpi_follow::scale(place.width, kept_at, dpi_now),
        dpi_follow::scale(place.height, kept_at, dpi_now),
    )
}

/// The DPI of the monitor that holds most of `place`.
fn dpi_of_monitor(place: Place) -> u32 {
    let rect = RECT {
        left: place.x,
        top: place.y,
        right: place.x + place.width,
        bottom: place.y + place.height,
    };
    let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    let (mut dpi_x, mut dpi_y) = (0u32, 0u32);
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    dpi_x
}

/// Gives ClaudeDeck's own window its kept place, moved just enough to stay
/// on a monitor that is still there, and its kept size scaled to the DPI
/// it has there. It moves first: on a monitor with another scale Windows
/// scales the window as it arrives, and the size set after that stays.
pub fn apply(hwnd: isize, place: Place) {
    let handle = hwnd as HWND;
    let dpi_there = dpi_of_monitor(place);
    window_place::place_at(handle, (place.x, place.y));
    let (width, height) = size_now(place, dpi_there, unsafe { GetDpiForWindow(handle) });
    unsafe {
        SetWindowPos(
            handle,
            std::ptr::null_mut(),
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    window_place::place_at(handle, (place.x, place.y));
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEPT: Place = Place {
        x: 4397,
        y: 887,
        width: 630,
        height: 840,
        dpi: 168,
    };

    #[test]
    fn a_place_kept_at_175_percent_comes_back_the_same_size_to_the_eye() {
        assert_eq!(size_now(KEPT, 168, 168), (630, 840), "the same monitor");
        assert_eq!(size_now(KEPT, 168, 144), (540, 720), "150 percent");
        assert_eq!(size_now(KEPT, 168, 96), (360, 480), "100 percent");
    }

    #[test]
    fn a_place_kept_without_a_dpi_counts_as_kept_on_its_monitor() {
        let old = Place { dpi: 0, ..KEPT };
        assert_eq!(size_now(old, 168, 168), (630, 840));
        assert_eq!(size_now(old, 168, 144), (540, 720));
    }

    #[test]
    fn an_old_file_without_a_dpi_still_loads() {
        let place: Place =
            serde_json::from_str(r#"{"x":1,"y":2,"width":300,"height":400}"#).expect("loads");
        assert_eq!(place.dpi, 0);
    }
}
