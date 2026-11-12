//! What ClaudeDeck lists, worked out without any drawing: each chat's
//! status mark, the groups by project folder, the fork tree inside a group,
//! the roll-up of a folded chat or group, and the line of cold chats.
//! Rows never move by themselves: groups go by name, chats by the time
//! they were made.

use std::collections::{HashMap, HashSet};

use crate::core::claude_sessions::{self, Age, Session, Status};
use crate::core::claude_windows::ChatWindow;

use super::labels::fork_line;
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
    Background,
    Idle(Age),
    Stopped,
    Failed,
    Unknown,
}

impl Mark {
    /// Most urgent first, as on the design page. A folded chat or group
    /// shows the most urgent mark inside it.
    pub const URGENCY: [Mark; 9] = [
        Mark::NeedsApproval,
        Mark::AsksYou,
        Mark::DoneNotSeen,
        Mark::Working,
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
}

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
        None => (Mark::Unknown, None),
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

/// Which parts of the list the user folded or switched.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub only_open: bool,
    pub fold_cold: bool,
    /// Group keys (lower-case folders; "" for New) that are folded.
    pub folded_groups: HashSet<String>,
    /// Chats whose forks are folded.
    pub folded_chats: HashSet<String>,
    /// The cold chats are listed under their line.
    pub cold_open: bool,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupLine {
    pub key: String,
    pub name: String,
    pub chats: usize,
    pub folded: bool,
    /// The most urgent mark inside, shown while folded.
    pub roll: Mark,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColdLine {
    pub chats: usize,
    /// The youngest and the oldest, in whole days idle.
    pub days: (u64, u64),
    pub open: bool,
}

fn is_cold(chat: &Chat) -> bool {
    matches!(chat.mark, Mark::Idle(age) if age.is_cold())
}

/// The most urgent of these chats; on a tie the first.
fn most_urgent<'a>(chats: impl Iterator<Item = &'a Chat>) -> Option<&'a Chat> {
    chats.min_by_key(|chat| chat.mark.rank())
}

/// A folded chat shows the most urgent of its family; when that is a fork,
/// its line says so: "A fork needs approval: Waits for your OK".
fn roll_up(shown: &mut Chat, urgent: &Chat) {
    if urgent.id == shown.id {
        return;
    }
    shown.mark = urgent.mark;
    shown.since = urgent.since;
    let fork = fork_line(urgent.mark);
    shown.text = Some(match &urgent.text {
        Some(text) => format!("{fork}: {text}"),
        None => fork.to_string(),
    });
    shown.full.clone_from(&urgent.full);
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

struct Emit<'a> {
    tree: &'a Tree<'a>,
    view: &'a View,
    archived: &'a Archived,
    lines: Vec<Line>,
    /// Chats already drawn, so a loop of forks cannot draw forever.
    drawn: HashSet<usize>,
}

