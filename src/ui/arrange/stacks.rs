//! WindowDeck's groups of related windows in the strip ("Group related
//! windows", on by default). A closed group is one stack card with one
//! number; its number, Enter or a click opens it in place, where its
//! windows are numbered 1, 2, 3 and the other cards dim. The groups
//! themselves live in `core::deck_groups` and are saved by WindowDeck.
//!
//! The strip keeps its data folded: each closed group's windows are one
//! card (`fold`), so every view, number and key treats a group as one
//! window. The rest of the strip calls in here at a few points: when data
//! arrives, to draw a card's marks, and before and after each frame.

use super::*;
use crate::core::deck_groups::{self, DeckGroup};

mod act;
mod dialogs;
mod fold;
mod menu;
mod paint;
pub(super) use act::*;
pub(super) use fold::{Found, Kind};
pub(super) use menu::*;
pub(super) use paint::*;

/// The other cards while a group is open.
const DIMMED: f32 = 0.35;

/// A question or a name box on top of the strip.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Dialog {
    /// "Group N windows": a name for a new group of these windows.
    Name {
        app: String,
        labels: Vec<String>,
        text: String,
        focused: bool,
    },
    /// F2 on a stack.
    Rename {
        app: String,
        old: String,
        text: String,
        focused: bool,
    },
    /// Ctrl+W or a middle click on a stack.
    Close {
        app_label: String,
        name: String,
        windows: Vec<(isize, String)>,
    },
}

/// What a menu item or a drop asked for; done after the frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Ask {
    Open(isize),
    Rename(isize),
    Ungroup(isize),
    Close(isize),
    /// "Add to group": the window joins the group with this name.
    AddTo(isize, String),
    /// "New group…": the name box for this window and the picked ones.
    NewGroup(isize),
    /// A card dropped on the middle of another card.
    DropOn {
        dragged: isize,
        card: isize,
    },
}

#[derive(Default)]
pub(super) struct Stacks {
    /// What the last fold found, for every app.
    found: Vec<Found>,
    /// The group shown open: its app and name.
    open: Option<(String, String)>,
    /// Windows picked with Ctrl + click or Shift + an arrow, for Ctrl+G.
    picked: Vec<isize>,
    dialog: Option<Dialog>,
    /// Where the dialog and the stack's list were drawn, so the pictures
    /// under them hide: DWM draws pictures above everything.
    covers: Vec<Rect>,
    asked: Vec<Ask>,
    /// A dragged card is over the middle of this card: a drop groups them.
    drop_on: Option<isize>,
    /// Shift and an arrow were pressed: the card the keys go to is picked
    /// too.
    pick_follow: bool,
    /// The open group's windows that show as a ghost in the group's column
    /// (By monitor view), as drawn last.
    ghosted: Vec<isize>,
}

/// The open group of the app on screen: its windows and the one used last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenMembers {
    pub hwnds: Vec<isize>,
    pub rep: Option<isize>,
}

/// When each window was last used, from the z-order: 0 is the latest.
fn recent_ranks() -> HashMap<isize, usize> {
    crate::core::windows_list::top_windows()
        .into_iter()
        .enumerate()
        .map(|(rank, hwnd)| (hwnd as isize, rank))
        .collect()
}

impl Stacks {
    /// New data from the host: its groups folded. A strip that just opened
    /// starts with every group closed and nothing picked.
    pub fn arrive(&mut self, snapshot: &mut ArrangeSnapshot, fresh: bool) {
        if fresh {
            self.open = None;
            self.picked.clear();
            self.dialog = None;
            self.drop_on = None;
            self.asked.clear();
        }
        self.found.clear();
        self.fold_all(snapshot);
    }

    /// The groups or the open group changed here: fold the strip's data
    /// again.
    pub fn refold(&mut self, snapshot: &mut ArrangeSnapshot) {
        for group in &mut snapshot.groups {
            fold::unfold(group, &self.found);
        }
        self.found.clear();
        self.fold_all(snapshot);
    }

    fn fold_all(&mut self, snapshot: &mut ArrangeSnapshot) {
        let defs = match &snapshot.deck_groups {
            Some(defs) if !defs.is_empty() => defs.clone(),
            _ => {
                self.open = None;
                self.picked.clear();
                return;
            }
        };
        let ranks = recent_ranks();
        let recent = |hwnd: isize| ranks.get(&hwnd).copied().unwrap_or(usize::MAX);
        for group in &mut snapshot.groups {
            let open = self
                .open
                .as_ref()
                .filter(|(app, _)| *app == group.key)
                .map(|(_, name)| name.as_str());
            self.found.extend(fold::fold(group, &defs, &recent, open));
        }
        if !self.found.iter().any(|stack| stack.kind == Kind::Open) {
            self.open = None;
        }
        let all: Vec<isize> = snapshot
            .groups
            .iter()
            .flat_map(|group| group.windows.iter().map(|w| w.hwnd))
            .collect();
        self.picked.retain(|hwnd| all.contains(hwnd));
    }

    /// The closed stack whose card is `hwnd`.
    pub fn closed(&self, hwnd: isize) -> Option<&Found> {
        self.found
            .iter()
            .find(|stack| stack.kind == Kind::Closed && stack.rep == hwnd)
    }

