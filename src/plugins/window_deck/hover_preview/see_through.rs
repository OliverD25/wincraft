//! Explorer's taskbar popup kept see-through (alpha 0) while the hover strip
//! watches the taskbar, so a show that comes before our hide is never seen.
//!
//! Only popups the hover strip itself has hidden are made see-through:
//! Explorer has several windows of the popup's class, and the others (for
//! example Alt+Tab) are never touched. Everything is put back the moment the
//! mouse leaves the taskbar, the feature is switched off, the plugin stops or
//! WinCraft quits; `repair` puts back at start what a crash left behind.
//! If a popup cannot be made see-through, this is logged once and the plain
//! hide goes on alone.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::HWND;

use crate::core::taskbar_popup::{self as popup, see_through};

/// At most this many popups are remembered.
const MAX_KNOWN: usize = 4;

#[derive(Default)]
struct State {
    wanted: bool,
    /// Popups the hover strip has hidden: the taskbar's own.
    known: Vec<HWND>,
    /// Popups made see-through now.
    applied: Vec<HWND>,
    /// A popup refused it: only the plain hide is used from now on.
    off: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Which popups to make see-through and which to put back, for `wanted`.
pub fn changes(wanted: bool, known: &[HWND], applied: &[HWND]) -> (Vec<HWND>, Vec<HWND>) {
    if wanted {
        let new = known
            .iter()
            .copied()
            .filter(|hwnd| !applied.contains(hwnd))
            .collect();
        (new, Vec::new())
    } else {
        (Vec::new(), applied.to_vec())
    }
}

/// Keeps the known popups see-through or puts them back.
pub fn set(wanted: bool) {
    STATE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return;
        };
        state.wanted = wanted && !state.off;
        state.known.retain(|hwnd| popup::is_popup_window(*hwnd));
        let (apply, restore) = changes(state.wanted, &state.known, &state.applied);
        for hwnd in restore {
            put_back(&mut state, hwnd);
        }
        for hwnd in apply {
            make(&mut state, hwnd);
        }
    });
}

/// The hover strip hid this popup: it is the taskbar's, and see-through
/// from now on while that is wanted.
pub fn learn(hwnd: HWND) {
    STATE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return;
        };
        if !state.known.contains(&hwnd) {
            if state.known.len() >= MAX_KNOWN {
                state.known.remove(0);
            }
            state.known.push(hwnd);
        }
        if state.wanted && !state.applied.contains(&hwnd) {
            make(&mut state, hwnd);
        }
    });
}

/// Puts back every popup made see-through, and forgets them all.
pub fn restore_all() {
    set(false);
    STATE.with(|cell| {
        if let Ok(mut state) = cell.try_borrow_mut() {
            state.known.clear();
            state.off = false;
        }
    });
}

/// At start: puts back any popup a WinCraft that crashed left see-through.
pub fn repair() {
    for hwnd in popup::find_popups() {
        if let see_through::Repair::Restore(_) = see_through::repair(hwnd) {
            log::info!(
                "taskbar preview: made visible again, an earlier WinCraft had left it see-through"
            );
        }
    }
}

fn make(state: &mut State, hwnd: HWND) {
    if !popup::is_popup_window(hwnd) {
        return;
    }
    match see_through::apply(hwnd) {
        Ok(_) => state.applied.push(hwnd),
        Err(reason) => {
            log::info!("taskbar preview: cannot be made see-through ({reason}); it is only hidden");
            state.off = true;
            state.wanted = false;
            for hwnd in std::mem::take(&mut state.applied) {
                see_through::restore(hwnd);
            }
        }
    }
}

fn put_back(state: &mut State, hwnd: HWND) {
    state.applied.retain(|applied| *applied != hwnd);
    if popup::is_popup_window(hwnd) {
        see_through::restore(hwnd);
    }
}

#[cfg(test)]
mod tests;