impl Emit<'_> {
    fn family(&mut self, family: &Family) {
        if let Some(top) = family.top {
            self.chat(top, 0, Vec::new(), false);
            return;
        }
        let kids: Vec<usize> = family
            .members
            .iter()
            .copied()
            .filter(|at| self.tree.chats[*at].parent.as_deref() == Some(family.missing.as_str()))
            .collect();
        let folded = self.view.folded_chats.contains(&family.missing);
        let mut stand_in = Chat {
            id: family.missing.clone(),
            title: stand_in_title(self.archived, &family.missing),
            folder: family.folder.clone(),
            branch: None,
            model: None,
            parent: None,
            created: family.created,
            mark: Mark::Unknown,
            since: None,
            jobs: 0,
            window: None,
            text: None,
            full: None,
        };
        let fold = if folded {
            let members = family.members.iter().map(|at| &self.tree.chats[*at]);
            if let Some(urgent) = most_urgent(members) {
                roll_up(&mut stand_in, urgent);
            }
            Fold::Folded(family.members.len())
        } else {
            Fold::Open
        };
        self.lines.push(Line::Chat(Box::new(ChatLine {
            chat: stand_in,
            stand_in: true,
            depth: 0,
            rails: Vec::new(),
            last: false,
            fold,
        })));
        if !folded {
            self.kids(&kids, 1, Vec::new());
        }
    }

    fn kids(&mut self, kids: &[usize], depth: usize, rails: Vec<bool>) {
        for (index, &kid) in kids.iter().enumerate() {
            let last = index + 1 == kids.len();
            self.chat(kid, depth, rails.clone(), last);
        }
    }

    fn chat(&mut self, at: usize, depth: usize, rails: Vec<bool>, last: bool) {
        if !self.drawn.insert(at) {
            return;
        }
        let chat = &self.tree.chats[at];
        let kids: Vec<usize> = self
            .tree
            .kids(&chat.id)
            .iter()
            .copied()
            .filter(|kid| !self.drawn.contains(kid))
            .collect();
        let folded = self.view.folded_chats.contains(&chat.id);
        let mut shown = chat.clone();
        let fold = if kids.is_empty() {
            Fold::None
        } else if folded {
            let mut below = Vec::new();
            self.tree
                .below(&chat.id, &mut HashSet::from([at]), &mut below);
            let all = std::iter::once(chat).chain(below.iter().map(|b| &self.tree.chats[*b]));
            if let Some(urgent) = most_urgent(all) {
                roll_up(&mut shown, urgent);
            }
            Fold::Folded(below.len())
        } else {
            Fold::Open
        };
        self.lines.push(Line::Chat(Box::new(ChatLine {
            chat: shown,
            stand_in: false,
            depth,
            rails: rails.clone(),
            last,
            fold,
        })));
        if !kids.is_empty() && !folded {
            let mut deeper = rails;
            if depth > 0 {
                deeper.push(!last);
            }
            self.kids(&kids, depth + 1, deeper);
        }
    }
}

/// One group before it becomes lines: its key, its name and its families.
type Group = (String, String, Vec<Family>);

/// The lines of the list, top to bottom: "New" first, then the groups by
/// name, then the line of cold chats. `archived` names the chats forks
/// came from that are no longer listed.
pub fn lines(chats: &[Chat], archived: &Archived, view: &View, now: u64) -> Vec<Line> {
    let tree = Tree::new(chats, view.only_open);
    let mut groups: Vec<Group> = Vec::new();
    let mut cold: Vec<Family> = Vec::new();
    for family in families(&tree) {
        let all_cold = family.members.iter().all(|at| is_cold(&chats[*at]));
        if view.fold_cold && all_cold {
            cold.push(family);
            continue;
        }
        let key = family
            .folder
            .as_deref()
            .map(str::to_lowercase)
            .unwrap_or_default();
        match groups.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, _, list)) => list.push(family),
            None => {
                let name = family
                    .folder
                    .as_deref()
                    .map_or(NEW_GROUP.to_string(), group_name);
                groups.push((key, name, vec![family]));
            }
        }
    }
    groups.sort_by(|a, b| {
        let rank = |g: &Group| (!g.0.is_empty(), g.1.to_lowercase(), g.0.clone());
        rank(a).cmp(&rank(b))
    });
    let mut emit = Emit {
        tree: &tree,
        view,
        archived,
        lines: Vec::new(),
        drawn: HashSet::new(),
    };
    for (key, name, mut families) in groups {
        families.sort_by_key(|family| family.created);
        let members: Vec<&Chat> = families
            .iter()
            .flat_map(|f| f.members.iter().map(|at| &chats[*at]))
            .collect();
        let roll = most_urgent(members.iter().copied()).map_or(Mark::Unknown, |c| c.mark);
        let folded = view.folded_groups.contains(&key);
        emit.lines.push(Line::Group(GroupLine {
            key,
            name,
            chats: members.len(),
            folded,
            roll,
        }));
        if !folded {
            for family in &families {
                emit.family(family);
            }
        }
    }
    if !cold.is_empty() {
        cold.sort_by_key(|family| family.created);
        let days: Vec<u64> = cold
            .iter()
            .flat_map(|f| f.members.iter())
            .map(|at| {
                chats[*at]
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
            open: view.cold_open,
        }));
        if view.cold_open {
            for family in &cold {
                emit.family(family);
            }
        }
    }
    emit.lines
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
