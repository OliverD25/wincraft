//! Custom WindowDeck: the host gives every window of the Claude desktop app
//! its Claude card (status and age) before the strip is sent, the same way
//! it fills in `attention`. The session files are read by
//! `core::claude_sessions` on a thread of its own; this only looks at what
//! was read last, so opening WindowDeck never waits for the disk.

use super::*;
use crate::core::{claude_sessions, claude_waits, windows_list};

/// Fills `claude` on each window of a Claude group. Other groups are left
/// alone, and nothing is read when no Claude group is open.
pub(super) fn fill_claude_cards(groups: &mut [ArrangeGroup]) {
    let mut claude = groups
        .iter_mut()
        .filter(|group| claude_waits::is_claude_app(&group.key))
        .peekable();
    if claude.peek().is_none() {
        return;
    }
    claude_sessions::read_soon();
    let (sessions, popouts) = claude_sessions::sessions();
    ATTENTION.with(|attention| {
        let attention = attention.borrow();
        for window in claude.flat_map(|group| group.windows.iter_mut()) {
            // A renamed window is matched by the title the app gave it.
            let title = windows_list::window_text(window.hwnd as HWND);
            let session = claude_sessions::find(&sessions, &popouts, &title);
            let waiting = attention
                .waiting_since(window.hwnd)
                .map(claude_sessions::instant_ms);
            window.claude = Some(claude_sessions::card(session, waiting));
        }
    });
}
