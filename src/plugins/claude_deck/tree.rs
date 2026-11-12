//! How ClaudeDeck's chats hang together: which are listed (with "Only open
//! windows", an open chat keeps the chats above it), each chat's forks in
//! the order they were made, and the families a group is made of.

use std::collections::{HashMap, HashSet};

use super::model::Chat;

/// The chats that are listed and how they hang together.
pub(super) struct Tree<'a> {
    pub chats: &'a [Chat],
    pub by_id: HashMap<&'a str, usize>,
    children: HashMap<String, Vec<usize>>,
    pub kept: Vec<bool>,
}

impl<'a> Tree<'a> {
    pub fn new(chats: &'a [Chat], only_open: bool) -> Self {
        let by_id: HashMap<&str, usize> = chats
            .iter()
            .enumerate()
            .map(|(at, chat)| (chat.id.as_str(), at))
            .collect();
        let mut kept: Vec<bool> = chats
            .iter()
            .map(|chat| !only_open || chat.window.is_some())
            .collect();
        // The chats above an open one stay, so its tree stays whole.
        for start in 0..chats.len() {
            if !kept[start] {
                continue;
            }
            let mut at = start;
            for _ in 0..chats.len() {
                let Some(parent) = parent_of(chats, &by_id, at) else {
                    break;
                };
                if kept[parent] {
                    break;
                }
                kept[parent] = true;
                at = parent;
            }
        }
        let mut children: HashMap<String, Vec<usize>> = HashMap::new();
        for (at, chat) in chats.iter().enumerate() {
            if !kept[at] {
                continue;
            }
            if let Some(parent) = chat.parent.as_deref() {
                if parent != chat.id && !parent.is_empty() {
                    children.entry(parent.to_string()).or_default().push(at);
                }
            }
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| order(&chats[*a], &chats[*b]));
        }
        Tree {
            chats,
            by_id,
            children,
            kept,
        }
    }

    /// Whether following parents from `at` comes round to a chat seen
    /// before: such a chat is treated as a top.
    fn in_loop(&self, at: usize) -> bool {
        let mut seen = HashSet::from([at]);
        let mut now = at;
        while let Some(parent) = parent_of(self.chats, &self.by_id, now) {
            if !seen.insert(parent) {
                return true;
            }
            now = parent;
        }
        false
    }

    pub fn kids(&self, id: &str) -> &[usize] {
        self.children.get(id).map_or(&[], |list| list.as_slice())
    }

    /// Every chat below `id`, depth first, each once.
    pub fn below(&self, id: &str, seen: &mut HashSet<usize>, into: &mut Vec<usize>) {
        for &kid in self.kids(id) {
            if seen.insert(kid) {
                into.push(kid);
                self.below(&self.chats[kid].id, seen, into);
            }
        }
    }
}

/// The listed parent of the chat at `at`, if it has one.
fn parent_of(chats: &[Chat], by_id: &HashMap<&str, usize>, at: usize) -> Option<usize> {
    let parent = chats[at].parent.as_deref()?;
    by_id.get(parent).copied().filter(|p| *p != at)
}

/// A family: a chat at the top (or a stand-in for its missing parent) and
/// everything below it.
pub(super) struct Family {
    /// The top chat, or None for a stand-in.
    pub top: Option<usize>,
    /// The missing parent's id, for a stand-in.
    pub missing: String,
    pub members: Vec<usize>,
    pub created: u64,
    pub folder: Option<String>,
}

fn order(a: &Chat, b: &Chat) -> std::cmp::Ordering {
    a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id))
}

pub(super) fn families(tree: &Tree) -> Vec<Family> {
    let mut families: Vec<Family> = Vec::new();
    let mut stand_ins: HashMap<String, usize> = HashMap::new();
    let mut placed: HashSet<usize> = HashSet::new();
    let mut tops: Vec<usize> = (0..tree.chats.len())
        .filter(|at| tree.kept[*at])
        .filter(|at| parent_of(tree.chats, &tree.by_id, *at).is_none() || tree.in_loop(*at))
        .collect();
    tops.sort_by(|a, b| order(&tree.chats[*a], &tree.chats[*b]));
    for top in tops {
        if !placed.insert(top) {
            continue;
        }
        let chat = &tree.chats[top];
        let missing = chat
            .parent
            .clone()
            .filter(|parent| !parent.is_empty() && *parent != chat.id)
            .filter(|parent| !tree.by_id.contains_key(parent.as_str()));
        let mut members = vec![top];
        tree.below(&chat.id, &mut placed, &mut members);
        match missing {
            Some(parent) => match stand_ins.get(&parent) {
                Some(&index) => families[index].members.extend(members),
                None => {
                    stand_ins.insert(parent.clone(), families.len());
                    families.push(Family {
                        top: None,
                        missing: parent,
                        members,
                        created: chat.created,
                        folder: chat.folder.clone(),
                    });
                }
            },
            None => families.push(Family {
                top: Some(top),
                missing: String::new(),
                members,
                created: chat.created,
                folder: chat.folder.clone(),
            }),
        }
    }
    families
}
