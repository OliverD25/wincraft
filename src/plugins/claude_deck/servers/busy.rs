//! Two chats busy on the same repo at once, on this PC or on a server, in
//! any mix: both rows say where the other one works, and the hover panel
//! warns that their changes can conflict. A repo is told by the last part
//! of its folder (`verdict-ai`); a git worktree counts as its repo. Only
//! chats that work (Working, or Waits on a server) count.

use crate::plugins::claude_deck::model::{Chat, Mark};

use super::{group_name, HostView};

pub const CONFLICT: &str =
    "Two chats work on this repo at the same time. Their changes can conflict.";
/// The place of a PC chat, in "also busy on this PC".
pub const THIS_PC: &str = "this PC";

/// A busy chat: where it runs, its repo and its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Busy {
    pub place: String,
    pub repo: String,
    pub id: String,
}

fn works(mark: Mark) -> bool {
    matches!(mark, Mark::Working | Mark::WaitsOnServer)
}

/// The repo a folder belongs to, in lower case: its last part, with a
/// worktree under `.claude/worktrees/` counted as its repo.
pub fn repo_of(folder: &str) -> Option<String> {
    let folder = folder.replace('\\', "/");
    let lower = folder.to_lowercase();
    let cut = lower.find("/.claude/worktrees/").unwrap_or(folder.len());
    let name = folder[..cut]
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())?;
    Some(name.to_lowercase())
}

/// Every busy chat with a known folder, on this PC and on the servers.
pub fn busy_chats(pc: &[Chat], views: &[(String, HostView)]) -> Vec<Busy> {
    let on_pc = pc.iter().map(|chat| (THIS_PC.to_string(), chat));
    let on_servers = views
        .iter()
        .filter(|(_, view)| view.online)
        .flat_map(|(server, view)| {
            view.chats
                .iter()
                .map(move |chat| (group_name(server), chat))
        });
    on_pc
        .chain(on_servers)
        .filter(|(_, chat)| works(chat.mark))
        .filter_map(|(place, chat)| {
            Some(Busy {
                place,
                repo: repo_of(chat.folder.as_deref()?)?,
                id: chat.id.clone(),
            })
        })
        .collect()
}

/// Adds the warning to a busy chat that shares its repo with another busy
/// chat: "Also busy on home2 · <its line>", and the conflict sentence first
/// in its hover panel.
pub fn warn(chat: &mut Chat, busy: &[Busy]) {
    if !works(chat.mark) {
        return;
    }
    let Some(repo) = chat.folder.as_deref().and_then(repo_of) else {
        return;
    };
    let Some(other) = busy
        .iter()
        .find(|other| other.repo == repo && other.id != chat.id)
    else {
        return;
    };
    let line = format!("Also busy on {}", other.place);
    chat.text = Some(match chat.text.take() {
        Some(text) => format!("{line} \u{b7} {text}"),
        None => line,
    });
    chat.full = Some(match chat.full.take() {
        Some(full) => format!("{CONFLICT}\n{full}"),
        None => CONFLICT.to_string(),
    });
}

/// Warns the PC chats that share a repo with another busy chat.
pub fn warn_pc(pc: &mut [Chat], views: &[(String, HostView)]) {
    let busy = busy_chats(pc, views);
    if busy.len() < 2 {
        return;
    }
    for chat in pc.iter_mut() {
        warn(chat, &busy);
    }
}

/// Warns the server chats that share a repo with another busy chat.
pub fn warn_servers(pc: &[Chat], views: &mut [(String, HostView)]) {
    let busy = busy_chats(pc, views);
    if busy.len() < 2 {
        return;
    }
    for (_, view) in views.iter_mut().filter(|(_, view)| view.online) {
        for chat in &mut view.chats {
            warn(chat, &busy);
        }
    }
}
