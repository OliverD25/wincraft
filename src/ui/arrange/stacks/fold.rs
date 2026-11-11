//! Folding a group's windows into stacks: the windows of a group become one
//! card, so every view, number and key of the strip treats the group as one
//! window. The card is a copy of the window used last, named after the
//! group; unfolding puts the windows back in its place. An open group stays
//! folded too: it opens on a layer above the row, so nothing in the row
//! moves.

use std::collections::HashMap;

use crate::core::deck_groups::{self, DeckGroup};
use crate::core::ui_bridge::{ArrangeAction, ArrangeGroup, ArrangeWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One card stands for the group.
    Closed,
    /// Its card stays in the row, and its windows show on a layer above.
    Open,
    /// Only one of its windows is open: a normal card with a small mark.
    Lone,
}

/// A group of the app that has open windows, as the last fold found it.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub app: String,
    pub name: String,
    /// The card that stands for the group: its window used last. For a lone
    /// one, the window.
    pub rep: isize,
    /// Its open windows, in the strip's order.
    pub members: Vec<ArrangeWindow>,
    pub kind: Kind,
}

impl Kind {
    /// The group is one card in the row: closed, or open on its layer.
    pub fn is_card(self) -> bool {
        self != Kind::Lone
    }
}

impl Found {
    pub fn hwnds(&self) -> Vec<isize> {
        self.members.iter().map(|window| window.hwnd).collect()
    }
}

/// The monitor that holds most of `members`; on a tie, the one of `last`
/// (the window used last) when it is among the most, else the first.
fn main_monitor(members: &[&ArrangeWindow], last: &ArrangeWindow) -> Option<usize> {
    let mut counts: Vec<(Option<usize>, usize)> = Vec::new();
    for window in members {
        match counts.iter_mut().find(|(m, _)| *m == window.on_monitor) {
            Some((_, n)) => *n += 1,
            None => counts.push((window.on_monitor, 1)),
        }
    }
    let most = counts.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let tied: Vec<Option<usize>> = counts
        .iter()
        .filter(|(_, n)| *n == most)
        .map(|(m, _)| *m)
        .collect();
    if tied.contains(&last.on_monitor) {
        last.on_monitor
    } else {
        tied.first().copied().flatten()
    }
}

/// Folds the closed groups of `group`'s app (`defs`) into stack cards.
/// `recent` ranks windows by when they were used, lower is later; `open` is
/// the name of the group shown open, which folds the same way. Returns what
/// it found.
pub fn fold(
    group: &mut ArrangeGroup,
    defs: &[DeckGroup],
    recent: &dyn Fn(isize) -> usize,
    open: Option<&str>,
) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for def in deck_groups::of_app(defs, &group.key) {
        // A stack's card carries its group's name, which is no window's.
        let is_card = |hwnd: isize| {
            found
                .iter()
                .any(|stack| stack.kind.is_card() && stack.rep == hwnd)
        };
        let places: Vec<usize> = group
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| def.has(&window.label) && !is_card(window.hwnd))
            .map(|(place, _)| place)
            .collect();
        let Some(&first) = places.first() else {
            continue;
        };
        let members: Vec<ArrangeWindow> =
            places.iter().map(|p| group.windows[*p].clone()).collect();
        let last = members
            .iter()
            .min_by_key(|window| recent(window.hwnd))
            .cloned()
            .unwrap_or_else(|| members[0].clone());
        let kind = if members.len() == 1 {
            Kind::Lone
        } else if open == Some(def.name.as_str()) {
            Kind::Open
        } else {
            Kind::Closed
        };
        if kind.is_card() {
            let mut rest: Vec<ArrangeWindow> = group
                .windows
                .iter()
                .enumerate()
                .filter(|(place, _)| !places.contains(place))
                .map(|(_, window)| window.clone())
                .collect();
            let refs: Vec<&ArrangeWindow> = members.iter().collect();
            let monitor = main_monitor(&refs, &last);
            let mut card = last.clone();
            card.label = def.name.clone();
            card.renamed = false;
            card.same_title = false;
            card.attention = members.iter().any(|window| window.attention);
            if let Some(on) = members.iter().find(|window| window.on_monitor == monitor) {
                card.on_monitor = on.on_monitor;
                card.monitor = on.monitor.clone();
                card.saved_monitor = on.saved_monitor;
            }
            rest.insert(first, card);
            group.windows = rest;
        }
        found.push(Found {
            app: group.key.clone(),
            name: def.name.clone(),
            rep: last.hwnd,
            members,
            kind,
        });
    }
    found
}

/// Puts the windows of the stacks in `found` back in place of their cards.
pub fn unfold(group: &mut ArrangeGroup, found: &[Found]) {
    for stack in found.iter().filter(|stack| stack.kind.is_card()) {
        if let Some(at) = group.windows.iter().position(|w| w.hwnd == stack.rep) {
            group.windows.splice(at..=at, stack.members.iter().cloned());
        }
    }
}

/// An order of the strip's cards with each stack's card replaced by its
/// windows, for the taskbar.
pub fn expand_order(order: &[isize], found: &[Found]) -> Vec<isize> {
    let closed: HashMap<isize, &Found> = found
        .iter()
        .filter(|stack| stack.kind.is_card())
        .map(|stack| (stack.rep, stack))
        .collect();
    order
        .iter()
        .flat_map(|hwnd| match closed.get(hwnd) {
            Some(stack) => stack.hwnds(),
            None => vec![*hwnd],
        })
        .collect()
}

/// What an action on a stack's card means for its windows: a reorder lists
/// them all, and a move to a desktop or a monitor moves each of them.
pub fn expand_action(action: ArrangeAction, found: &[Found]) -> Vec<ArrangeAction> {
    let stack = |hwnd: isize| {
        found
            .iter()
            .find(|stack| stack.kind == Kind::Closed && stack.rep == hwnd)
    };
    match action {
        ArrangeAction::Reorder { group, order } => vec![ArrangeAction::Reorder {
            group,
            order: expand_order(&order, found),
        }],
        ArrangeAction::MoveToDesktop { hwnd, desktop } => match stack(hwnd) {
            Some(stack) => stack
                .hwnds()
                .into_iter()
                .map(|hwnd| ArrangeAction::MoveToDesktop {
                    hwnd,
                    desktop: desktop.clone(),
                })
                .collect(),
            None => vec![ArrangeAction::MoveToDesktop { hwnd, desktop }],
        },
        ArrangeAction::MoveToMonitor { hwnd, monitor } => match stack(hwnd) {
            Some(stack) => stack
                .hwnds()
                .into_iter()
                .map(|hwnd| ArrangeAction::MoveToMonitor {
                    hwnd,
                    monitor: monitor.clone(),
                })
                .collect(),
            None => vec![ArrangeAction::MoveToMonitor { hwnd, monitor }],
        },
        other => vec![other],
    }
}
