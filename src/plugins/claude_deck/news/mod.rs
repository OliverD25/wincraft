//! ClaudeDeck's News tab (design sections 13 and 14): every reply a chat
//! finished in the last 24 hours, one block per chat, the blocks in the
//! order of their newest reply; the chats that wait for the user pinned
//! on top; each chat's latest older reply in a folded "Older" group.
//! Server chats are in it too. A chat's block lists its unseen replies,
//! or its newest when all are seen; the rest fold into "N earlier
//! replies" until the user unfolds the chat, and fold again when the chat
//! gets a new reply.
//!
//! A reply is unseen when it finished after the chat was last opened in
//! the Claude app (the session's `lastFocusedAt`, or its window seen in
//! front while ClaudeDeck is open) and it was not read here. Reading an
//! item here marks it seen; the marks are kept in a small file next to
//! the list's, for 7 days. A server chat has no app focus time, so only
//! the marks count for it.
//!
//! - `collect`: the items, from the chats and their replies (pure).
//! - `view`: the lines the tab shows, the counts and the words (pure).
//! - `draw`: the tabs strip, the tab, the mini mode count.
//! - `groups`: drawing a chat's block line, its replies and the
//!   "earlier replies" line.
//! - `keys`: the keyboard on the tab.

pub mod collect;
pub mod draw;
mod groups;
pub mod keys;
pub mod view;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::claude_jobs;
use crate::core::claude_sessions::Session;
use crate::core::config;

use super::window::{listed, DeckUi};
use collect::{collect, News, Source};
use view::{Folds, Head};

/// A read mark is kept this long, then dropped. A reply older than this
/// counts as seen.
pub const KEEP_MARKS_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const FILE: &str = "claude_deck.news.json";
const VERSION: u32 = 1;

/// The window's two tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Chats,
    News,
}

/// The items read here: each item's key and the time of its reply.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub read: HashMap<String, u64>,
}

impl Marks {
    pub fn has(&self, key: &str) -> bool {
        self.read.contains_key(key)
    }

    /// Marks an item read. True when it was not before.
    pub fn mark(&mut self, key: &str, at: u64) -> bool {
        self.read.insert(key.to_string(), at).is_none()
    }

    /// Drops the marks of replies older than `KEEP_MARKS_MS`.
    pub fn drop_old(&mut self, now: u64) {
        self.read
            .retain(|_, at| now.saturating_sub(*at) < KEEP_MARKS_MS);
    }
}

pub fn path() -> PathBuf {
    config::plugins_dir().join(FILE)
}

/// The marks in the file; none when it is missing or broken (a broken
/// file only means the replies show as unseen again).
pub fn load(path: &Path, now: u64) -> Marks {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Marks::default(),
        Err(err) => {
            log::warn!("ClaudeDeck: could not read {}: {err}", path.display());
            return Marks::default();
        }
    };
    let mut marks = match serde_json::from_str::<Marks>(&text) {
        Ok(marks) => marks,
        Err(err) => {
            log::warn!("ClaudeDeck: the News marks file is broken, starting fresh: {err}");
            Marks::default()
        }
    };
    marks.drop_old(now);
    marks
}

/// Writes the marks, without the old ones.
pub fn save(path: &Path, marks: &Marks, now: u64) {
    let mut marks = marks.clone();
    marks.version = VERSION;
    marks.drop_old(now);
    let Ok(text) = serde_json::to_string(&marks) else {
        return;
    };
    if let Err(err) = config::write_atomic(path, &text) {
        log::warn!("ClaudeDeck: could not save which replies were read: {err}");
    }
}

/// The News part of the window's state.
#[derive(Debug, Default)]
pub struct NewsUi {
    pub tab: Tab,
    /// The items, shared with the frame that draws them.
    pub news: Arc<News>,
    /// The read marks; None until the file was read.
    pub marks: Option<Marks>,
    /// Where the marks are kept; None is the real file.
    pub file: Option<PathBuf>,
    /// The open item, by its key.
    pub open: Option<String>,
    /// The items whose whole reply shows ("Show all").
    pub whole: HashSet<String>,
    /// The folded groups. "Older" starts folded.
    pub folded: BTreeSet<Head>,
    /// The groups the user unfolded, so "Older" stays open.
    pub unfolded: BTreeSet<Head>,
    /// The chats the user unfolded, with the time of their newest item
    /// then. A newer item folds the chat again.
    pub open_chats: HashMap<String, u64>,
    pub only_unseen: bool,
    /// The item or group line the keyboard is on.
    pub selected: Option<String>,
    pub scroll_to_selected: bool,
    /// The keys of the items and group lines, top to bottom, as drawn last.
    pub picks: Vec<String>,
    /// The chats with an unseen reply.
    pub unseen_chats: HashSet<String>,
    /// How many replies are unseen: the tab's count.
    pub count: usize,
}

impl NewsUi {
    /// What a closed window keeps: the marks and the switch.
    pub fn carry(&mut self) -> NewsUi {
        NewsUi {
            marks: self.marks.take(),
            file: self.file.take(),
            only_unseen: self.only_unseen,
            ..NewsUi::default()
        }
    }

    fn file(&self) -> PathBuf {
        self.file.clone().unwrap_or_else(path)
    }

    pub fn is_folded(&self, head: Head) -> bool {
        self.folded.contains(&head) || (head == Head::Older && !self.unfolded.contains(&head))
    }

    /// Folds or unfolds a group line.
    pub fn fold(&mut self, head: Head, folded: bool) {
        if folded {
            self.folded.insert(head);
            self.unfolded.remove(&head);
        } else {
            self.folded.remove(&head);
            self.unfolded.insert(head);
        }
    }

