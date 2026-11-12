//! The chats ClaudeDeck lists, built from what the readers read last: the
//! session files, the open windows, the background jobs and the last turn
//! of each transcript; and which of them wait on a listed server.

use std::collections::HashSet;

use crate::core::claude_jobs;
use crate::core::claude_sessions::{self, Session};
use crate::core::claude_turns::Turn;
use crate::core::claude_windows::{self, ChatWindow};

use super::model::{self, Archived, Chat, Mark};
use super::servers::{self, Aliases, Wait};
use super::status::{self, Seen};

/// The chats, and the waits on servers among them.
pub fn chats(
    sessions: &[Session],
    windows: &[ChatWindow],
    seen: &Seen,
    aliases: &Aliases,
    now: u64,
) -> (Vec<Chat>, Vec<Wait>) {
    let mut waits = Vec::new();
    let chats = sessions
        .iter()
        .map(|session| {
            let window = claude_windows::of(windows, &session.id);
            let jobs = claude_jobs::running(&session.cli_session_id, now);
            let turn = claude_jobs::turn(&session.cli_session_id);
            let chat = model::chat(session, window, jobs, now);
            let chat = status::refine(chat, session, &seen.evidence(&session.id, turn.as_ref()));
            waits_on_server(chat, turn.as_ref(), aliases, &mut waits)
        })
        .collect();
    (chats, waits)
}

/// A working chat whose open tool call is a foreground ssh command to a
/// listed server waits on that server. A chat that needs approval for it
/// still needs approval: that is more urgent.
pub fn waits_on_server(
    mut chat: Chat,
    turn: Option<&Turn>,
    aliases: &Aliases,
    waits: &mut Vec<Wait>,
) -> Chat {
    if chat.mark != Mark::Working {
        return chat;
    }
    let Some(server) = servers::waits_on(turn, aliases) else {
        return chat;
    };
    let Some(ssh) = turn.and_then(|turn| turn.ssh.as_ref()) else {
        return chat;
    };
    chat.mark = Mark::WaitsOnServer;
    chat.since = ssh.at.or(chat.since);
    chat.text = Some(servers::wait_line(server, &ssh.command));
    chat.full = Some(servers::wait_full(&ssh.command));
    waits.push(Wait {
        chat: chat.id.clone(),
        server: server.to_string(),
        since: chat.since,
        title: chat.title.clone(),
    });
    chat
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
