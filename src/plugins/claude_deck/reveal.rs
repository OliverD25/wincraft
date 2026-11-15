//! ClaudeDeck's window goes on screen only once it has drawn itself, so
//! Windows never shows it white first (`crate::ui::reveal`). It is made
//! hidden, found and put in its place on the main frame, shown covered,
//! and uncovered after one frame drawn there. Back from the tray and after
//! a restore it is covered the same way, as Windows has no picture of it.

use std::sync::Mutex;
use std::time::Instant;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, SW_RESTORE, SW_SHOW};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest};
use crate::core::{dpi_follow, window_place};
use crate::ui::dwm_thumbs;
use crate::ui::reveal::{self, Reveal, Step};
use crate::ui::settings;

use super::place;
use super::window::{viewport_id, DeckUi, TITLE};
use super::Shared;

/// The main frame's step, only when neither lock is busy
/// (`reveal::try_both`). Returns the pin and the mini mode for the window's
/// builder, or None when the builder keeps what it had: the step was
/// skipped, or the window is hidden in the tray. A new pin there would
/// change a winit window flag, and winit, which thinks the window is still
/// shown, would show it again.
pub(super) fn main_frame(
    ctx: &egui::Context,
    shared: &Mutex<Shared>,
    state: &Mutex<DeckUi>,
    palette: isize,
) -> Option<(bool, bool)> {
    let (shared, mut state) = reveal::try_both(ctx, shared, state)?;
    on_main_frame(ctx, &shared, &mut state, palette);
    (!shared.home.hidden).then_some((shared.settings.stay_on_top, shared.settings.mini))
}

/// Runs while the window may still be hidden.
fn on_main_frame(ctx: &egui::Context, shared: &Shared, state: &mut DeckUi, palette: isize) {
    if !shared.open || shared.home.hidden {
        return;
    }
    if state.reveal == Reveal::Hidden {
        find_and_place(state, palette, &Tokens::get(ctx));
    }
    match state
        .reveal
        .on_main_frame(state.window != 0, Instant::now())
    {
        Step::Show => {
            reveal::show_covered(state.window, SW_SHOW);
            // winit hides a window it thinks is hidden whenever one of its
            // flags changes (the pin, minimise), so it must know.
            ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Visible(true));
            ctx.request_repaint_of(viewport_id());
            ctx.request_repaint();
        }
        Step::UncoverLate => {
            reveal::uncover(state.window);
            log::warn!("ClaudeDeck: its window was shown before it had drawn itself");
        }
        Step::Again => {
            ctx.request_repaint_of(viewport_id());
            ctx.request_repaint();
        }
        Step::Nothing | Step::Uncover => {}
    }
}

/// The window's own frame, before it draws: uncovers it after the first
/// frame drawn while covered, and takes the keyboard when it was asked.
pub(super) fn on_window_frame(ctx: &egui::Context, state: &mut DeckUi, to_host: &HostChannel) {
    match state.reveal.on_window_frame() {
        Step::Again => {
            ctx.request_repaint();
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        Step::Uncover => reveal::uncover(state.window),
        _ => {}
    }
    if state.window != 0 && std::mem::take(&mut state.focus_wanted) {
        to_host.send(HostRequest::FocusWindow(state.window));
    }
}

/// A minimised window does not draw, so it would never ask to come
/// forward: restore it here, at its last place, covered until it drew.
pub(super) fn restore(state: &mut DeckUi) {
    reveal::show_covered(state.window, SW_RESTORE);
    state.reveal.covered_again(Instant::now());
}

/// `keep_style` hid and showed the window again: it is covered until it
/// has drawn once more.
pub(super) fn covered_again(state: &mut DeckUi) {
    state.reveal.covered_again(Instant::now());
}

/// Finds the window egui made, gives it rounded corners and a border, and
/// puts it where it was the first time.
pub(super) fn find_and_place(state: &mut DeckUi, palette: isize, tokens: &Tokens) {
    if state.window != 0 && unsafe { IsWindow(state.window as HWND) } == 0 {
        // A window from before the last close: the new one is not placed
        // or shown yet.
        state.window = 0;
        state.placed = false;
        state.border = None;
        state.reveal = Reveal::Hidden;
    }
    if state.window == 0 {
        state.window = settings::find_window(TITLE, palette);
    }
    if state.window == 0 {
        return;
    }
    dpi_follow::follow(state.window);
    if state.border != Some(tokens.border) {
        state.border = Some(tokens.border);
        dwm_thumbs::round_corners(state.window as HWND, theme::colorref(tokens.border));
    }
    if !std::mem::replace(&mut state.placed, true) {
        if state.kept.is_none() {
            state.kept = place::load(&place::path());
        }
        match state.kept {
            Some(kept) => place::apply(state.window, kept),
            None => window_place::centre_on_cursor_monitor(state.window as HWND),
        }
        state.focus_wanted = true;
    }
}

#[cfg(test)]
mod tests;
