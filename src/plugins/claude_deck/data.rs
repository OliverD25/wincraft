//! The chats ClaudeDeck lists, built from what the readers read last: the
//! session files, the open windows, the background jobs and the last turn
//! of each transcript.

use std::collections::HashSet;

use crate::core::claude_jobs;
use crate::core::claude_sessions::{self, Session};
use crate::core::claude_windows::{self, ChatWindow};
use crate::core::window_names;

use super::model::{self, Archived, Chat};
use super::status::{self, Seen};

pub fn chats(sessions: &[Session], windows: &[ChatWindow], seen: &Seen, now: u64) -> Vec<Chat> {
    sessions
        .iter()
        .map(|session| {
            let window = claude_windows::of(windows, &session.id);
            let jobs = claude_jobs::running(&session.cli_session_id, now);
            let turn = claude_jobs::turn(&session.cli_session_id);
            let chat = model::chat(session, window, jobs, now);
            let mut chat =
                status::refine(chat, session, &seen.evidence(&session.id, turn.as_ref()));
            // A window the user renamed shows under that name, as in WindowDeck.
            if let Some(name) = chat.window.and_then(window_names::custom) {
                chat.title = name;
            }
            chat
        })
        .collect()
}

/// The titles of archived chats that forks hang under. Read only when a
/// fork's chat is not in the list.
pub fn archived(chats: &[Chat]) -> Archived {
    let listed: HashSet<&str> = chats.iter().map(|chat| chat.id.as_str()).collect();
    let missing: HashSet<&str> = chats
        .iter()
        .filter_map(|chat| chat.parent.as_deref())
        .filter(|parent| !listed.contains(parent))
        .collect();
    if missing.is_empty() {
        return Archived::new();
    }
    claude_sessions::sessions_with_archived()
        .into_iter()
        .filter(|session| missing.contains(session.id.as_str()))
        .map(|session| (session.id, session.title))
        .collect()
}
