//! What the servers show, worked out without drawing. On the Chats tab
//! each server has one short line, with its chats that wait for the user
//! under it (`short_lines`); the Servers tab lists a server's chats in
//! full (`lines`). Also the words on a server's line and in its hover
//! panel.

use std::collections::BTreeSet;

use crate::core::claude_sessions::duration_text;
use crate::plugins::claude_deck::emit::is_cold;
use crate::plugins::claude_deck::labels::time_label;
use crate::plugins::claude_deck::model::{Chat, Mark, DAY_MS};

use super::{group_name, HostView, Wait};

/// One line of the servers part of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerLine {
    /// A server's line, by its place in the views.
    Server(usize),
    Chat {
        chat: Box<Chat>,
        server: String,
        /// Drawn greyed: the server does not answer, or the chat is hidden.
        greyed: bool,
        pinned: bool,
        hidden: bool,
    },
    /// The chats idle for over a day: how many, youngest and oldest in
    /// days, and whether they are listed.
    Quiet {
        server: String,
        chats: usize,
        days: (u64, u64),
        open: bool,
    },
    Hidden {
        server: String,
        chats: usize,
        open: bool,
    },
}

/// What the lines are built from besides the views.
pub struct Shape<'a> {
    pub pinned: &'a BTreeSet<String>,
    pub hidden: &'a BTreeSet<String>,
    pub quiet_open: &'a BTreeSet<String>,
    pub hidden_open: &'a BTreeSet<String>,
    pub fold_cold: bool,
    pub now: u64,
}

/// The lines of every server group, top to bottom.
pub fn lines(views: &[(String, HostView)], shape: &Shape) -> Vec<ServerLine> {
    let mut lines = Vec::new();
    for (at, (server, view)) in views.iter().enumerate() {
        lines.push(ServerLine::Server(at));
        let offline = view.checked.is_some() && !view.online;
        let mut shown: Vec<&Chat> = Vec::new();
        let mut quiet: Vec<&Chat> = Vec::new();
        let mut hidden: Vec<&Chat> = Vec::new();
        for chat in &view.chats {
            if shape.hidden.contains(&chat.id) {
                hidden.push(chat);
            } else if shape.fold_cold && is_cold(chat) {
                quiet.push(chat);
            } else {
                shown.push(chat);
            }
        }
        // Pinned chats first; otherwise the order the server gave.
        shown.sort_by_key(|chat| !shape.pinned.contains(&chat.id));
        let row = |chat: &Chat, hidden: bool| ServerLine::Chat {
            chat: Box::new(chat.clone()),
            server: server.clone(),
            greyed: offline || hidden,
            pinned: shape.pinned.contains(&chat.id),
            hidden,
        };
        lines.extend(shown.iter().map(|chat| row(chat, false)));
        if !quiet.is_empty() {
            let open = shape.quiet_open.contains(server);
            let days: Vec<u64> = quiet
                .iter()
                .map(|chat| {
                    chat.since
                        .map_or(0, |at| shape.now.saturating_sub(at) / DAY_MS)
                })
                .collect();
            lines.push(ServerLine::Quiet {
                server: server.clone(),
                chats: quiet.len(),
                days: (
                    days.iter().copied().min().unwrap_or(0),
                    days.iter().copied().max().unwrap_or(0),
                ),
                open,
            });
            if open {
                lines.extend(quiet.iter().map(|chat| row(chat, false)));
            }
        }
        if !hidden.is_empty() {
            let open = shape.hidden_open.contains(server);
            lines.push(ServerLine::Hidden {
                server: server.clone(),
                chats: hidden.len(),
                open,
            });
            if open {
                lines.extend(hidden.iter().map(|chat| row(chat, true)));
            }
        }
    }
    lines
}

/// A server chat that waits for the user: it needs approval or asks a
/// question. It shows on the Chats tab under its server's short line.
pub fn waits_for_user(mark: Mark) -> bool {
    matches!(mark, Mark::NeedsApproval | Mark::AsksYou)
}

/// The Chats tab's lines: each server's short line, and under it the
/// server's chats that wait for the user (pinned first; hidden ones stay
/// hidden).
pub fn short_lines(views: &[(String, HostView)], shape: &Shape) -> Vec<ServerLine> {
    let mut lines = Vec::new();
    for (at, (server, view)) in views.iter().enumerate() {
        lines.push(ServerLine::Server(at));
        let offline = view.checked.is_some() && !view.online;
        let mut waiting: Vec<&Chat> = view
            .chats
            .iter()
            .filter(|chat| waits_for_user(chat.mark) && !shape.hidden.contains(&chat.id))
            .collect();
        waiting.sort_by_key(|chat| !shape.pinned.contains(&chat.id));
        lines.extend(waiting.into_iter().map(|chat| ServerLine::Chat {
            chat: Box::new(chat.clone()),
            server: server.clone(),
            greyed: offline,
            pinned: shape.pinned.contains(&chat.id),
            hidden: false,
        }));
    }
    lines
}

/// "3 h ago", "20 s ago".
fn ago(at: u64, now: u64) -> String {
    format!("{} ago", duration_text(now.saturating_sub(at)))
}

/// How many PC chats wait on this server now.
pub fn waiting_on(server: &str, waits: &[Wait]) -> usize {
    waits.iter().filter(|wait| wait.server == server).count()
}

