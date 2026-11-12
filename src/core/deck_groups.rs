//! WindowDeck's groups of related windows: a name and the shown names of
//! its windows, inside one app's list. A window belongs to a group while its
//! shown name (its WindowNamer name, else its title) is one of the group's
//! names, so a window that opens again with that name joins by itself, the
//! way WindowNamer finds a window again. Only the user makes a group
//! (Principle 1); nothing here groups windows by itself.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::config;

const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckGroup {
    /// The taskbar group (WindowDeck's app key) the group lives in.
    pub app: String,
    pub name: String,
    /// The shown names of its windows, in order.
    pub members: Vec<String>,
}

impl DeckGroup {
    pub fn has(&self, label: &str) -> bool {
        self.members.iter().any(|member| member == label)
    }
}

/// The group of `app` that `label` belongs to.
pub fn group_of<'a>(groups: &'a [DeckGroup], app: &str, label: &str) -> Option<&'a DeckGroup> {
    groups
        .iter()
        .find(|group| group.app == app && group.has(label))
}

/// The groups of one app.
pub fn of_app<'a>(groups: &'a [DeckGroup], app: &str) -> impl Iterator<Item = &'a DeckGroup> {
    let app = app.to_string();
    groups.iter().filter(move |group| group.app == app)
}

/// A name no other group of `app` has: `wanted`, or `wanted 2`, `wanted 3`.
fn free_name(groups: &[DeckGroup], app: &str, wanted: &str, skip: Option<&str>) -> String {
    let taken = |name: &str| {
        of_app(groups, app).any(|group| group.name == name && Some(group.name.as_str()) != skip)
    };
    if !taken(wanted) {
        return wanted.to_string();
    }
    (2..)
        .map(|n| format!("{wanted} {n}"))
        .find(|name| !taken(name))
        .unwrap_or_default()
}

/// Takes `label` out of every group of `app`, and drops a group left with no
/// names.
fn take_out(groups: &mut Vec<DeckGroup>, app: &str, label: &str) {
    for group in groups.iter_mut().filter(|group| group.app == app) {
        group.members.retain(|member| member != label);
    }
    groups.retain(|group| !group.members.is_empty());
}

/// A new group of `app` named `name` (made unique) with `labels`, which
/// leave any group they were in: a window is in one group at a time.
/// Returns the name it got, or None when there is no name or fewer than two
/// windows.
pub fn make(
    groups: &mut Vec<DeckGroup>,
    app: &str,
    name: &str,
    labels: &[String],
) -> Option<String> {
    let name = name.trim();
    let mut members: Vec<String> = Vec::new();
    for label in labels {
        if !members.contains(label) {
            members.push(label.clone());
        }
    }
    if name.is_empty() || members.len() < 2 {
        return None;
    }
    for label in &members {
        take_out(groups, app, label);
    }
    let name = free_name(groups, app, name, None);
    groups.push(DeckGroup {
        app: app.to_string(),
        name: name.clone(),
        members,
    });
    Some(name)
}

/// Puts `label` into the group `name` of `app`, at the end. False when
/// there is no such group.
pub fn add(groups: &mut Vec<DeckGroup>, app: &str, name: &str, label: &str) -> bool {
    if !of_app(groups, app).any(|group| group.name == name) {
        return false;
    }
    if group_of(groups, app, label).is_some_and(|group| group.name == name) {
        return true;
    }
    take_out(groups, app, label);
    match groups
        .iter_mut()
        .find(|group| group.app == app && group.name == name)
    {
        Some(group) => {
            group.members.push(label.to_string());
            true
        }
        // The group held only this window.
        None => false,
    }
}

/// Takes `label` out of its group of `app`.
pub fn remove(groups: &mut Vec<DeckGroup>, app: &str, label: &str) {
    take_out(groups, app, label);
}

/// Gives the group a new name (made unique) and returns it. None when the
/// name is empty or there is no such group.
pub fn rename(groups: &mut [DeckGroup], app: &str, old: &str, new: &str) -> Option<String> {
    let new = new.trim();
    if new.is_empty() {
        return None;
    }
    let free = free_name(groups, app, new, Some(old));
    match groups
        .iter_mut()
        .find(|group| group.app == app && group.name == old)
    {
        Some(group) => {
            group.name = free.clone();
            Some(free)
        }
        None => None,
    }
}

/// A window of the group got a new shown name: the group keeps it.
pub fn rename_member(groups: &mut [DeckGroup], app: &str, old: &str, new: &str) {
    for group in groups.iter_mut().filter(|group| group.app == app) {
        for member in group.members.iter_mut().filter(|member| *member == old) {
            *member = new.to_string();
        }
    }
}

/// A grouped window now shows `new` where it showed `old`: the group keeps
/// it under the new name. Nothing changes when the name is the same, when
/// no group of `app` has `old`, or when a group of `app` already has `new`
/// (another window may have that name). True when a group changed.
pub fn follow_rename(groups: &mut [DeckGroup], app: &str, old: &str, new: &str) -> bool {
    let of_app = |group: &&DeckGroup| group.app == app;
    if old == new
        || new.trim().is_empty()
        || !groups.iter().filter(of_app).any(|group| group.has(old))
        || groups.iter().filter(of_app).any(|group| group.has(new))
    {
        return false;
    }
    rename_member(groups, app, old, new);
    true
}

