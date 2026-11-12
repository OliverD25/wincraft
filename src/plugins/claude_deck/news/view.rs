//! What the News tab shows, worked out without drawing: its lines in
//! order, the group counts, the cut of a long reply and the words.

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
/// A closed item shows this many lines of its reply.
pub const SHORT_LINES: usize = 3;
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
    /// A group's line: its count, whether it is folded, and whether an
    /// unseen reply is folded away in it.
    Head {
        head: Head,
        count: usize,
        folded: bool,
        dot: bool,
    },
    Item(&'a Item),
    /// The words of an empty tab.
    Empty,
}

/// The lines, top to bottom: the pinned chats, the last 24 hours, the
/// older replies. "Only unseen" hides the seen replies, never a pinned
/// chat. The empty words show only when nothing at all happened.
pub fn lines<'a>(
    news: &'a News,
    only_unseen: bool,
    folded: impl Fn(Head) -> bool,
) -> Vec<Line<'a>> {
    if news.is_empty() {
        return vec![Line::Empty];
    }
    let shown = |items: &'a [Item]| -> Vec<&'a Item> {
        items
            .iter()
            .filter(|item| !only_unseen || item.unseen)
            .collect()
    };
    let mut lines = Vec::new();
    let mut part = |head: Head, items: Vec<&'a Item>, always: bool| {
        if items.is_empty() && !always {
            return;
        }
        let folded = folded(head);
        lines.push(Line::Head {
            head,
            count: items.len(),
            folded,
            dot: folded && items.iter().any(|item| item.unseen && item.mark.is_none()),
        });
        if !folded {
            lines.extend(items.into_iter().map(Line::Item));
        }
    };
    part(Head::Waits, news.pinned.iter().collect(), false);
    // The line of the last 24 hours holds "Only unseen" and "Mark all
    // seen", so it stays even when it has no reply.
    part(Head::Recent, shown(&news.recent), true);
    part(Head::Older, shown(&news.older), false);
    lines
}

/// The keys of the lines the keyboard reaches, top to bottom.
pub fn picks(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| match line {
            Line::Head { head, .. } => Some(head.key()),
            Line::Item(item) => Some(item.key.clone()),
            Line::Empty => None,
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
