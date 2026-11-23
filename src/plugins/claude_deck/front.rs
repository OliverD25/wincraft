//! The list marks the chat the user works in: when another window comes
//! to the front and it shows a listed chat, that chat's row becomes the
//! marked one. A hook on the window in front (`core::front_watch`) asks
//! for a frame at once, so the mark moves without waiting for the next
//! timed frame. While ClaudeDeck is hidden in the tray, its timer does the
//! same, so the right row is marked when the window shows again.

use std::sync::Arc;

use crate::core::{claude_app, claude_windows, front_watch};

use super::model::Line;
use super::select;
use super::window::{viewport_id, DeckUi};
use super::ID;

/// Asks for a frame of ClaudeDeck's window each time the window in front
/// changes. Cheap to call on every frame.
pub(super) fn listen(ctx: &egui::Context) {
    if front_watch::listens(ID) {
        return;
    }
    let ctx = ctx.clone();
    front_watch::listen(ID, Arc::new(move || ctx.request_repaint_of(viewport_id())));
}

/// Stops the frames on a change of the window in front.
pub(super) fn stop() {
    front_watch::stop(ID);
}

/// Marks the row of the chat in front. Only reads which window is in
/// front: no call here waits for another window, so it is safe with the
/// ClaudeDeck locks held. A menu, a rename or a drag is left alone.
pub(super) fn follow(state: &mut DeckUi, lines: &[Line]) {
    if state.chats.is_empty() {
        return;
    }
    let front = claude_windows::foreground();
    let moved = state.front_seen.replace(front) != Some(front);
    let busy = state.drag.is_some()
        || state.renaming.is_some()
        || state.noting.is_some()
        || state.menu_for.is_some();
    if busy {
        return;
    }
    let home = claude_app::home_chat();
    let home = home.as_ref().map(|(hwnd, id)| (*hwnd, id.as_str()));
    let pick = select::front_pick(lines, front, home);
    // The same window can show another chat (the main window after a
    // link), and the data can name a window's chat a moment later: a new
    // answer counts like a new window in front.
    let changed = pick != state.front_pick;
    state.front_pick = pick.clone();
    if let Some(pick) = pick.filter(|_| moved || changed) {
        if state.selected.as_ref() != Some(&pick) {
            state.selected = Some(pick);
            state.scroll_to_selected = true;
        }
    }
}
