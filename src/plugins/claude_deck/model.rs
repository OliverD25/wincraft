//! What ClaudeDeck lists, worked out without any drawing: each chat's
//! status mark, the groups by project folder, the fork tree inside a group,
//! the roll-up of a folded chat or group, the line of cold chats and the
//! line of hidden chats. Rows never move by themselves: the groups and the
//! chats in them keep the user's saved order (`arrange`).

use std::collections::HashMap;

use crate::core::claude_sessions::{self, Age, Session, Status};
use crate::core::claude_windows::ChatWindow;

use super::arrange::{arrange, hidden_chats, hidden_roots};
use super::emit::{is_cold, most_urgent, Emit};
use super::store::{Colour, Saved};
use super::tree::{families, Family, Tree};

pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// A chat's status, as its mark shows it. `chat` gives Needs approval, Asks
/// you, Working, Background, Idle and Unknown; `status::refine` adds Done,
/// not seen, Stopped and Failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    NeedsApproval,
    AsksYou,
    DoneNotSeen,
    Working,
    /// A PC chat blocked on a foreground ssh command to a listed server.
    WaitsOnServer,
    Background,
    Idle(Age),
    Stopped,
    Failed,
    Unknown,
}

impl Mark {
    /// Most urgent first, as on the design page. A folded chat or group
    /// shows the most urgent mark inside it.
    pub const URGENCY: [Mark; 10] = [
        Mark::NeedsApproval,
        Mark::AsksYou,
        Mark::DoneNotSeen,
        Mark::Working,
        Mark::WaitsOnServer,
        Mark::Background,
        Mark::Idle(Age::Fresh),
        Mark::Stopped,
        Mark::Failed,
        Mark::Unknown,
    ];

    /// 0 is the most urgent; every age of Idle ranks the same.
    pub fn rank(self) -> u8 {
        let kind = std::mem::discriminant(&self);
        Mark::URGENCY
            .iter()
            .position(|mark| std::mem::discriminant(mark) == kind)
            .unwrap_or(Mark::URGENCY.len()) as u8
    }

    /// The short word "Shapes and words" puts after the shape.
    pub fn word(self) -> &'static str {
        match self {
            Mark::NeedsApproval => "Approve",
            Mark::AsksYou => "Asks",
            Mark::DoneNotSeen => "Done",
            Mark::Working => "Working",
            Mark::WaitsOnServer => "Waits",
            Mark::Background => "Bg",
            Mark::Idle(_) => "Idle",
            Mark::Stopped => "Stopped",
            Mark::Failed => "Failed",
            Mark::Unknown => "Unknown",
        }
    }

    /// Whether the word is bold: the statuses that want the user.
    pub fn bold(self) -> bool {
        matches!(self, Mark::NeedsApproval | Mark::AsksYou | Mark::Failed)
    }

    /// The marks the header counts, in its order.
    pub const COUNTED: [Mark; 4] = [
        Mark::NeedsApproval,
        Mark::AsksYou,
        Mark::Working,
        Mark::DoneNotSeen,
    ];
}

/// One chat, as ClaudeDeck needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chat {
    pub id: String,
    pub title: String,
    /// The project folder; None puts the chat in "New".
    pub folder: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    /// The chat it was forked from or started by.
    pub parent: Option<String>,
    pub created: u64,
    pub mark: Mark,
    /// When the status began, in Unix ms; None shows no time.
    pub since: Option<u64>,
    pub jobs: usize,
    /// The window that shows the chat, if one is open.
    pub window: Option<isize>,
    /// The line under the name: the question, what it does, or the last
    /// result. Set by `status::refine`.
    pub text: Option<String>,
    /// The hover panel's text: the full question or the last result.
    pub full: Option<String>,
    /// What "Fork in a terminal" needs; None when the chat cannot be forked.
    pub fork: Option<super::fork::Fork>,
}

const QUIET_MEANS_IDLE_MS: u64 = 60 * 60 * 1000;

