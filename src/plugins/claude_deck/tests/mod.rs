//! The tests of ClaudeDeck, by topic; the helpers here make chats and
//! read lines back as short words.

use std::collections::BTreeSet;

use crate::plugins::claude_deck::arrange::*;
use crate::plugins::claude_deck::store::*;

use crate::core::claude_sessions::{Age, Lineage, Session};
use crate::core::claude_windows::ChatWindow;
use crate::plugins::claude_deck::labels::*;
use crate::plugins::claude_deck::model::*;

const MIN: u64 = 60_000;
const NOW: u64 = 100 * DAY_MS;

fn chat(id: &str, folder: Option<&str>, created: u64) -> Chat {
    Chat {
        id: id.to_string(),
        title: format!("Chat {id}"),
        folder: folder.map(str::to_string),
        branch: Some("main".into()),
        model: None,
        parent: None,
        created,
        mark: Mark::Idle(Age::Fresh),
        since: Some(NOW - MIN),
        jobs: 0,
        window: None,
        text: None,
        full: None,
    }
}

fn fork(id: &str, parent: &str, created: u64) -> Chat {
    Chat {
        parent: Some(parent.to_string()),
        ..chat(id, Some(r"E:\code\WinCraft"), created)
    }
}

fn cold(id: &str, folder: Option<&str>, days: u64) -> Chat {
    Chat {
        mark: Mark::Idle(Age::Cold),
        since: Some(NOW - days * DAY_MS),
        ..chat(id, folder, days)
    }
}

/// The lines as short words: "G name count", "C id depth", "S title",
/// "~ count", "H count".
fn shape(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            Line::Group(g) => format!("G {} {}", g.name, g.chats),
            Line::Chat(c) if c.stand_in => format!("S {}", c.chat.title),
            Line::Chat(c) => format!("C {} {}", c.chat.id, c.depth),
            Line::Cold(c) => format!("~ {}", c.chats),
            Line::Hidden(h) => format!("H {}", h.chats),
        })
        .collect()
}

fn chat_line<'a>(lines: &'a [Line], id: &str) -> &'a ChatLine {
    lines
        .iter()
        .find_map(|line| match line {
            Line::Chat(c) if c.chat.id == id => Some(c),
            _ => None,
        })
        .expect("listed")
}

fn session(id: &str) -> Session {
    Session {
        id: id.to_string(),
        title: "A chat".into(),
        latest_user: Some(NOW - 10 * MIN),
        last_turn_edge: Some(NOW - 5 * MIN),
        last_activity: Some(NOW - 5 * MIN),
        lineage: Lineage {
            origin_cwd: Some(r"E:\code\WinCraft".into()),
            cwd: Some(r"E:\code\WinCraft\.claude\worktrees\wt".into()),
            branch: Some("claude/wt".into()),
            model: Some("claude-opus-5-5".into()),
            forked_from: Some("local_p".into()),
            created: Some(42),
            ..Lineage::default()
        },
        ..Session::default()
    }
}

/// A saved arrangement with only this chat's forks folded.
fn folded_chat(id: &str) -> Saved {
    Saved {
        folded_chats: BTreeSet::from([id.to_string()]),
        ..Saved::default()
    }
}

mod arrange;
mod chats;
mod drop;
mod edit;
mod keyboard;
mod list;
mod modes;
mod plugin;
mod store;
