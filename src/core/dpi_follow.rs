//! Keeps a frameless egui window steady when it moves to a monitor with
//! another scale.
//!
//! winit answers `WM_DPICHANGED` with its own size and, while the window is
//! dragged, moves it so the mouse keeps its place along the window's width.
//! For a window dragged by its own header that move can leave half of the
//! bigger window on the old monitor. Windows then gives it the old scale
//! back, winit shrinks it, and the window flips between the two scales on
//! every mouse move: it lags behind the mouse and ends at a wrong size.
//!
//! This helper lets winit handle the message, so egui learns the new scale,
//! but the move winit makes inside it goes to the corner Windows suggested,
//! with the size scaled from the window's own. Back on a monitor with the
//! scale it had before, the window gets its exact old size back.

use std::cell::Cell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_DPICHANGED, WM_NCDESTROY,
    WM_WINDOWPOSCHANGING,
};

const SUBCLASS_ID: usize = 0x5743_4450;

/// A window size in physical pixels at a DPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtDpi {
    pub dpi: u32,
    pub width: i32,
    pub height: i32,
}

/// One length scaled from one DPI to another, rounded to the nearest pixel.
/// An unknown DPI (0) keeps the length.
pub fn scale(length: i32, from_dpi: u32, to_dpi: u32) -> i32 {
    if from_dpi == 0 || to_dpi == 0 {
        return length;
    }
    (f64::from(length) * f64::from(to_dpi) / f64::from(from_dpi)).round() as i32
}

/// The size a window of `now` gets at `to_dpi`. When it had `before` at
/// that DPI and was not resized since (scaling `before` gives `now`
/// again), it gets exactly `before` back, so a trip to another monitor and
/// back never loses a pixel to rounding.
pub fn size_at(now: AtDpi, to_dpi: u32, before: Option<AtDpi>) -> (i32, i32) {
    if let Some(before) = before.filter(|before| before.dpi == to_dpi) {
        let same = scale(before.width, before.dpi, now.dpi) == now.width
            && scale(before.height, before.dpi, now.dpi) == now.height;
        if same {
            return (before.width, before.height);
        }
    }
    (
        scale(now.width, now.dpi, to_dpi),
        scale(now.height, now.dpi, to_dpi),
    )
}

/// What the move made inside `WM_DPICHANGED` becomes: Windows' suggested
/// top-left corner (left, top of `suggested`) and `size`. A change that
/// does not both move and size the window is left as it is.
pub fn steady(flags: u32, suggested: [i32; 4], size: (i32, i32)) -> Option<[i32; 4]> {
    if flags & (SWP_NOMOVE | SWP_NOSIZE) != 0 {
        return None;
    }
    Some([suggested[0], suggested[1], size.0, size.1])
}

/// The scale change one window is going through.
#[derive(Clone, Copy)]
struct Changing {
    hwnd: isize,
    suggested: [i32; 4],
    size: (i32, i32),
}

thread_local! {
    /// The scale change being handled on this thread, if any.
    static CHANGING: Cell<Option<Changing>> = const { Cell::new(None) };
}

/// Per window: its DPI now (inside `WM_DPICHANGED` Windows already
/// reports the new one), and its size at the DPI it had before the last
/// change.
struct Memory {
    dpi: Cell<u32>,
    before: Cell<Option<AtDpi>>,
}

/// Starts keeping `hwnd` steady across monitor scales. Call it on the
/// thread that owns the window; a second call for the same window does
/// nothing.
pub fn follow(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let handle = hwnd as HWND;
    let mut data = 0usize;
    if unsafe { GetWindowSubclass(handle, Some(proc), SUBCLASS_ID, &mut data) } != 0 {
        return;
    }
    let memory = Box::into_raw(Box::new(Memory {
        dpi: Cell::new(unsafe { GetDpiForWindow(handle) }),
        before: Cell::new(None),
    }));
    if unsafe { SetWindowSubclass(handle, Some(proc), SUBCLASS_ID, memory as usize) } == 0 {
        log::warn!("could not keep a window steady across monitor scales");
        drop(unsafe { Box::from_raw(memory) });
    }
}

fn size_of(hwnd: HWND) -> Option<(i32, i32)> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    (unsafe { GetWindowRect(hwnd, &mut rect) } != 0)
        .then_some((rect.right - rect.left, rect.bottom - rect.top))
}