/// How many of a server's chats wait for the user, and how many work.
pub fn counts(view: &HostView) -> (usize, usize) {
    let waiting = view
        .chats
        .iter()
        .filter(|chat| waits_for_user(chat.mark))
        .count();
    let working = view
        .chats
        .iter()
        .filter(|chat| chat.mark == Mark::Working)
        .count();
    (waiting, working)
}

/// The words at the right of a server's short line: the PC chats that
/// wait on it ("1 PC chat waits"), then its chats that wait for the user
/// and that work: "1 waits for you \u{b7} 2 working".
pub fn line_text(view: &HostView, waiting: usize, now: u64) -> String {
    match (view.checked, view.online) {
        (None, _) => "Checking\u{2026}".to_string(),
        (Some(_), false) => match view.last_seen {
            Some(seen) => format!("Offline \u{b7} last seen {}", ago(seen, now)),
            None => "Offline".to_string(),
        },
        (Some(_), true) => {
            let (for_you, working) = counts(view);
            let mut parts = Vec::new();
            if waiting > 0 {
                parts.push(waits_label(waiting));
            }
            match for_you {
                0 => {}
                1 => parts.push(WAITS_FOR_YOU_ONE.to_string()),
                n => parts.push(format!("{n} wait for you")),
            }
            if working > 0 {
                parts.push(format!("{working} working"));
            }
            if !parts.is_empty() {
                parts.join(" \u{b7} ")
            } else if view.chats.is_empty() {
                NO_CHATS.to_string()
            } else {
                NONE_WORKING.to_string()
            }
        }
    }
}

pub const WAITS_FOR_YOU_ONE: &str = "1 waits for you";
pub const NO_CHATS: &str = "No Claude chats";
pub const NONE_WORKING: &str = "No chat working";

/// "1 PC chat waits", "2 PC chats wait".
pub fn waits_label(count: usize) -> String {
    match count {
        1 => "1 PC chat waits".to_string(),
        n => format!("{n} PC chats wait"),
    }
}

/// "8 Claude chats: 1 working, 1 asks you, 6 idle."
fn chats_sentence(chats: &[Chat]) -> String {
    let count = |want: fn(Mark) -> bool| chats.iter().filter(|chat| want(chat.mark)).count();
    let parts: Vec<String> = [
        (count(|m| m == Mark::Working), "working"),
        (count(|m| m == Mark::AsksYou), "asks you"),
        (count(|m| matches!(m, Mark::Idle(_))), "idle"),
        (count(|m| m == Mark::Stopped), "stopped"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, words)| format!("{n} {words}"))
    .collect();
    let head = match chats.len() {
        1 => "1 Claude chat".to_string(),
        n => format!("{n} Claude chats"),
    };
    if parts.is_empty() {
        format!("{head}.")
    } else {
        format!("{head}: {}.", parts.join(", "))
    }
}

/// The server's health, for its hover panel: "Up 12 days · Restart
/// needed", "Disk 120 GB free of 512 GB", "Busiest: dockerd 85%".
pub fn health_lines(health: &super::reply::Health) -> Vec<String> {
    let mut lines = Vec::new();
    let mut first = Vec::new();
    if let Some(up) = health.up {
        first.push(format!("Up {}", duration_text(up * 1000)));
    }
    if health.reboot {
        first.push("Restart needed".to_string());
    }
    if !first.is_empty() {
        lines.push(first.join(" \u{b7} "));
    }
    if let Some((free, total)) = health.disk {
        let gb = |kb: u64| (kb as f64 / 1024.0 / 1024.0).round() as u64;
        lines.push(format!("Disk {} GB free of {} GB", gb(free), gb(total)));
    }
    if let Some((cpu, name)) = &health.top {
        lines.push(format!("Busiest: {name} {cpu}%"));
    }
    lines
}

/// The hover panel of a server's line.
pub fn hover_text(server: &str, view: &HostView, waits: &[Wait], now: u64) -> String {
    let name = group_name(server);
    let mut lines = Vec::new();
    match (view.checked, view.online) {
        (None, _) => lines.push(format!("{name} \u{b7} checking now")),
        (Some(_), false) => {
            let seen = match view.last_seen {
                Some(seen) => format!(" \u{b7} last seen {}", ago(seen, now)),
                None => String::new(),
            };
            lines.push(format!("{name} \u{b7} offline{seen}"));
            lines.push("The chats show what was known last.".to_string());
        }
        (Some(checked), true) => {
            lines.push(format!(
                "{name} \u{b7} online \u{b7} checked {}",
                ago(checked, now)
            ));
            lines.push(chats_sentence(&view.chats));
            if let (Some(cpu), Some(ram), Some((total, available))) = (view.cpu, view.ram, view.mem)
            {
                let gb = |kb: u64| kb as f64 / 1024.0 / 1024.0;
                lines.push(format!(
                    "CPU {cpu}%, RAM {ram}% ({:.1} of {:.0} GB).",
                    gb(total.saturating_sub(available)),
                    gb(total)
                ));
            }
        }
    }
    if view.online {
        lines.extend(health_lines(&view.health));
    }
    let mine: Vec<&Wait> = waits.iter().filter(|wait| wait.server == server).collect();
    if !mine.is_empty() {
        lines.push(format!("{} on {name}:", waits_label(mine.len())));
        for wait in mine {
            lines.push(format!(
                "{} \u{b7} {}",
                wait.title,
                time_label(wait.since, now)
            ));
        }
    }
    lines.join("\n")
}
