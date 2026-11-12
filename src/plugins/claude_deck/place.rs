//! Where ClaudeDeck's window was and how big, kept in its own small file so
//! it comes back the same after a restart. Physical pixels.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONULL};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
};

use crate::core::{config, window_place};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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
    };
    let iconic = unsafe { IsIconic(handle) } != 0;
    let on_monitor = !unsafe { MonitorFromWindow(handle, MONITOR_DEFAULTTONULL) }.is_null();
    worth_keeping(iconic, on_monitor, place).then_some(place)
}

/// Gives ClaudeDeck's own window its kept size, then its place, moved just
/// enough to stay on a monitor that is still there.
pub fn apply(hwnd: isize, place: Place) {
    let handle = hwnd as HWND;
    unsafe {
        SetWindowPos(
            handle,
            std::ptr::null_mut(),
            0,
            0,
            place.width,
            place.height,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    window_place::place_at(handle, (place.x, place.y));
}
