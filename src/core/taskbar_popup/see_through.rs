//! Making Explorer's taskbar popup fully transparent ("see-through") while
//! WindowDeck's hover strip watches the taskbar, so a show that comes before
//! our hide is never seen.
//!
//! The popup gets `WS_EX_LAYERED` and alpha 0. What was there before is kept
//! in a window property on the popup itself (`MARKER`), so `restore` puts
//! back exactly that, and `repair` can undo it after a crash, the next time
//! WinCraft starts. A popup that already draws itself with
//! `UpdateLayeredWindow` or a colour key is never touched.

use windows_sys::Win32::Foundation::{GetLastError, SetLastError, HWND};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetLayeredWindowAttributes, GetPropW, GetWindowLongPtrW, RemovePropW,
    SetLayeredWindowAttributes, SetPropW, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, LWA_COLORKEY,
    WS_EX_LAYERED,
};

use crate::core::wide;

/// The window property that says WinCraft made this popup see-through, and
/// holds what to put back.
pub const MARKER: &str = "WinCraft.SeeThrough";
const TAG: usize = 0x5743_0000;
const ADDED: usize = 0x100;

/// What the popup had before it was made see-through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Saved {
    /// WinCraft added `WS_EX_LAYERED`; restore takes it off again.
    pub added_layered: bool,
    /// The alpha it had (255 when it was not layered).
    pub alpha: u8,
}

impl Saved {
    pub fn encode(self) -> usize {
        TAG | if self.added_layered { ADDED } else { 0 } | usize::from(self.alpha)
    }

    pub fn decode(value: usize) -> Option<Self> {
        (value & !0xFFFF == TAG).then_some(Self {
            added_layered: value & ADDED != 0,
            alpha: (value & 0xFF) as u8,
        })
    }
}

/// The popup's layered state as read now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layered {
    pub layered: bool,
    /// `None` when not layered, or when its attributes cannot be read (it
    /// uses `UpdateLayeredWindow`).
    pub alpha: Option<u8>,
    /// A colour key is set.
    pub color_key: bool,
}

/// What `apply` may do with a popup in this state, before any change.
pub fn plan(now: Layered) -> Result<Saved, &'static str> {
    if !now.layered {
        return Ok(Saved {
            added_layered: true,
            alpha: 255,
        });
    }
    match now.alpha {
        None => Err("it draws itself with UpdateLayeredWindow"),
        Some(_) if now.color_key => Err("it uses a colour key"),
        Some(alpha) => Ok(Saved {
            added_layered: false,
            alpha,
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repair {
    Nothing,
    /// Put the saved state back and take the marker off.
    Restore(Saved),
    /// The marker is there but the popup is not see-through any more:
    /// only the marker goes.
    DropMarker,
}

/// What to do at start with a popup a crashed WinCraft may have left
/// see-through: only a popup with our marker, layered and at alpha 0, is
/// restored.
pub fn repair_plan(now: Layered, marker: Option<Saved>) -> Repair {
    match marker {
        None => Repair::Nothing,
        Some(saved) if now.layered && now.alpha == Some(0) => Repair::Restore(saved),
        Some(_) => Repair::DropMarker,
    }
}

pub fn read(hwnd: HWND) -> Layered {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let layered = style & WS_EX_LAYERED != 0;
    if !layered {
        return Layered {
            layered,
            alpha: None,
            color_key: false,
        };
    }
    let (mut key, mut alpha, mut flags) = (0u32, 0u8, 0u32);
    let ok = unsafe { GetLayeredWindowAttributes(hwnd, &mut key, &mut alpha, &mut flags) } != 0;
    Layered {
        layered,
        alpha: ok.then_some(if flags & LWA_ALPHA != 0 { alpha } else { 255 }),
        color_key: ok && flags & LWA_COLORKEY != 0,
    }
}

pub fn marker(hwnd: HWND) -> Option<Saved> {
    let value = unsafe { GetPropW(hwnd, wide(MARKER).as_ptr()) } as usize;
    Saved::decode(value)
}

/// Makes the popup see-through. `Ok(false)` when it already was. On an
/// error nothing is left changed.
pub fn apply(hwnd: HWND) -> Result<bool, String> {
    if marker(hwnd).is_some() {
        return Ok(false);
    }
    let saved = plan(read(hwnd)).map_err(str::to_string)?;
    let name = wide(MARKER);
    // The marker goes first, so a crash after the style change can be
    // repaired at the next start.
    let ok = unsafe { SetPropW(hwnd, name.as_ptr(), saved.encode() as _) } != 0;
    call("SetPropW WinCraft.SeeThrough", hwnd, i64::from(ok), !ok);
    if !ok {
        return Err(format!("SetPropW failed ({})", unsafe { GetLastError() }));
    }
    if saved.added_layered {
        if let Err(error) = set_layered(hwnd, true) {
            unsafe { RemovePropW(hwnd, name.as_ptr()) };
            return Err(error);
        }
    }
    let ok = unsafe { SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA) } != 0;
    call(
        "SetLayeredWindowAttributes alpha 0 (taskbar preview)",
        hwnd,
        i64::from(ok),
        !ok,
    );
    if !ok || read(hwnd).alpha != Some(0) {
        let error = unsafe { GetLastError() };
        restore(hwnd);
        return Err(format!("alpha 0 did not take ({error})"));
    }
    Ok(true)
}

/// Puts back what `apply` changed. False when the popup carries no marker.
pub fn restore(hwnd: HWND) -> bool {
    let Some(saved) = marker(hwnd) else {
        return false;
    };
    put_back(hwnd, saved);
    true
}

/// At start: undoes what a WinCraft that crashed left on this popup.
pub fn repair(hwnd: HWND) -> Repair {
    let action = repair_plan(read(hwnd), marker(hwnd));
    match action {
        Repair::Nothing => {}
        Repair::Restore(saved) => put_back(hwnd, saved),
        Repair::DropMarker => unsafe {
            RemovePropW(hwnd, wide(MARKER).as_ptr());
        },
    }
    action
}

fn put_back(hwnd: HWND, saved: Saved) {
    let ok = unsafe { SetLayeredWindowAttributes(hwnd, 0, saved.alpha, LWA_ALPHA) } != 0;
    call(
        "SetLayeredWindowAttributes restore (taskbar preview)",
        hwnd,
        i64::from(saved.alpha),
        !ok,
    );
    if saved.added_layered {
        if let Err(error) = set_layered(hwnd, false) {
            log::warn!("taskbar preview: {error}");
        }
    }
    unsafe { RemovePropW(hwnd, wide(MARKER).as_ptr()) };
}

fn set_layered(hwnd: HWND, on: bool) -> Result<(), String> {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let wanted = if on {
        style | WS_EX_LAYERED as isize
    } else {
        style & !(WS_EX_LAYERED as isize)
    };
    unsafe { SetLastError(0) };
    let before = unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted) };
    let error = unsafe { GetLastError() };
    let failed = before == 0 && error != 0;
    call(
        if on {
            "SetWindowLongPtrW add WS_EX_LAYERED (taskbar preview)"
        } else {
            "SetWindowLongPtrW remove WS_EX_LAYERED (taskbar preview)"
        },
        hwnd,
        before as i64,
        failed,
    );
    if failed {
        return Err(format!("SetWindowLongPtrW failed ({error})"));
    }
    Ok(())
}

fn call(what: &str, hwnd: HWND, result: i64, failed: bool) {
    crate::core::detailed::call(what, hwnd as isize, result, failed);
}

#[cfg(test)]
mod tests;
