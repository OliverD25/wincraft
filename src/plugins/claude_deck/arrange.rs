//! Merges the user's saved arrangement with the chats listed now. A chat
//! the arrangement does not know yet goes to the end of the group that
//! already holds chats of its project folder; a fork hangs under its chat;
//! a chat of a project no group holds goes to "New" at the top. The first
//! time, there are no groups yet, so each project folder gets its own.
//! A saved chat that is no longer listed keeps its place for 30 days, so it
//! comes back where it was. Nothing moves by itself otherwise.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::model::{group_name, Chat, NEW_GROUP};
use super::store::{Group, Saved, KEEP_GONE_MS, VERSION};
use super::tree::{families, Tree};

/// The id of the group "New", where chats of an unknown project go.
pub const NEW_ID: &str = "new";

/// A family as the arrangement sees it: the chat at its top (or the id of
/// the missing chat its forks came from), its project folder, and when it
/// was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub key: String,
    pub folder: Option<String>,
    pub created: u64,
}

/// The families of these chats, oldest first.
pub fn families_of(chats: &[Chat]) -> Vec<Family> {
    let tree = Tree::new(chats, false);
    families(&tree)
        .into_iter()
        .map(|family| Family {
            key: match family.top {
                Some(at) => chats[at].id.clone(),
                None => family.missing.clone(),
            },
            folder: family.folder,
            created: family.created,
        })
        .collect()
}

fn lower(folder: &Option<String>) -> Option<String> {
    folder.as_deref().map(str::to_lowercase)
}

/// The saved arrangement with every listed family placed.
pub fn arrange(saved: &Saved, chats: &[Chat], now: u64) -> Saved {
    let found = families_of(chats);
    let mut out = saved.clone();
    out.version = out.version.max(VERSION);
    let live: HashSet<&str> = chats
        .iter()
        .map(|chat| chat.id.as_str())
        .chain(found.iter().map(|family| family.key.as_str()))
        .collect();
    forget_gone(&mut out, &live, now);
    let first = out.groups.is_empty();
    let folders: HashMap<&str, String> = found
        .iter()
        .filter_map(|family| Some((family.key.as_str(), lower(&family.folder)?)))
        .collect();
    let placed: HashSet<String> = out
        .groups
        .iter()
        .flat_map(|group| group.chats.iter().cloned())
        .collect();
    for family in found.iter().filter(|family| !placed.contains(&family.key)) {
        let folder = lower(&family.folder);
        let at = match home(&out, &folders, folder.as_deref()) {
            Some(at) => at,
            None if first && folder.is_some() => {
                let name = family.folder.as_deref().map_or(String::new(), group_name);
                out.groups.push(Group {
                    id: next_id(&out),
                    name,
                    folder,
                    ..Group::default()
                });
                out.groups.len() - 1
            }
            None => inbox(&mut out),
        };
        out.groups[at].chats.push(family.key.clone());
    }
    if first {
        out.groups.sort_by(|a, b| {
            let rank = |g: &Group| (g.id != NEW_ID, g.name.to_lowercase(), g.folder.clone());
            rank(a).cmp(&rank(b))
        });
    }
    out.groups
        .retain(|group| group.keep || !group.chats.is_empty());
    pinned_first(&mut out);
    out
}

/// The group a family of this folder goes to: one that already holds a
/// listed chat of the folder, else the one made for the folder.
pub fn home(saved: &Saved, folders: &HashMap<&str, String>, folder: Option<&str>) -> Option<usize> {
    let folder = folder?;
    saved
        .groups
        .iter()
        .position(|group| {
            group
                .chats
                .iter()
                .any(|key| folders.get(key.as_str()).map(String::as_str) == Some(folder))
        })
        .or_else(|| {
            saved
                .groups
                .iter()
                .position(|group| group.folder.as_deref() == Some(folder))
        })
}

/// The place of "New", made at the top when it is not there.
pub fn inbox(saved: &mut Saved) -> usize {
    if let Some(at) = saved.groups.iter().position(|group| group.id == NEW_ID) {
        return at;
    }
    saved.groups.insert(
        0,
        Group {
            id: NEW_ID.to_string(),
            name: NEW_GROUP.to_string(),
            ..Group::default()
        },
    );
    0
}

/// A group id no group has yet: "g1", "g2", ...
pub fn next_id(saved: &Saved) -> String {
    (1..)
        .map(|n| format!("g{n}"))
        .find(|id| saved.groups.iter().all(|group| group.id != *id))
        .unwrap_or_default()
}

/// Notes when saved chats went missing, and drops those missing for
/// longer than `KEEP_GONE_MS` from every part of the arrangement.
fn forget_gone(saved: &mut Saved, live: &HashSet<&str>, now: u64) {
    let tracked: BTreeSet<String> = saved
        .groups
        .iter()
        .flat_map(|group| group.chats.iter().cloned())
        .chain(saved.hidden.iter().cloned())
        .chain(saved.pinned.iter().cloned())
        .chain(saved.folded_chats.iter().cloned())
        .collect();
    saved.gone.retain(|id, _| tracked.contains(id));
    for id in &tracked {
        if live.contains(id.as_str()) {
            saved.gone.remove(id);
        } else {
            saved.gone.entry(id.clone()).or_insert(now);
        }
    }
    let expired: HashSet<String> = saved
        .gone
        .iter()
        .filter(|(_, since)| now.saturating_sub(**since) >= KEEP_GONE_MS)
        .map(|(id, _)| id.clone())
        .collect();
    if expired.is_empty() {
        return;
    }
    for group in &mut saved.groups {
        group.chats.retain(|key| !expired.contains(key));
    }
    saved.hidden.retain(|id| !expired.contains(id));
    saved.pinned.retain(|id| !expired.contains(id));
    saved.folded_chats.retain(|id| !expired.contains(id));
    saved.gone.retain(|id, _| !expired.contains(id));
}

/// Pinned chats stay at the top of their group, in their own order.
pub fn pinned_first(saved: &mut Saved) {
    for group in &mut saved.groups {
        let (mut pinned, rest): (Vec<String>, Vec<String>) = group
            .chats
            .drain(..)
            .partition(|key| saved.pinned.contains(key));
        pinned.extend(rest);
        group.chats = pinned;
    }
}

/// The chats left out of the list: each hidden chat and every chat below
/// it, by place in `chats`.
pub fn hidden_chats(chats: &[Chat], hidden: &BTreeSet<String>) -> HashSet<usize> {
    if hidden.is_empty() {
        return HashSet::new();
    }
    let by_id: HashMap<&str, &Chat> = chats.iter().map(|chat| (chat.id.as_str(), chat)).collect();
    (0..chats.len())
        .filter(|&at| {
            let mut id = chats[at].id.as_str();
            for _ in 0..=chats.len() {
                if hidden.contains(id) {
                    return true;
                }
                let Some(parent) = by_id.get(id).and_then(|chat| chat.parent.as_deref()) else {
                    return false;
                };
                if parent == id {
                    return false;
                }
                id = parent;
            }
            false
        })
        .collect()
}

/// The hidden chats the "hidden chats" line lists: those hidden by the user
/// that are still there, a chat or the missing chat forks came from.
pub fn hidden_roots(chats: &[Chat], hidden: &BTreeSet<String>) -> Vec<String> {
    hidden
        .iter()
        .filter(|id| {
            chats
                .iter()
                .any(|chat| chat.id == **id || chat.parent.as_deref() == Some(id.as_str()))
        })
        .cloned()
        .collect()
}