/// Notes the scale change and lets winit handle it.
///
/// # Safety
/// `lparam` points at the suggested RECT, as `WM_DPICHANGED` promises.
unsafe fn dpi_changed(hwnd: HWND, wparam: WPARAM, lparam: LPARAM, memory: &Memory) -> LRESULT {
    let rect = unsafe { *(lparam as *const RECT) };
    let new_dpi = (wparam & 0xffff) as u32;
    let old_dpi = memory.dpi.replace(new_dpi);
    let Some((width, height)) = size_of(hwnd) else {
        return unsafe { DefSubclassProc(hwnd, WM_DPICHANGED, wparam, lparam) };
    };
    let now = AtDpi {
        dpi: old_dpi,
        width,
        height,
    };
    let size = size_at(now, new_dpi, memory.before.get());
    memory.before.set(Some(now));
    let changing = Changing {
        hwnd: hwnd as isize,
        suggested: [rect.left, rect.top, rect.right, rect.bottom],
        size,
    };
    let outer = CHANGING.with(|cell| cell.replace(Some(changing)));
    let result = unsafe { DefSubclassProc(hwnd, WM_DPICHANGED, wparam, lparam) };
    CHANGING.with(|cell| cell.set(outer));
    result
}

unsafe extern "system" fn proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let memory = data as *const Memory;
    match message {
        WM_DPICHANGED if lparam != 0 && !memory.is_null() => unsafe {
            dpi_changed(hwnd, wparam, lparam, &*memory)
        },
        WM_WINDOWPOSCHANGING if lparam != 0 => {
            let changing = CHANGING
                .with(Cell::get)
                .filter(|changing| changing.hwnd == hwnd as isize);
            let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
            if let Some(changing) = changing {
                let pos = unsafe { &mut *(lparam as *mut WINDOWPOS) };
                if let Some([x, y, cx, cy]) = steady(pos.flags, changing.suggested, changing.size) {
                    pos.x = x;
                    pos.y = y;
                    pos.cx = cx;
                    pos.cy = cy;
                }
            }
            result
        }
        WM_NCDESTROY => {
            unsafe { RemoveWindowSubclass(hwnd, Some(proc), SUBCLASS_ID) };
            if !memory.is_null() {
                drop(unsafe { Box::from_raw(memory as *mut Memory) });
            }
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT_150: AtDpi = AtDpi {
        dpi: 144,
        width: 540,
        height: 720,
    };

    #[test]
    fn a_size_scales_by_the_two_dpis() {
        assert_eq!(size_at(AT_150, 168, None), (630, 840));
        assert_eq!(size_at(AT_150, 96, None), (360, 480));
        assert_eq!(scale(541, 144, 96), 361);
        assert_eq!(scale(500, 0, 96), 500, "an unknown DPI keeps the size");
    }

    #[test]
    fn back_at_the_first_scale_the_window_has_exactly_its_old_size() {
        // 541 px at 150% is 360.7 points: rounding alone gives 542 back.
        let first = AtDpi {
            dpi: 144,
            width: 541,
            height: 721,
        };
        let (width, height) = size_at(first, 96, None);
        let there = AtDpi {
            dpi: 96,
            width,
            height,
        };
        assert_eq!(scale(width, 96, 144), 542, "plain rounding drifts");
        assert_eq!(size_at(there, 144, Some(first)), (541, 721));
    }

    #[test]
    fn a_window_resized_on_the_other_monitor_scales_its_new_size() {
        let there = AtDpi {
            dpi: 168,
            width: 700,
            height: 840,
        };
        assert_eq!(size_at(there, 144, Some(AT_150)), (600, 720));
    }

    #[test]
    fn the_move_inside_the_scale_change_goes_where_windows_suggested() {
        // Seen on 2026-11-12: winit moved the window from 3607 to 3525, half
        // of it went back on the 150% monitor and the scale flipped back.
        let suggested = [3607, 820, 4237, 1660];
        assert_eq!(
            steady(0x14, suggested, (630, 840)),
            Some([3607, 820, 630, 840])
        );
        assert_eq!(steady(SWP_NOMOVE | 0x14, suggested, (630, 840)), None);
        assert_eq!(steady(SWP_NOSIZE | 0x14, suggested, (630, 840)), None);
    }
}