/// The chat for a session, with its window (if open) and its running
/// background jobs (how many, and the start of the oldest). The status is
/// WindowDeck's card, so both show the same.
pub fn chat(
    session: &Session,
    window: Option<&ChatWindow>,
    jobs: (usize, Option<u64>),
    now: u64,
) -> Chat {
    let card = claude_sessions::card(Some(session), window.and_then(|w| w.waiting_since), jobs);
    let (mark, since) = match card.status {
        Some(Status::Waiting) if window.is_some_and(|w| w.approval) => {
            (Mark::NeedsApproval, card.since)
        }
        Some(Status::Waiting) => (Mark::AsksYou, card.since),
        Some(Status::Working) => (Mark::Working, card.since),
        Some(Status::Background) => (Mark::Background, card.since),
        Some(Status::Idle) => {
            let age = session
                .last_activity
                .map_or(Age::Fresh, |at| Age::of(now.saturating_sub(at)));
            (Mark::Idle(age), session.last_activity)
        }
        None => match session.last_activity {
            // The app has not loaded this chat since it started, so the turn
            // times are missing. A running chat keeps its activity time
            // fresh, so a quiet hour means it is idle.
            Some(at) if now.saturating_sub(at) >= QUIET_MEANS_IDLE_MS => {
                (Mark::Idle(Age::of(now - at)), Some(at))
            }
            _ => (Mark::Unknown, None),
        },
    };
    let lineage = &session.lineage;
    Chat {
        id: session.id.clone(),
        title: session.title.clone(),
        folder: lineage.folder().and_then(project_folder),
        branch: lineage.branch.clone(),
        model: lineage.model.clone(),
        parent: lineage.parent().map(str::to_string),
        created: lineage
            .created
            .or(session.last_activity)
            .unwrap_or(u64::MAX),
        mark,
        since,
        jobs: card.jobs,
        window: window.map(|w| w.hwnd),
        text: None,
        full: None,
        fork: super::fork::Fork::new(
            &session.cli_session_id,
            lineage.cwd.as_deref().or(lineage.folder()),
        ),
    }
}

/// The repository a folder belongs to: a worktree under
/// `<repo>\.claude\worktrees\` counts as the repository itself.
pub fn project_folder(folder: &str) -> Option<String> {
    let folder = folder.trim().replace('/', "\\");
    let lower = folder.to_lowercase();
    let cut = lower.find("\\.claude\\worktrees\\").unwrap_or(folder.len());
    let folder = folder[..cut].trim_end_matches('\\');
    (!folder.is_empty()).then(|| folder.to_string())
}

/// What a group of this folder is called: its last part.
pub fn group_name(folder: &str) -> String {
    folder
        .rsplit('\\')
        .find(|part| !part.is_empty())
        .unwrap_or(folder)
        .to_string()
}

/// The name of the group of chats with no known folder.
pub const NEW_GROUP: &str = "New";