/// Drops the group; its windows stay open as they are.
pub fn ungroup(groups: &mut Vec<DeckGroup>, app: &str, name: &str) {
    groups.retain(|group| !(group.app == app && group.name == name));
}

fn is_separator(c: char) -> bool {
    !c.is_alphanumeric()
}

/// The start that all `names` share, cut at the end of a word and without
/// the separators after it: "NewsHQ - My Tasks" and "NewsHQ - design" share
/// "NewsHQ". Empty when they share no whole word.
pub fn shared_start(names: &[&str]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut end = first.len();
    for name in &names[1..] {
        let common = first
            .char_indices()
            .zip(name.chars())
            .find(|((_, a), b)| a != b)
            .map_or(first.len().min(name.len()), |((at, _), _)| at);
        end = end.min(common);
    }
    // A cut inside a word goes back to the word's start.
    let whole = |name: &str| name[end..].chars().next().is_none_or(is_separator);
    let start = if names.iter().all(|name| whole(name)) {
        &first[..end]
    } else {
        let back = first[..end]
            .char_indices()
            .rev()
            .find(|(_, c)| is_separator(*c))
            .map_or(0, |(at, _)| at);
        &first[..back]
    };
    keep_brackets_whole(start)
}

const BRACKETS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// `start` without the separators at its end, but never with half a
/// bracket: "Rust (fork) -" is "Rust (fork)", and "Rust (fork" is "Rust".
fn keep_brackets_whole(start: &str) -> String {
    let trimmed = start.trim_end_matches(is_separator);
    let Some(open) = unclosed(trimmed) else {
        return trimmed.to_string();
    };
    let close = BRACKETS.iter().find(|(o, _)| *o == open.1).map(|b| b.1);
    let rest = &start[trimmed.len()..];
    if rest.starts_with(|c| Some(c) == close) {
        let with = &start[..trimmed.len() + rest.chars().next().map_or(0, char::len_utf8)];
        if unclosed(with).is_none() {
            return with.to_string();
        }
    }
    keep_brackets_whole(&trimmed[..open.0])
}

/// The last bracket in `text` that opens and is not closed, with its place.
fn unclosed(text: &str) -> Option<(usize, char)> {
    let mut open: Vec<(usize, char)> = Vec::new();
    for (at, c) in text.char_indices() {
        if BRACKETS.iter().any(|(o, _)| *o == c) {
            open.push((at, c));
        } else if let Some((o, _)) = BRACKETS.iter().find(|(_, cl)| *cl == c) {
            if open.last().is_some_and(|(_, last)| last == o) {
                open.pop();
            }
        }
    }
    open.pop()
}

/// The rest of a name after the shared start, without the separators at its
/// front. A bracket that holds the whole first word loses both its halves:
/// "(fork) - Design" is "fork - Design", never "fork) - Design".
fn after_start(rest: &str) -> String {
    let rest =
        rest.trim_start_matches(|c| is_separator(c) && !BRACKETS.iter().any(|(o, _)| *o == c));
    let Some(open) = rest.chars().next() else {
        return String::new();
    };
    let Some(&(_, close)) = BRACKETS.iter().find(|(o, _)| *o == open) else {
        return rest.to_string();
    };
    let inner = &rest[open.len_utf8()..];
    match inner.find(close) {
        Some(end)
            if end > 0
                && !inner[..end].contains(char::is_whitespace)
                && !inner[..end].contains(open) =>
        {
            format!("{}{}", &inner[..end], &inner[end + close.len_utf8()..])
        }
        _ => rest.to_string(),
    }
}

/// A window's name without the start it shares with the group's name:
/// "NewsHQ - My Tasks" in "NewsHQ" is "My Tasks". The whole name when
/// nothing would be left.
pub fn short_name(label: &str, group: &str) -> String {
    let start = shared_start(&[label, group]);
    let rest = after_start(&label[start.len()..]);
    if start.is_empty() || rest.trim().is_empty() {
        label.to_string()
    } else {
        rest
    }
}

/// The name the name box suggests for a new group of `labels`: the start
/// they share, else the app's name.
pub fn suggested_name(labels: &[&str], app_label: &str) -> String {
    let start = shared_start(labels);
    if start.is_empty() {
        app_label.to_string()
    } else {
        start
    }
}

#[derive(Serialize, Deserialize)]
struct GroupsFile {
    version: u32,
    groups: Vec<DeckGroup>,
}

pub fn parse(text: &str) -> Result<Vec<DeckGroup>, String> {
    serde_json::from_str::<GroupsFile>(config::strip_bom(text))
        .map(|file| file.groups)
        .map_err(|err| err.to_string())
}

pub fn to_json(groups: &[DeckGroup]) -> String {
    let file = GroupsFile {
        version: VERSION,
        groups: groups.to_vec(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// No file means no groups yet. A file that cannot be read is logged and
/// treated as empty; the next save replaces it.
pub fn load(path: &Path) -> Vec<DeckGroup> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    match parse(&text) {
        Ok(groups) => groups,
        Err(err) => {
            log::warn!(
                "{} cannot be read ({err}); starting with no groups, the next save replaces it",
                path.display()
            );
            Vec::new()
        }
    }
}

pub fn save(path: &Path, groups: &[DeckGroup]) -> Result<(), String> {
    config::write_atomic(path, &to_json(groups))
}

#[cfg(test)]
mod tests;
