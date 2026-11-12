//! The statuses ClaudeDeck tells beyond WindowDeck's (Done, not seen;
//! Stopped; Failed) and the words of a row's text line and hover panel.
//! Pure code: the evidence comes from `data.rs`.
//!
//! The rules, and why (real files read on 2026-11-12):
//! - **Stopped**: the chat is idle and its transcript's last record is the
//!   user's stop (`claude_turns::End::Interrupted`).
//! - **Failed**: the chat is idle and its last record is a failed request.
//! - Both only while the chat is under a day old (fresh or cooling): an old
//!   stop matters less than how old the chat is, so it stays Idle.
//! - **Done, not seen**: the chat is idle, its last record is a reply (or
//!   the transcript was not read), its turn ended while ClaudeDeck was open
//!   and watching, and since then neither was its window seen in front nor
//!   does the session's `lastFocusedAt` come after the end. `lastFocusedAt`
//!   alone cannot say "not seen": the app writes it when a window opens (11
//!   chats got the same second at the app's start) and not while the user
//!   types in a window already in front. So a turn that ended while
//!   ClaudeDeck was closed stays Idle.
//! - `interruptedAwaitingPerson` was null in all 40 newest session files, so
//!   it is not used.

use std::collections::HashMap;

use crate::core::claude_sessions::{Age, Session};
use crate::core::claude_turns::{first_sentence, last_question, End, Turn};
use crate::core::claude_windows::ChatWindow;

use super::labels::{
    approval_line, tool_doing, FAILED_LINE, LAST, STOPPED_LINE, THINKING, UNKNOWN_LINE,
};
use super::model::{Chat, Mark};

/// What is known about a chat beyond its session file.
#[derive(Clone, Copy, Debug, Default)]
pub struct Evidence<'a> {
    pub turn: Option<&'a Turn>,
    /// Since when ClaudeDeck has watched which window is in front.
    pub watching_since: Option<u64>,
    /// When the chat's window was last seen in front.
    pub looked_at: Option<u64>,
}

/// Which chat windows the user had in front, while ClaudeDeck is open.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    since: Option<u64>,
    looked: HashMap<String, u64>,
}

impl Seen {
    /// Notes the window in front now.
    pub fn note(&mut self, windows: &[ChatWindow], front: isize, now: u64) {
        self.since.get_or_insert(now);
        if front == 0 {
            return;
        }
        for window in windows.iter().filter(|window| window.hwnd == front) {
            self.looked.insert(window.session_id.clone(), now);
        }
    }

    pub fn evidence<'a>(&self, session_id: &str, turn: Option<&'a Turn>) -> Evidence<'a> {
        Evidence {
            turn,
            watching_since: self.since,
            looked_at: self.looked.get(session_id).copied(),
        }
    }
}

/// The chat with its full status and its words.
pub fn refine(mut chat: Chat, session: &Session, evidence: &Evidence) -> Chat {
    if let Mark::Idle(age) = chat.mark {
        let recent = matches!(age, Age::Fresh | Age::Cooling);
        let ended = session.last_turn_edge.or(session.last_activity);
        match evidence.turn.map(|turn| turn.end) {
            Some(End::Interrupted) if recent => {
                chat.mark = Mark::Stopped;
                chat.since = ended;
            }
            Some(End::Failed) if recent => {
                chat.mark = Mark::Failed;
                chat.since = ended;
            }
            Some(End::Replied) | None if not_seen(session, evidence) => {
                chat.mark = Mark::DoneNotSeen;
                chat.since = session.last_turn_edge;
            }
            _ => {}
        }
    }
    chat.text = line_text(chat.mark, evidence.turn);
    chat.full = full_text(chat.mark, evidence.turn);
    chat
}

/// The turn ended while ClaudeDeck watched, and the user has not had the
/// chat in front since.
fn not_seen(session: &Session, evidence: &Evidence) -> bool {
    let (Some(end), Some(watching)) = (session.last_turn_edge, evidence.watching_since) else {
        return false;
    };
    end >= watching
        && session
            .lineage
            .last_focused
            .is_none_or(|focused| focused < end)
        && evidence.looked_at.is_none_or(|looked| looked < end)
}

fn text(turn: Option<&Turn>) -> Option<&str> {
    turn.and_then(|turn| turn.text.as_deref())
}

fn last(turn: Option<&Turn>) -> Option<String> {
    text(turn)
        .map(first_sentence)
        .filter(|sentence| !sentence.is_empty())
}

/// The one line under the name.
pub fn line_text(mark: Mark, turn: Option<&Turn>) -> Option<String> {
    match mark {
        Mark::NeedsApproval => Some(approval_line(turn.and_then(|t| t.tool.as_deref()))),
        Mark::AsksYou => turn
            .and_then(|t| t.question.clone())
            .or_else(|| text(turn).and_then(last_question))
            .or_else(|| last(turn)),
        Mark::Working => turn.map(|turn| match (turn.end, &turn.tool) {
            (End::ToolCalled, Some(tool)) => tool_doing(tool),
            _ => THINKING.to_string(),
        }),
        Mark::DoneNotSeen => last(turn),
        Mark::Background | Mark::Idle(_) => last(turn).map(|sentence| format!("{LAST}{sentence}")),
        Mark::Stopped => Some(STOPPED_LINE.to_string()),
        Mark::Failed => Some(last(turn).unwrap_or_else(|| FAILED_LINE.to_string())),
        Mark::Unknown => Some(UNKNOWN_LINE.to_string()),
    }
}

/// The hover panel's text: the full question, else the last result.
pub fn full_text(mark: Mark, turn: Option<&Turn>) -> Option<String> {
    match mark {
        Mark::Unknown => None,
        Mark::AsksYou | Mark::NeedsApproval => turn
            .and_then(|t| t.question.clone())
            .or_else(|| text(turn).map(str::to_string)),
        _ => text(turn).map(str::to_string),
    }
}

#[cfg(test)]
mod tests;
