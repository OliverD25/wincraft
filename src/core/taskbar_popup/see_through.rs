//! Making Explorer's taskbar popup fully transparent ("see-through") while
//! WindowDeck's hover strip watches the taskbar, so a show that comes before
//! our hide is never seen.
//!
//! The popup gets `WS_EX_LAYERED` and alpha 0. What was there before is kept
//! in a window property on the popup itself (`MARKER`), so `restore` puts
//! back exactly that, and `repair` can undo it after a crash, the next time
//! WinCraft starts. A popup that already draws itself with
//! `UpdateLayeredWindow` or a colour key is never touched.
//!
//! The marker also holds the process id of the WinCraft that made the
//! change. A second WinCraft (a test copy next to the real one) never
//! restores or repairs a popup that a running WinCraft holds, so it cannot
//! undo what the other one did (Principle 1).

use windows_sys::Win32::Foundation::{GetLastError, SetLastError, HWND};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetLayeredWindowAttributes, GetPropW, GetWindowLongPtrW, RemovePropW,
    SetLayeredWindowAttributes, SetPropW, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, LWA_COLORKEY,
    WS_EX_LAYERED,
};

use crate::core::wide;

/// The window property that says WinCraft made this popup see-through, and
/// holds what to put back and which WinCraft did it.
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
    /// `owner` is the process id of the WinCraft that made the change; it
    /// goes in the upper 32 bits.
    pub fn encode(self, owner: u32) -> usize {
        ((u64::from(owner) << 32) as usize)
            | TAG
            | if self.added_layered { ADDED } else { 0 }
            | usize::from(self.alpha)
    }

    pub fn decode(value: usize) -> Option<Self> {
        (value & 0xFFFF_0000 == TAG).then_some(Self {
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

/// The process id of the WinCraft that wrote the marker `value`; 0 for a
/// marker of an older WinCraft, which did not record it.
pub fn owner(value: usize) -> u32 {
    ((value as u64) >> 32) as u32
}

/// A marker of `owner` belongs to another WinCraft that still runs, so this
/// one (`me`) must leave the popup alone.
pub fn held_elsewhere(owner: u32, me: u32, is_running_wincraft: impl Fn(u32) -> bool) -> bool {
    owner != 0 && owner != me && is_running_wincraft(owner)
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
/// restored. A marker of another WinCraft that still runs is left alone.
pub fn repair_plan(now: Layered, marker: Option<Saved>, held_elsewhere: bool) -> Repair {
    match marker {
        None => Repair::Nothing,
        Some(_) if held_elsewhere => Repair::Nothing,
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

fn marker_value(hwnd: HWND) -> usize {
    unsafe { GetPropW(hwnd, wide(MARKER).as_ptr()) as usize }
}

pub fn marker(hwnd: HWND) -> Option<Saved> {
    Saved::decode(marker_value(hwnd))
}

/// The process id in the popup's marker, when it has one.
fn marker_owner(hwnd: HWND) -> Option<u32> {
    let value = marker_value(hwnd);
    Saved::decode(value).map(|_| owner(value))
}

fn me() -> u32 {
    std::process::id()
}

/// Whether `pid` is a running WinCraft: a program with the same file name
/// as this one.
fn is_running_wincraft(pid: u32) -> bool {
    let theirs = crate::core::windows_list::exe_path(pid);
    let ours = std::env::current_exe().unwrap_or_default();
    let name = |path: &std::path::Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    };
    !theirs.is_empty() && name(std::path::Path::new(&theirs)) == name(&ours)
}

fn is_held_elsewhere(owner: u32) -> bool {
    held_elsewhere(owner, me(), is_running_wincraft)
}

/// Makes the popup see-through. `Ok(true)` when this WinCraft has it
/// see-through now, changed now or before. `Ok(false)` when another
/// WinCraft that still runs made it see-through: it is left alone. On an
/// error nothing is left changed.
pub fn apply(hwnd: HWND) -> Result<bool, String> {
    if let Some(owner) = marker_owner(hwnd) {
        if owner == me() {
            return Ok(true);
        }
        if is_held_elsewhere(owner) {
            return Ok(false);
        }
        // Left behind by a WinCraft that is gone: put back first.
        repair(hwnd);
    }
    let saved = plan(read(hwnd)).map_err(str::to_string)?;
    let name = wide(MARKER);
    // The marker goes first, so a crash after the style change can be
    // repaired at the next start.
    let ok = unsafe { SetPropW(hwnd, name.as_ptr(), saved.encode(me()) as _) } != 0;
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

/// Puts back what `apply` changed. False when the popup carries no marker
/// of this WinCraft: a marker of another one is left alone.
pub fn restore(hwnd: HWND) -> bool {
    let value = marker_value(hwnd);
    let Some(saved) = Saved::decode(value) else {
        return false;
    };
    if owner(value) != me() {
        return false;
    }
    put_back(hwnd, saved);
    true
}

/// At start: undoes what a WinCraft that crashed left on this popup.
pub fn repair(hwnd: HWND) -> Repair {
    let held = marker_owner(hwnd).is_some_and(is_held_elsewhere);
    let action = repair_plan(read(hwnd), marker(hwnd), held);
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
