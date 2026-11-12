//! Where a dragged chat or group lands, worked out from the lines as they
//! are on screen, and where Alt+Up and Alt+Down move a chat. No drawing
//! here, so every rule is tested.

use super::edit::{GroupSpot, Spot};
use super::model::Line;

/// What a line on screen is, for dragging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Group {
        id: String,
        folded: bool,
    },
    /// A chat row inside a group: `key` is Some for the chat at the top of
    /// its family (the one that can be dragged), None for a fork.
    Chat {
        key: Option<String>,
    },
    /// The cold line, the hidden line and the rows under them.
    Other,
}

/// A line and where it is, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub kind: Kind,
    pub top: f32,
    pub bottom: f32,
}

/// A family on screen: its top chat and all its forks.
struct Span {
    key: String,
    top: f32,
    bottom: f32,
}

/// A group on screen, its header and everything under it.
struct GroupSpan {
    id: String,
    folded: bool,
    header: (f32, f32),
    bottom: f32,
    families: Vec<Span>,
}

fn spans(lines: &[Placed]) -> Vec<GroupSpan> {
    let mut groups: Vec<GroupSpan> = Vec::new();
    let mut inside = false;
    for line in lines {
        match &line.kind {
            Kind::Group { id, folded } => {
                inside = true;
                groups.push(GroupSpan {
                    id: id.clone(),
                    folded: *folded,
                    header: (line.top, line.bottom),
                    bottom: line.bottom,
                    families: Vec::new(),
                });
            }
            Kind::Chat { key } if inside => {
                let Some(group) = groups.last_mut() else {
                    continue;
                };
                group.bottom = line.bottom;
                match (key, group.families.last_mut()) {
                    (Some(key), _) => group.families.push(Span {
                        key: key.clone(),
                        top: line.top,
                        bottom: line.bottom,
                    }),
                    (None, Some(family)) => family.bottom = line.bottom,
                    (None, None) => {}
                }
            }
            Kind::Chat { .. } => {}
            Kind::Other => inside = false,
        }
    }
    groups
}

/// Where a chat dropped at height `y` lands, and the height of the line
/// that shows it. None over the cold or hidden lines.
pub fn chat_spot(lines: &[Placed], y: f32) -> Option<(Spot, f32)> {
    let groups = spans(lines);
    for group in &groups {
        if y < group.header.0 {
            break;
        }
        if y < group.header.1 {
            return Some(match group.families.first() {
                Some(first) if !group.folded => (Spot::Before(first.key.clone()), group.header.1),
                _ => (Spot::End(group.id.clone()), group.header.1),
            });
        }
        for (at, family) in group.families.iter().enumerate() {
            if y < family.top || y >= family.bottom {
                continue;
            }
            if y < (family.top + family.bottom) / 2.0 {
                return Some((Spot::Before(family.key.clone()), family.top));
            }
            return Some(match group.families.get(at + 1) {
                Some(next) => (Spot::Before(next.key.clone()), family.bottom),
                None => (Spot::End(group.id.clone()), family.bottom),
            });
        }
    }
    let last = groups.last()?;
    (y >= last.bottom && below_groups(lines, y)).then(|| (Spot::End(last.id.clone()), last.bottom))
}

/// Whether `y` is under the last group but above the cold and hidden
/// lines, or under everything when they are not there.
fn below_groups(lines: &[Placed], y: f32) -> bool {
    lines
        .iter()
        .find(|line| line.kind == Kind::Other)
        .is_none_or(|other| y < other.top)
}

/// Where a group dropped at height `y` lands, and the height of the line
/// that shows it.
pub fn group_spot(lines: &[Placed], y: f32) -> Option<(GroupSpot, f32)> {
    let groups = spans(lines);
    let first = groups.first()?;
    if y < first.header.0 {
        return Some((GroupSpot::Before(first.id.clone()), first.header.0));
    }
    for (at, group) in groups.iter().enumerate() {
        if y < group.header.0 || y >= group.bottom {
            continue;
        }
        if y < (group.header.0 + group.bottom) / 2.0 {
            return Some((GroupSpot::Before(group.id.clone()), group.header.0));
        }
        return Some(match groups.get(at + 1) {
            Some(next) => (GroupSpot::Before(next.id.clone()), group.bottom),
            None => (GroupSpot::End, group.bottom),
        });
    }
    let last = groups.last()?;
    (y >= last.bottom).then_some((GroupSpot::End, last.bottom))
}

/// Where Alt+Up (`up`) or Alt+Down moves the chat `key`: past the chat
/// above or below it, and from the edge of a group into the next group.
pub fn step_chat(lines: &[Placed], key: &str, up: bool) -> Option<Spot> {
    let groups = spans(lines);
    let (g, at) = groups.iter().enumerate().find_map(|(g, group)| {
        group
            .families
            .iter()
            .position(|family| family.key == key)
            .map(|at| (g, at))
    })?;
    let group = &groups[g];
    if up {
        if at > 0 {
            return Some(Spot::Before(group.families[at - 1].key.clone()));
        }
        let above = groups.get(g.checked_sub(1)?)?;
        return Some(Spot::End(above.id.clone()));
    }
    if at + 1 < group.families.len() {
        return Some(match group.families.get(at + 2) {
            Some(next) => Spot::Before(next.key.clone()),
            None => Spot::End(group.id.clone()),
        });
    }
    let below = groups.get(g + 1)?;
    Some(match below.families.first() {
        Some(first) if !below.folded => Spot::Before(first.key.clone()),
        _ => Spot::End(below.id.clone()),
    })
}

/// What each line is for dragging, top to bottom, without its place.
pub fn kinds(lines: &[Line]) -> Vec<Kind> {
    let mut in_group = false;
    lines
        .iter()
        .map(|line| match line {
            Line::Group(group) => {
                in_group = true;
                Kind::Group {
                    id: group.key.clone(),
                    folded: group.folded,
                }
            }
            Line::Chat(chat) if in_group && !chat.hidden => Kind::Chat {
                key: (chat.depth == 0).then(|| chat.chat.id.clone()),
            },
            Line::Chat(_) => Kind::Other,
            Line::Cold(_) | Line::Hidden(_) => {
                in_group = false;
                Kind::Other
            }
        })
        .collect()
}
