//! The News items, from the chats and their finished replies. Pure code:
//! `news::refresh` gathers what it needs.

use crate::core::claude_turns::Reply;

use super::super::model::{Chat, Mark, DAY_MS};
use super::super::select;
use super::{Marks, KEEP_MARKS_MS};

/// At most this many replies of one chat in the last 24 hours.
pub const PER_CHAT: usize = 20;

/// One chat and what the News tab needs of it.
pub struct Source<'a> {
    pub chat: &'a Chat,
    /// Its finished replies, oldest first.
    pub replies: &'a [Reply],
    /// When the user last had the chat open in the Claude app.
    pub focused: Option<u64>,
    /// The listed server it runs on; None for a PC chat.
    pub server: Option<&'a str>,
}

/// One line of the tab: a reply, or a chat that waits (pinned).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Stable while the reply is there: the chat's id and the reply's
    /// time; `wait:<id>` for a pinned chat.
    pub key: String,
    /// The chat's id in the list.
    pub chat: String,
    pub title: String,
    /// The project: the last part of the chat's folder.
    pub project: Option<String>,
    /// The listed server, for a server chat.
    pub server: Option<String>,
    /// The chat's folder, where a server chat's terminal opens.
    pub folder: Option<String>,
    pub window: Option<isize>,
    /// When the reply was finished, or since when the chat waits.
    pub at: u64,
    pub text: String,
    pub unseen: bool,
    /// A pinned chat's status mark; None for a reply.
    pub mark: Option<Mark>,
}

/// The tab's items, each part newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct News {
    /// The chats that need approval or ask the user.
    pub pinned: Vec<Item>,
    /// Every reply of the last 24 hours.
    pub recent: Vec<Item>,
    /// Each chat's latest reply older than 24 hours.
    pub older: Vec<Item>,
}

impl News {
    /// The replies: the recent ones, then the older ones.
    pub fn replies(&self) -> impl Iterator<Item = &Item> {
        self.recent.iter().chain(self.older.iter())
    }

    /// Every item, pinned ones too.
    pub fn all(&self) -> impl Iterator<Item = &Item> {
        self.pinned.iter().chain(self.replies())
    }

    pub fn is_empty(&self) -> bool {
        self.pinned.is_empty() && self.recent.is_empty() && self.older.is_empty()
    }
}

/// The key of a reply's item.
pub fn key_of(chat: &str, at: u64) -> String {
    format!("{chat}@{at}")
}

/// Whether a reply is unseen: it is under 7 days old, it was not read
/// here, and (for a PC chat) it finished after the chat was last opened
/// in the app.
pub fn is_unseen(key: &str, at: u64, source: &Source, marks: &Marks, now: u64) -> bool {
    if now.saturating_sub(at) >= KEEP_MARKS_MS || marks.has(key) {
        return false;
    }
    source.server.is_some() || source.focused.is_none_or(|focused| focused < at)
}

/// The last part of a folder path, either slash.
pub fn project_of(folder: Option<&str>) -> Option<String> {
    let folder = folder?.trim_end_matches(['\\', '/']);
    folder
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn item(source: &Source, key: String, at: u64, text: String) -> Item {
    let chat = source.chat;
    Item {
        key,
        chat: chat.id.clone(),
        title: chat.title.clone(),
        project: project_of(chat.folder.as_deref()),
        server: source.server.map(str::to_string),
        folder: chat.folder.clone(),
        window: chat.window,
        at,
        text,
        unseen: false,
        mark: None,
    }
}

/// The items of all the chats.
pub fn collect(sources: &[Source], marks: &Marks, now: u64) -> News {
    let cut = now.saturating_sub(DAY_MS);
    let mut news = News::default();
    for source in sources {
        let chat = source.chat;
        if select::waits(chat.mark) {
            let text = chat
                .full
                .clone()
                .or_else(|| chat.text.clone())
                .unwrap_or_default();
            let mut pinned = item(
                source,
                format!("wait:{}", chat.id),
                chat.since.unwrap_or(now),
                text,
            );
            pinned.mark = Some(chat.mark);
            pinned.unseen = true;
            news.pinned.push(pinned);
        }
        let recent: Vec<&Reply> = source.replies.iter().filter(|r| r.at >= cut).collect();
        for reply in recent.iter().rev().take(PER_CHAT) {
            news.recent.push(reply_item(source, reply, marks, now));
        }
        if let Some(older) = source.replies.iter().rev().find(|r| r.at < cut) {
            news.older.push(reply_item(source, older, marks, now));
        }
    }
    for part in [&mut news.pinned, &mut news.recent, &mut news.older] {
        part.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.key.cmp(&b.key)));
    }
    news
}

fn reply_item(source: &Source, reply: &Reply, marks: &Marks, now: u64) -> Item {
    let key = key_of(&source.chat.id, reply.at);
    let mut item = item(source, key, reply.at, reply.text.clone());
    item.unseen = is_unseen(&item.key, reply.at, source, marks, now);
    item
}