    /// The time of a chat's newest item.
    fn newest(&self, chat: &str) -> Option<u64> {
        self.news
            .all()
            .filter(|item| item.chat == chat)
            .map(|item| item.at)
            .max()
    }

    /// Unfolds or folds a chat's block.
    pub fn fold_chat(&mut self, chat: &str, folded: bool) {
        if folded {
            self.open_chats.remove(chat);
        } else if let Some(at) = self.newest(chat) {
            self.open_chats.insert(chat.to_string(), at);
        }
    }

    /// Marks these items read and saves the marks when any is new.
    pub fn mark_read<'a>(&mut self, items: impl IntoIterator<Item = (&'a str, u64)>, now: u64) {
        let marks = self.marks.get_or_insert_with(Marks::default);
        let mut changed = false;
        for (key, at) in items {
            changed |= marks.mark(key, at);
        }
        if changed {
            let marks = marks.clone();
            save(&self.file(), &marks, now);
            self.apply_marks();
        }
    }

    /// Marks every reply in the tab read ("Mark all seen").
    pub fn mark_all(&mut self, now: u64) {
        let all: Vec<(String, u64)> = self
            .news
            .replies()
            .map(|item| (item.key.clone(), item.at))
            .collect();
        self.mark_read(all.iter().map(|(key, at)| (key.as_str(), *at)), now);
    }

    /// Marks every reply of one chat seen: its chat was opened.
    pub fn read_chat(&mut self, chat: &str, now: u64) {
        let replies: Vec<(String, u64)> = self
            .news
            .replies()
            .filter(|item| item.chat == chat)
            .map(|item| (item.key.clone(), item.at))
            .collect();
        self.mark_read(replies.iter().map(|(key, at)| (key.as_str(), *at)), now);
    }

    /// Opens an item, or closes it when it is open. Opening marks it seen.
    pub fn toggle(&mut self, key: &str, now: u64) {
        if self.open.as_deref() == Some(key) {
            self.open = None;
            return;
        }
        self.open = Some(key.to_string());
        self.read(key, now);
    }

    /// Marks one item seen.
    pub fn read(&mut self, key: &str, now: u64) {
        let at = self
            .news
            .replies()
            .find(|item| item.key == key)
            .map(|i| i.at);
        if let Some(at) = at {
            self.mark_read([(key, at)], now);
        }
    }

    /// The items' unseen state after the marks changed, without waiting
    /// for the next read.
    fn apply_marks(&mut self) {
        let Some(marks) = &self.marks else {
            return;
        };
        let news = Arc::make_mut(&mut self.news);
        for item in news.recent.iter_mut().chain(news.older.iter_mut()) {
            if marks.has(&item.key) {
                item.unseen = false;
            }
        }
        self.recount();
    }

    fn recount(&mut self) {
        self.count = self.news.replies().filter(|item| item.unseen).count();
        self.unseen_chats = self
            .news
            .replies()
            .filter(|item| item.unseen)
            .map(|item| item.chat.clone())
            .collect();
    }

    /// Takes the items worked out again.
    pub fn take(&mut self, news: News) {
        self.news = Arc::new(news);
        let open: Vec<(String, u64)> = self.open_chats.drain().collect();
        for (chat, at) in open {
            if self.newest(&chat).is_some_and(|newest| newest <= at) {
                self.open_chats.insert(chat, at);
            }
        }
        self.recount();
    }
}

impl Folds for NewsUi {
    fn only_unseen(&self) -> bool {
        self.only_unseen
    }

    fn head_folded(&self, head: Head) -> bool {
        self.is_folded(head)
    }

    fn chat_open(&self, chat: &str) -> bool {
        self.open_chats.contains_key(chat)
    }

    fn keep(&self, key: &str) -> bool {
        self.open.as_deref() == Some(key) || self.selected.as_deref() == Some(key)
    }
}

/// Builds the News items again from what was read last, and marks the
/// chats that have an unseen reply. Called with each build of the chats.
pub fn refresh(state: &mut DeckUi, sessions: &[Session], now: u64) {
    if state.news.marks.is_none() {
        state.news.marks = Some(load(&state.news.file(), now));
    }
    let chats = listed(state);
    let views = state.servers.views();
    let by_id: HashMap<&str, &Session> = sessions.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut kept = Vec::new();
    for chat in &chats {
        let Some(session) = by_id.get(chat.id.as_str()) else {
            continue;
        };
        let replies = claude_jobs::replies(&session.cli_session_id).unwrap_or_default();
        let looked = state.seen.evidence(&chat.id, None).looked_at;
        let focused = session.lineage.last_focused.max(looked);
        kept.push((chat, replies, focused));
    }
    let mut sources: Vec<Source> = kept
        .iter()
        .map(|(chat, replies, focused)| Source {
            chat,
            replies: replies.as_slice(),
            focused: *focused,
            server: None,
        })
        .collect();
    let hidden = &state.saved.hidden;
    for (server, view) in &views {
        for chat in view.chats.iter().filter(|chat| !hidden.contains(&chat.id)) {
            sources.push(Source {
                chat,
                replies: view.replies.get(&chat.id).map_or(&[], Vec::as_slice),
                focused: None,
                server: Some(server),
            });
        }
    }
    let empty = Marks::default();
    let marks = state.news.marks.as_ref().unwrap_or(&empty);
    let news = collect(&sources, marks, now);
    state.news.take(news);
    let unseen = &state.news.unseen_chats;
    for chat in &mut state.chats {
        chat.news = unseen.contains(&chat.id);
    }
}

#[cfg(test)]
mod tests;
