//! How one family becomes lines of the list: the chat at the top (or a
//! grey stand-in for its missing chat), its forks with their tree lines,
//! and the roll-up of a folded chat.

use std::collections::{BTreeSet, HashSet};

use super::labels::fork_line;
use super::model::{stand_in, Archived, Chat, ChatLine, Fold, Line, Mark};
use super::tree::{Family, Tree};

pub(super) fn is_cold(chat: &Chat) -> bool {
    matches!(chat.mark, Mark::Idle(age) if age.is_cold())
}

/// The most urgent of these chats; on a tie the first.
pub(super) fn most_urgent<'a>(chats: impl Iterator<Item = &'a Chat>) -> Option<&'a Chat> {
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

pub(super) struct Emit<'a> {
    pub tree: &'a Tree<'a>,
    /// Chats whose forks are folded.
    pub folded: &'a BTreeSet<String>,
    pub pinned: &'a BTreeSet<String>,
    pub archived: &'a Archived,
    pub lines: Vec<Line>,
    /// Chats already drawn, so a loop of forks cannot draw forever.
    pub drawn: HashSet<usize>,
}

impl Emit<'_> {
    pub fn family(&mut self, family: &Family) {
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
        let folded = self.folded.contains(&family.missing);
        let mut stand_in = stand_in(
            self.archived,
            &family.missing,
            family.folder.clone(),
            family.created,
        );
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
            pinned: self.pinned.contains(&family.missing),
            hidden: false,
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
        let folded = self.folded.contains(&chat.id);
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
            pinned: depth == 0 && self.pinned.contains(&chat.id),
            hidden: false,
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
