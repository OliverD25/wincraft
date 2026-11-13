//! Reading the chats into the window's state: the session files, the open
//! chat windows, the servers' jobs, the forks and the news. The window
//! does it while it shows; while ClaudeDeck is hidden in the tray, the
//! plugin's timer does it on the host thread (`tray_home`).

use std::time::Instant;

use crate::core::{animations, claude_sessions, claude_windows};

use super::arrange::arrange;
use super::data;
use super::forks;
use super::news;
use super::servers;
use super::store;
use super::window::DeckUi;

/// Builds the chats again from what was read last.
pub(super) fn read(state: &mut DeckUi) {
    state.built = Some(Instant::now());
    state.started.get_or_insert_with(Instant::now);
    state.animate = animations::on();
    let (sessions, _) = claude_sessions::sessions();
    let windows = claude_windows::latest();
    let now = claude_sessions::now_ms();
    state.seen.note(&windows, claude_windows::foreground(), now);
    let (chats, waits) = data::chats(
        &sessions,
        &windows,
        &state.seen,
        state.servers.aliases(),
        now,
    );
    state.chats = chats;
    state.servers.waits = waits;
    servers::jobs::refresh(&mut state.servers, &mut state.chats, &sessions, now);
    forks::add_rows(&mut state.chats, now);
    servers::busy::warn_pc(&mut state.chats, &state.servers.views());
    state.archived = data::archived(&state.chats);
    news::refresh(state, &sessions, now);
    if state.written.is_none() {
        let (saved, writable) = store::load(&store::path()).start();
        state.written = Some(saved.clone());
        state.saved = saved;
        state.writable = writable;
    }
    // Before the first read there are no chats yet; arranging then would
    // count every saved chat as gone.
    if !state.chats.is_empty() {
        state.saved = arrange(&state.saved, &state.chats, now);
    }
}