/// Which parts of the list the user folded or switched, and the user's
/// arrangement of it.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub only_open: bool,
    pub fold_cold: bool,
    /// The hidden chats are listed under their line.
    pub hidden_open: bool,
    pub saved: Saved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fold {
    /// No forks.
    None,
    Open,
    /// Folded, with how many forks it hides.
    Folded(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Group(GroupLine),
    /// Boxed: a chat line holds the chat and its words, much more than the
    /// other lines.
    Chat(Box<ChatLine>),
    Cold(ColdLine),
    Hidden(HiddenLine),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupLine {
    /// The group's id in the saved arrangement.
    pub key: String,
    pub name: String,
    pub colour: Option<Colour>,
    pub chats: usize,
    pub folded: bool,
    /// The most urgent mark inside, shown while folded.
    pub roll: Mark,
    /// Since when the chat behind `roll` is in that state.
    pub roll_since: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLine {
    pub chat: Chat,
    /// A grey stand-in for an archived or missing chat that forks hang
    /// under; its title says so.
    pub stand_in: bool,
    /// 0 for a chat at the top of its family.
    pub depth: usize,
    /// For each level between the top and this chat's own: whether the
    /// tree line there goes on past this row.
    pub rails: Vec<bool>,
    /// The last fork of its chat: its line ends at this row.
    pub last: bool,
    pub fold: Fold,
    /// Kept at the top of its group by the user.
    pub pinned: bool,
    /// Listed under the "hidden chats" line.
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColdLine {
    pub chats: usize,
    /// The youngest and the oldest, in whole days idle.
    pub days: (u64, u64),
    pub open: bool,
}

/// The line that holds the chats the user hid from the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HiddenLine {
    pub chats: usize,
    pub open: bool,
}

/// The titles of archived chats, by id, to name the stand-ins.
pub type Archived = HashMap<String, String>;

/// The title a stand-in shows.
pub fn stand_in_title(archived: &Archived, id: &str) -> String {
    match archived.get(id).filter(|title| !title.is_empty()) {
        Some(title) => format!("{title} \u{b7} archived"),
        None => "Chat not found".to_string(),
    }
}

/// The key a family is saved under: its top chat, or the missing chat its
/// forks came from.
fn key_of(tree: &Tree, family: &Family) -> String {
    match family.top {
        Some(at) => tree.chats[at].id.clone(),
        None => family.missing.clone(),
    }
}

/// The lines of the list, top to bottom: the groups in the user's order,
/// each with its chats in the user's order, then the line of cold chats and
/// the line of hidden chats. `archived` names the chats forks came from
/// that are no longer listed.
pub fn lines(chats: &[Chat], archived: &Archived, view: &View, now: u64) -> Vec<Line> {
    let saved = arrange(&view.saved, chats, now);
    let hidden = hidden_chats(chats, &saved.hidden);
    let shown: Vec<Chat> = chats
        .iter()
        .enumerate()
        .filter(|(at, _)| !hidden.contains(at))
        .map(|(_, chat)| chat.clone())
        .collect();
    let tree = Tree::new(&shown, view.only_open);
    let mut by_key: HashMap<String, Family> = HashMap::new();
    let mut cold: Vec<Family> = Vec::new();
    for family in families(&tree) {
        let all_cold = family.members.iter().all(|at| is_cold(&shown[*at]));
        if view.fold_cold && all_cold {
            cold.push(family);
        } else {
            by_key.insert(key_of(&tree, &family), family);
        }
    }
    let mut emit = Emit {
        tree: &tree,
        folded: &saved.folded_chats,
        pinned: &saved.pinned,
        archived,
        lines: Vec::new(),
        drawn: Default::default(),
    };
    for group in &saved.groups {
        let families: Vec<Family> = group
            .chats
            .iter()
            .filter_map(|key| by_key.remove(key))
            .collect();
        if families.is_empty() && !(group.keep && !view.only_open) {
            continue;
        }
        let members: Vec<&Chat> = families
            .iter()
            .flat_map(|f| f.members.iter().map(|at| &shown[*at]))
            .collect();
        let urgent = most_urgent(members.iter().copied());
        let roll = urgent.map_or(Mark::Unknown, |c| c.mark);
        emit.lines.push(Line::Group(GroupLine {
            key: group.id.clone(),
            name: group.name.clone(),
            colour: group.colour,
            chats: members.len(),
            folded: group.folded,
            roll,
            roll_since: urgent.and_then(|c| c.since),
        }));
        if !group.folded {
            for family in &families {
                emit.family(family);
            }
        }
    }
    // `arrange` places every family, so nothing is left here; should one
    // be, it is still listed rather than lost.
    let mut left: Vec<Family> = by_key.into_values().collect();
    left.sort_by_key(|family| family.created);
    for family in &left {
        emit.family(family);
    }
    if !cold.is_empty() {
        cold.sort_by_key(|family| family.created);
        let days: Vec<u64> = cold
            .iter()
            .flat_map(|f| f.members.iter())
            .map(|at| {
                shown[*at]
                    .since
                    .map_or(0, |since| now.saturating_sub(since) / DAY_MS)
            })
            .collect();
        emit.lines.push(Line::Cold(ColdLine {
            chats: days.len(),
            days: (
                days.iter().copied().min().unwrap_or(0),
                days.iter().copied().max().unwrap_or(0),
            ),
            open: saved.cold_open,
        }));
        if saved.cold_open {
            for family in &cold {
                emit.family(family);
            }
        }
    }
    let mut lines = emit.lines;
    hidden_lines(&mut lines, chats, archived, &saved, view.hidden_open);
    lines
}

/// The line of hidden chats and, while it is open, one plain row for each.
fn hidden_lines(
    lines: &mut Vec<Line>,
    chats: &[Chat],
    archived: &Archived,
    saved: &Saved,
    open: bool,
) {
    let roots = hidden_roots(chats, &saved.hidden);
    if roots.is_empty() {
        return;
    }
    lines.push(Line::Hidden(HiddenLine {
        chats: roots.len(),
        open,
    }));
    if !open {
        return;
    }
    for id in roots {
        let (chat, stand_in) = match chats.iter().find(|chat| chat.id == id) {
            Some(chat) => (chat.clone(), false),
            None => (stand_in(archived, &id, None, 0), true),
        };
        lines.push(Line::Chat(Box::new(ChatLine {
            chat,
            stand_in,
            depth: 0,
            rails: Vec::new(),
            last: false,
            fold: Fold::None,
            pinned: false,
            hidden: true,
        })));
    }
}

/// The grey chat a stand-in row shows for a missing chat.
pub fn stand_in(archived: &Archived, id: &str, folder: Option<String>, created: u64) -> Chat {
    Chat {
        id: id.to_string(),
        title: stand_in_title(archived, id),
        folder,
        branch: None,
        model: None,
        parent: None,
        created,
        mark: Mark::Unknown,
        since: None,
        jobs: 0,
        window: None,
        text: None,
        full: None,
        fork: None,
    }
}

/// How many listed chats have each counted mark, in the header's order;
/// marks with no chat are left out.
pub fn counts(chats: &[Chat], only_open: bool) -> Vec<(Mark, usize)> {
    Mark::COUNTED
        .iter()
        .map(|mark| {
            let count = chats
                .iter()
                .filter(|chat| !only_open || chat.window.is_some())
                .filter(|chat| chat.mark == *mark)
                .count();
            (*mark, count)
        })
        .filter(|(_, count)| *count > 0)
        .collect()
}
