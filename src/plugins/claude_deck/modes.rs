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
    let size = mini.then(|| mini::size(&mini::items(lines)));
    if state.mini_on == Some(mini) && state.mini_size == size {
        return;
    }
    let was = state.mini_on.replace(mini);
    state.mini_size = size;
    let [width, height] = min_size(mini);
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(width, height)));
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

/// Hands the tray tooltip to the plugin when it changed; the plugin sets
/// it on the host thread.
pub(super) fn tell_tray(shared: &mut Shared, state: &mut DeckUi, tip: Option<String>) {
    if state.tip_sent.as_ref() == Some(&tip) {
        return;
    }
    shared.tray_tip = tip.clone();
    state.tip_sent = Some(tip);
    host::post_to_plugins(TRAY_POST_ID);
}

/// One frame of the fade. Returns when it needs the next frame.
pub(super) fn fade_step(ctx: &egui::Context, state: &mut DeckUi, on: bool) -> Option<Duration> {
    let (mouse_in, focused) = ctx.input(|i| (i.pointer.has_pointer(), i.focused));
    let waiting = &state.cache.waiting_ids;
    let seen = fade::Seen {
        on,
        mouse_in,
        focused,
        new_wait: select::new_wait(&state.waiting_before, waiting),
    };
    state.waiting_before = waiting.clone();
    let next = state.fade.step(seen, Instant::now());
    state.fade.apply(state.window);
    next
}