    /// The group whose only open window is `hwnd`.
    pub fn lone(&self, hwnd: isize) -> Option<&Found> {
        self.found
            .iter()
            .find(|stack| stack.kind == Kind::Lone && stack.rep == hwnd)
    }

    /// The group shown open.
    pub fn open_stack(&self) -> Option<&Found> {
        self.found.iter().find(|stack| stack.kind == Kind::Open)
    }

    /// The group (closed, open or lone) that `hwnd` belongs to.
    fn stack_of(&self, hwnd: isize) -> Option<&Found> {
        self.found
            .iter()
            .find(|stack| stack.members.iter().any(|w| w.hwnd == hwnd))
    }

    /// The open group of `group`'s app, if one is open.
    pub fn open_members(&self, group: &ArrangeGroup) -> OpenMembers {
        match self.open_stack().filter(|stack| stack.app == group.key) {
            Some(stack) => OpenMembers {
                hwnds: stack.hwnds(),
                rep: Some(stack.rep),
            },
            None => OpenMembers::default(),
        }
    }

    pub fn set_ghosted(&mut self, ghosted: Vec<isize>) {
        self.ghosted = ghosted;
    }

    /// The window shows as a ghost inside the open group; its own card
    /// dims and gives its number to the ghost.
    pub fn is_ghosted(&self, hwnd: isize) -> bool {
        self.open.is_some() && self.ghosted.contains(&hwnd)
    }

    /// How much of a window's own card shows.
    pub fn dim_card(&self, hwnd: isize) -> f32 {
        if self.is_ghosted(hwnd) {
            DIMMED
        } else {
            self.dim(hwnd)
        }
    }

    /// How much of a card shows: the cards outside an open group dim.
    pub fn dim(&self, hwnd: isize) -> f32 {
        match self.open_stack() {
            Some(stack) if !stack.members.iter().any(|w| w.hwnd == hwnd) => DIMMED,
            _ => 1.0,
        }
    }

    /// The cards' numbers: an open group's windows are 1, 2, 3.
    pub fn numbers(&self, group: &ArrangeGroup, mut numbers: Vec<usize>) -> Vec<usize> {
        if let Some(stack) = self.open_stack().filter(|stack| stack.app == group.key) {
            let mut next = 1;
            for (place, window) in group.windows.iter().enumerate() {
                if stack.members.iter().any(|w| w.hwnd == window.hwnd) {
                    numbers[place] = next;
                    next += 1;
                }
            }
        }
        numbers
    }

    /// The window a typed number opens while a group is open: Some(None)
    /// when the number is none of its windows, None when no group is open.
    pub fn digit(&self, group: &ArrangeGroup, number: usize) -> Option<Option<isize>> {
        let stack = self.open_stack().filter(|stack| stack.app == group.key)?;
        let members: Vec<isize> = group
            .windows
            .iter()
            .map(|w| w.hwnd)
            .filter(|hwnd| stack.members.iter().any(|w| w.hwnd == *hwnd))
            .collect();
        Some(
            number
                .checked_sub(1)
                .and_then(|at| members.get(at).copied()),
        )
    }

    pub fn is_picked(&self, hwnd: isize) -> bool {
        self.picked.contains(&hwnd)
    }

    /// Esc belongs to the dialog or to the open group: it closes them, not
    /// WindowDeck.
    pub fn wants_escape(&self) -> bool {
        self.dialog.is_some() || self.open.is_some()
    }

    /// Which group hints the footer shows.
    pub fn hints(&self, snapshot: &ArrangeSnapshot) -> GroupHints {
        if snapshot.deck_groups.is_none() {
            GroupHints::Off
        } else if self.open.is_some() {
            GroupHints::Open
        } else {
            GroupHints::On
        }
    }
}

/// The footer's hints with the group keys: "1 – 0 Card or group" and
/// "Ctrl + G Group or ungroup" while grouping is on, and the open group's
/// own keys while one is open.
pub fn with_group_hints(
    mut hints: Vec<(&'static str, &'static str)>,
    groups: GroupHints,
) -> Vec<(&'static str, &'static str)> {
    let numbers = hints
        .iter()
        .position(|(_, label)| *label == "Open by number");
    let after_enter = |hints: &[(&str, &str)]| {
        hints
            .iter()
            .position(|(key, _)| *key == "Enter")
            .map_or(0, |at| at + 1)
    };
    match groups {
        GroupHints::Off => {}
        GroupHints::On => {
            if let Some(at) = numbers {
                hints[at] = ("1 \u{2013} 0", "Card or group");
            }
            let at = after_enter(&hints);
            hints.insert(at, ("Ctrl + G", "Group or ungroup"));
        }
        GroupHints::Open => {
            if let Some(at) = numbers {
                hints.remove(at);
            }
            let after = after_enter(&hints);
            for (at, hint) in [
                ("1 \u{2013} 9", "Window in the group"),
                ("Esc", "Close the group"),
                ("Ctrl + G", "Take out of the group"),
            ]
            .into_iter()
            .enumerate()
            {
                hints.insert(after + at, hint);
            }
        }
    }
    hints
}

/// The group keys in the By monitor view's footer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupHints {
    /// "Group related windows" is off.
    Off,
    On,
    /// A group is open.
    Open,
}

#[cfg(test)]
mod tests;
