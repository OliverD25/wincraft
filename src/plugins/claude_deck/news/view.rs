//! What the News tab shows, worked out without drawing: its lines in
//! order (groups, one block per chat, its replies, the folded ones), the
//! counts, the cut of a long reply and the words.

use super::collect::{Item, News};

pub const CHATS: &str = "Chats";
pub const NEWS: &str = "News";
pub const WAITS: &str = "Waits for you";
pub const RECENT: &str = "Last 24 hours";
pub const OLDER: &str = "Older";
pub const ONLY_UNSEEN: &str = "Only unseen";
pub const MARK_ALL: &str = "Mark all seen";
pub const OPEN_CHAT: &str = "Open the chat";
pub const OPEN_HINT: &str = "or Enter, or a double-click";
pub const EMPTY: &str = "No replies in the last 24 hours. Each reply a chat finishes shows here.";
pub const NOT_SEEN: &str = "Not seen yet";
/// A closed reply shows this many lines.
pub const SHORT_LINES: usize = 2;
/// An open item shows this many lines before "Show all".
pub const LONG_LINES: usize = 11;

/// The three groups of the tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Head {
    Waits,
    Recent,
    Older,
}

impl Head {
    pub fn name(self) -> &'static str {
        match self {
            Head::Waits => WAITS,
            Head::Recent => RECENT,
            Head::Older => OLDER,
        }
    }

    /// The key a group line has among the picks.
    pub fn key(self) -> String {
        format!("head:{}", self.name())
    }

    pub fn of_key(key: &str) -> Option<Head> {
        [Head::Waits, Head::Recent, Head::Older]
            .into_iter()
            .find(|head| head.key() == key)
    }
}

/// One line of the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line<'a> {
    /// A group's line: its count of chats, whether it is folded, and
    /// whether an unseen reply is folded away in it.
    Head {
        head: Head,
        count: usize,
        folded: bool,
        dot: bool,
    },
    /// A chat's block line: its newest item gives the name, the folder
    /// and the server. `unseen` counts its unseen replies, also while the
    /// chat is collapsed; `open` is true while any reply is listed.
    Block {
        head: Head,
        item: &'a Item,
        unseen: usize,
        open: bool,
        fold: ChatFold,
    },
    Item(&'a Item),
    /// "N earlier replies" under a folded chat.
    Earlier {
        chat: String,
        count: usize,
    },
    /// The words of an empty tab.
    Empty,
}

/// What the lines depend on besides the items.
pub trait Folds {
    fn only_unseen(&self) -> bool;
    fn head_folded(&self, head: Head) -> bool;
    /// How the user folded this chat; Default after a new reply.
    fn chat_fold(&self, chat: &str) -> ChatFold;
    /// An item that must stay listed even when its chat is folded: the
    /// open one and the one the keyboard is on.
    fn keep(&self, key: &str) -> bool;
}

/// How a chat's block shows its replies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatFold {
    /// The unseen replies, or the newest when all are seen; the rest fold
    /// into "N earlier replies".
    #[default]
    Default,
    /// Every reply.
    AllOpen,
    /// Only the block line.
    Collapsed,
}

impl ChatFold {
    /// The block line's arrow, and Space: it folds a chat whose replies
    /// show, and brings back the default of a collapsed one.
    pub fn arrow(self) -> ChatFold {
        match self {
            ChatFold::Collapsed => ChatFold::Default,
            _ => ChatFold::Collapsed,
        }
    }

    /// Right on a block line: one step more open.
    pub fn right(self) -> ChatFold {
        match self {
            ChatFold::Collapsed => ChatFold::Default,
            _ => ChatFold::AllOpen,
        }
    }

    /// Left on a block line.
    pub fn left(self) -> ChatFold {
        ChatFold::Collapsed
    }
}

/// The group whose block lists this item: a waiting chat's replies of
/// the day sit in its pinned block.
pub fn head_of(news: &News, key: &str) -> Option<Head> {
    let waits = |chat: &str| news.pinned.iter().any(|item| item.chat == chat);
    if let Some(item) = news.recent.iter().find(|item| item.key == key) {
        return Some(if waits(&item.chat) {
            Head::Waits
        } else {
            Head::Recent
        });
    }
    if news.pinned.iter().any(|item| item.key == key) {
        return Some(Head::Waits);
    }
    news.older
        .iter()
        .any(|item| item.key == key)
        .then_some(Head::Older)
}

/// The words of the line that unfolds a chat.
pub fn earlier(n: usize) -> String {
    match n {
        1 => "1 earlier reply".to_string(),
        n => format!("{n} earlier replies"),
    }
}

/// The key a chat's block line has among the picks.
pub fn block_key(head: Head, chat: &str) -> String {
    format!("chat:{}:{chat}", head.name())
}

/// The group and the chat of a block line's key.
pub fn of_block_key(key: &str) -> Option<(Head, &str)> {
    let rest = key.strip_prefix("chat:")?;
    [Head::Waits, Head::Recent, Head::Older]
        .into_iter()
        .find_map(|head| Some((head, rest.strip_prefix(head.name())?.strip_prefix(':')?)))
}

/// The items grouped by chat. The items come newest first, so the blocks
/// are in the order of their newest reply, and so are the items in each.
pub fn by_chat<'a>(items: impl IntoIterator<Item = &'a Item>) -> Vec<Vec<&'a Item>> {
    let mut blocks: Vec<Vec<&'a Item>> = Vec::new();
    for item in items {
        match blocks.iter_mut().find(|block| block[0].chat == item.chat) {
            Some(block) => block.push(item),
            None => blocks.push(vec![item]),
        }
    }
    blocks
}

