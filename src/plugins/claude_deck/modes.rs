//! What ClaudeDeck's window does around the drawing: switching between
//! the full list and mini mode, giving the keyboard back on Esc, telling
//! the plugin the tray count, and the fade.

use std::time::{Duration, Instant};

use egui::vec2;
use windows_sys::Win32::Foundation::HWND;

use crate::core::ui_bridge::{HostChannel, HostRequest};
use crate::core::{host, instance};

use super::fade;
use super::mini;
use super::model::Line;
use super::place;
use super::select;
use super::window::{keep_place, DeckUi, MIN_SIZE, SIZE};
use super::{Shared, TRAY_POST_ID};

pub(super) fn min_size(mini: bool) -> [f32; 2] {
    if mini {
        [mini::WIDTH, mini::TOP]
    } else {
        MIN_SIZE
    }
}

/// Shrinks the window to the column of marks, or gives it its full place
/// and size back. The full place is kept while the column shows.
pub(super) fn switch_mode(ctx: &egui::Context, state: &mut DeckUi, mini: bool, lines: &[Line]) {
    if state.window == 0 {
        return;
    }
    let size =
        mini.then(|| mini::size(&mini::items(lines)) + vec2(0.0, mini::counted(state.news.count)));
    if state.mini_on == Some(mini) && state.mini_size == size {
        return;
    }
    let was = state.mini_on.replace(mini);
    state.mini_size = size;
    match size {
        Some(size) => {
            if was == Some(false) {
                state.mini_on = Some(false);
                keep_place(state);
                state.mini_on = Some(true);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        None if was == Some(true) => match state.kept {
            Some(kept) => place::apply(state.window, kept),
            None => ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(SIZE.into())),
        },
        None => {}
    }
}

/// Gives the window its smallest size again when the mode or the scale
/// changed. egui turns the size in points into pixels when it is sent, so
/// after a move to a monitor with another scale it must be sent again, or
/// the old monitor's smallest size would hold the window.
pub(super) fn keep_min_size(ctx: &egui::Context, state: &mut DeckUi, mini: bool) {
    if state.window == 0 {
        return;
    }
    let now = (mini, ctx.pixels_per_point());
    if state.min_for == Some(now) {
        return;
    }
    state.min_for = Some(now);
    let [width, height] = min_size(mini);
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(width, height)));
}

/// Esc: the keyboard goes back to the window that was in front when the
/// hotkey was pressed, if it is still there. A test copy only logs it.
pub(super) fn give_back(state: &mut DeckUi, to_host: &HostChannel) {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;
    let Some(before) = state.before else {
        return;
    };
    state.keyboard = false;
    if unsafe { IsWindow(before as HWND) } == 0 {
        state.before = None;
        return;
    }
    if instance::is_test() {
        log::info!("read-only test instance: giving the keyboard back skipped");
        return;
    }
    to_host.send(HostRequest::FocusWindow(before));
}

/// Hands the tray counts (the chats that wait, the unseen replies) to the
/// plugin when they changed; the plugin shows them on the host thread.
/// None: nothing reads any more.
pub(super) fn tell_tray(shared: &mut Shared, state: &mut DeckUi, counts: Option<(usize, usize)>) {
    if state.tip_sent == Some(counts) {
        return;
    }
    shared.home.counts = counts;
    state.tip_sent = Some(counts);
    host::post_to_plugins(TRAY_POST_ID);
}

/// One frame of the fade. Returns when it needs the next frame.
pub(super) fn fade_step(ctx: &egui::Context, state: &mut DeckUi, on: bool) -> Option<Duration> {
    let (mouse_in, focused) = ctx.input(|i| (i.pointer.has_pointer(), i.focused));
    let menu_open = egui::Popup::is_any_open(ctx) || state.quick.is_open(ctx);
    let waiting = &state.cache.waiting_ids;
    let seen = fade::Seen {
        on,
        mouse_in,
        focused,
        new_wait: select::new_wait(&state.waiting_before, waiting),
        menu_open,
    };
    state.waiting_before = waiting.clone();
    let next = state.fade.step(seen, Instant::now());
    state.fade.apply(state.window);
    next
}
