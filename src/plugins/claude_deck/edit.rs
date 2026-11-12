//! The changes the user makes to the arrangement: move a chat or a group,
//! pin, hide, fold, rename, colour, a new group, and ungroup. Each one only
//! changes ClaudeDeck's own list. None of them loses a chat: a chat leaves
//! a group only by going into another one.

use std::collections::HashMap;

use super::arrange::{home, inbox, next_id, pinned_first, Family, NEW_ID};
use super::model::group_name;
use super::store::{Colour, Group, Saved};

/// The name a new group starts with; it opens for renaming at once.
pub const NEW_GROUP_NAME: &str = "New group";

/// Where a moved chat lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    /// Just above this chat, in its group.
    Before(String),
    /// At the end of this group.
    End(String),
}

/// Where a moved group lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupSpot {
    Before(String),
    End,
}

/// The group and place of a saved chat.
pub fn find(saved: &Saved, key: &str) -> Option<(usize, usize)> {
    saved
        .groups
        .iter()
        .enumerate()
        .find_map(|(g, group)| group.chats.iter().position(|k| k == key).map(|at| (g, at)))
}

fn group_at(saved: &Saved, id: &str) -> Option<usize> {
    saved.groups.iter().position(|group| group.id == id)
}

/// Moves a chat (and the forks under it) to a spot. Above a pinned chat it
/// is pinned too, anywhere else it is not, so pinned chats stay on top.
pub fn place_chat(saved: &mut Saved, key: &str, spot: &Spot) {
    let target_ok = match spot {
        Spot::Before(other) => other != key && find(saved, other).is_some(),
        Spot::End(group) => group_at(saved, group).is_some(),
    };
    let Some((from, at)) = find(saved, key).filter(|_| target_ok) else {
        return;
    };
    saved.groups[from].chats.remove(at);
    let pinned = match spot {
        Spot::Before(other) => {
            let Some((g, at)) = find(saved, other) else {
                return;
            };
            saved.groups[g].chats.insert(at, key.to_string());
            saved.pinned.contains(other)
        }
        Spot::End(group) => {
            let Some(g) = group_at(saved, group) else {
                return;
            };
            saved.groups[g].chats.push(key.to_string());
            false
        }
    };
    if pinned {
        saved.pinned.insert(key.to_string());
    } else {
        saved.pinned.remove(key);
    }
    pinned_first(saved);
}

/// Pins a chat to the top of its group, or unpins it (it then goes just
/// below the pinned ones).
pub fn pin(saved: &mut Saved, key: &str, on: bool) {
    let Some((g, at)) = find(saved, key) else {
        return;
    };
    if on {
        let key = saved.groups[g].chats.remove(at);
        saved.groups[g].chats.insert(0, key.clone());
        saved.pinned.insert(key);
    } else {
        saved.pinned.remove(key);
    }
    pinned_first(saved);
}

/// Hides a chat (and everything below it) from the list, or shows it again.
pub fn hide(saved: &mut Saved, id: &str, on: bool) {
    if on {
        saved.hidden.insert(id.to_string());
    } else {
        saved.hidden.remove(id);
    }
}

pub fn fold_chat(saved: &mut Saved, id: &str) {
    if !saved.folded_chats.remove(id) {
        saved.folded_chats.insert(id.to_string());
    }
}

pub fn fold_group(saved: &mut Saved, id: &str) {
    if let Some(g) = group_at(saved, id) {
        saved.groups[g].folded = !saved.groups[g].folded;
    }
}

pub fn fold_all(saved: &mut Saved, folded: bool) {
    for group in &mut saved.groups {
        group.folded = folded;
    }
}

/// A new name; an empty one changes nothing.
pub fn rename(saved: &mut Saved, id: &str, name: &str) {
    let name = name.trim();
    let Some(g) = group_at(saved, id) else {
        return;
    };
    if name.is_empty() || saved.groups[g].name == name {
        return;
    }
    saved.groups[g].name = name.to_string();
    saved.groups[g].keep = true;
}

pub fn colour(saved: &mut Saved, id: &str, colour: Option<Colour>) {
    if let Some(g) = group_at(saved, id) {
        saved.groups[g].colour = colour;
        saved.groups[g].keep = true;
    }
}

/// Makes an empty group below `after` (at the end without one) and moves
/// the chat `with` into it. Returns the new group's id.
pub fn new_group(saved: &mut Saved, after: Option<&str>, with: Option<&str>) -> String {
    let id = next_id(saved);
    let at = after
        .and_then(|after| group_at(saved, after))
        .map_or(saved.groups.len(), |g| g + 1);
    saved.groups.insert(
        at,
        Group {
            id: id.clone(),
            name: NEW_GROUP_NAME.to_string(),
            keep: true,
            ..Group::default()
        },
    );
    if let Some(key) = with {
        place_chat(saved, key, &Spot::End(id.clone()));
    }
    id
}

/// Moves a group to a spot among the groups.
pub fn place_group(saved: &mut Saved, id: &str, spot: &GroupSpot) {
    if matches!(spot, GroupSpot::Before(other) if other == id) {
        return;
    }
    let Some(from) = group_at(saved, id) else {
        return;
    };
    let group = saved.groups.remove(from);
    let at = match spot {
        GroupSpot::Before(other) => group_at(saved, other).unwrap_or(from),
        GroupSpot::End => saved.groups.len(),
    };
    saved.groups.insert(at.min(saved.groups.len()), group);
}

/// Moves a group one place up or down.
pub fn step_group(saved: &mut Saved, id: &str, up: bool) {
    let Some(at) = group_at(saved, id) else {
        return;
    };
    let other = if up { at.checked_sub(1) } else { Some(at + 1) };
    if let Some(other) = other.filter(|o| *o < saved.groups.len()) {
        saved.groups.swap(at, other);
    }
}

/// Removes a group. Each of its chats goes back to its project folder: to
/// the group that holds that folder's chats, else to a group made for the
/// folder where this one was. A chat with no folder goes to "New"; in
/// "New" itself such a chat stays, so the group stays with it.
pub fn ungroup(saved: &mut Saved, id: &str, families: &[Family]) {
    let Some(mut at) = group_at(saved, id) else {
        return;
    };
    let group = saved.groups.remove(at);
    let originals: HashMap<&str, &str> = families
        .iter()
        .filter_map(|f| Some((f.key.as_str(), f.folder.as_deref()?)))
        .collect();
    let lowered: HashMap<&str, String> = originals
        .iter()
        .map(|(key, folder)| (*key, folder.to_lowercase()))
        .collect();
    let mut stay = Vec::new();
    let mut made = 0;
    for key in group.chats {
        let folder = lowered.get(key.as_str()).cloned();
        let to = match home(saved, &lowered, folder.as_deref()) {
            Some(to) => to,
            None => match originals.get(key.as_str()) {
                Some(original) => {
                    let new = Group {
                        id: next_id(saved),
                        name: group_name(original),
                        folder,
                        ..Group::default()
                    };
                    saved.groups.insert(at + made, new);
                    made += 1;
                    at + made - 1
                }
                None if group.id == NEW_ID => {
                    stay.push(key);
                    continue;
                }
                None => {
                    let before = saved.groups.len();
                    let to = inbox(saved);
                    if saved.groups.len() > before {
                        // "New" was made at the top: the rest moved down.
                        at += 1;
                    }
                    to
                }
            },
        };
        saved.groups[to].chats.push(key);
    }
    if !stay.is_empty() {
        saved.groups.insert(
            at.min(saved.groups.len()),
            Group {
                chats: stay,
                ..group
            },
        );
    }
    pinned_first(saved);
}