/// The items a folded chat lists: every unseen one, or the newest when
/// all are seen, and the kept ones. The second number is how many fold
/// away; nothing folds in an unfolded chat.
pub fn fold<'a>(
    items: &[&'a Item],
    open: bool,
    keep: impl Fn(&str) -> bool,
) -> (Vec<&'a Item>, usize) {
    if open {
        return (items.to_vec(), 0);
    }
    let any_unseen = items.iter().any(|item| item.unseen);
    let shown: Vec<&'a Item> = items
        .iter()
        .enumerate()
        .filter(|(at, item)| item.unseen || keep(&item.key) || (!any_unseen && *at == 0))
        .map(|(_, item)| *item)
        .collect();
    let folded = items.len() - shown.len();
    (shown, folded)
}

/// The unseen replies in a block; a pinned question is not a reply.
pub fn unseen_replies(items: &[&Item]) -> usize {
    items
        .iter()
        .filter(|item| item.unseen && item.mark.is_none())
        .count()
}

/// The lines, top to bottom: the pinned chats, the last 24 hours, the
/// older replies, each group one block per chat. A chat that waits shows
/// its question and its replies of the day in its pinned block only.
/// "Only unseen" hides the seen replies and a block left with none, never
/// a pinned chat. The empty words show only when nothing happened.
pub fn lines<'a>(news: &'a News, folds: &impl Folds) -> Vec<Line<'a>> {
    if news.is_empty() {
        return vec![Line::Empty];
    }
    let only_unseen = folds.only_unseen();
    let shown = |item: &&'a Item| !only_unseen || item.unseen;
    let waiting: Vec<&str> = news.pinned.iter().map(|item| item.chat.as_str()).collect();
    let pinned: Vec<Vec<&'a Item>> = news
        .pinned
        .iter()
        .map(|question| {
            let mut block = vec![question];
            block.extend(
                news.recent
                    .iter()
                    .filter(|item| item.chat == question.chat)
                    .filter(shown),
            );
            block
        })
        .collect();
    let recent = by_chat(
        news.recent
            .iter()
            .filter(|item| !waiting.contains(&item.chat.as_str()))
            .filter(shown),
    );
    let older = by_chat(news.older.iter().filter(shown));
    let mut lines = Vec::new();
    let mut part = |head: Head, blocks: Vec<Vec<&'a Item>>, always: bool| {
        if blocks.is_empty() && !always {
            return;
        }
        let folded = folds.head_folded(head);
        lines.push(Line::Head {
            head,
            count: blocks.len(),
            folded,
            dot: folded && blocks.iter().any(|block| unseen_replies(block) > 0),
        });
        if folded {
            return;
        }
        for block in blocks {
            let chat = &block[0].chat;
            let state = folds.chat_fold(chat);
            let (listed, rest) = match state {
                ChatFold::Collapsed => (Vec::new(), 0),
                _ => fold(&block, state == ChatFold::AllOpen, |key| folds.keep(key)),
            };
            lines.push(Line::Block {
                head,
                item: block[0],
                unseen: unseen_replies(&block),
                open: !listed.is_empty(),
                fold: state,
            });
            lines.extend(listed.into_iter().map(Line::Item));
            if rest > 0 {
                lines.push(Line::Earlier {
                    chat: chat.clone(),
                    count: rest,
                });
            }
        }
    };
    part(Head::Waits, pinned, false);
    // The line of the last 24 hours holds "Only unseen" and "Mark all
    // seen", so it stays even when it has no reply.
    part(Head::Recent, recent, true);
    part(Head::Older, older, false);
    lines
}

/// The keys of the lines the keyboard reaches, top to bottom. The
/// "earlier replies" line is skipped: Right on the block line unfolds.
pub fn picks(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| match line {
            Line::Head { head, .. } => Some(head.key()),
            Line::Block { head, item, .. } => Some(block_key(*head, &item.chat)),
            Line::Item(item) => Some(item.key.clone()),
            Line::Earlier { .. } | Line::Empty => None,
        })
        .collect()
}

/// How many lines of an open reply show, and the link's words when it is
/// cut: "Show all · 24 lines".
pub fn cut(total: usize, whole: bool) -> (usize, Option<String>) {
    if whole || total <= LONG_LINES {
        (total, None)
    } else {
        (LONG_LINES, Some(format!("Show all \u{b7} {total} lines")))
    }
}

/// A closed item's text: its lines joined, as the design shows it.
pub fn short_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The tray tooltip: the chats that wait and the unseen replies. It
/// always names WinCraft; None leaves the plain tooltip.
pub fn tray_tip(on: bool, waiting: usize, news: usize) -> Option<String> {
    if !on {
        return None;
    }
    let mut parts = Vec::new();
    match waiting {
        0 => {}
        1 => parts.push("1 chat waits for you".to_string()),
        n => parts.push(format!("{n} chats wait for you")),
    }
    match news {
        0 => {}
        1 => parts.push("1 new reply".to_string()),
        n => parts.push(format!("{n} new replies")),
    }
    (!parts.is_empty()).then(|| format!("WinCraft \u{2014} {}", parts.join(", ")))
}

/// The tip of the count in mini mode.
pub fn mini_tip(count: usize) -> String {
    match count {
        1 => "1 unseen reply: a click opens the News tab".to_string(),
        n => format!("{n} unseen replies: a click opens the News tab"),
    }
}
