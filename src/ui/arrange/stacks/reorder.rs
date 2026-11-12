//! Moving a window to another place inside an open group: a drag on its
//! card, or Alt + Left / Right. A group's order is the taskbar's order, so a
//! move sends the same Reorder a drag in the row sends (decided 2026-11-13).
//! A drag that ends outside the group's layer takes the window out of the
//! group, like Ctrl+G.

use super::*;

/// The gap a dragged card would land in: the one before card `slot`, and
/// `parts.len()` after the last. `parts` are the cards' shown parts, left
/// to right.
pub fn drop_slot(parts: &[Rect], x: f32) -> usize {
    parts.iter().filter(|part| part.center().x < x).count()
}

/// The new order of `n` windows after the one at `from` lands in the gap
/// before `slot`; None when it lands where it already is.
pub fn moved(n: usize, from: usize, slot: usize) -> Option<Vec<usize>> {
    if from >= n || slot > n || slot == from || slot == from + 1 {
        return None;
    }
    let mut order: Vec<usize> = (0..n).collect();
    let item = order.remove(from);
    order.insert(if slot > from { slot - 1 } else { slot }, item);
    Some(order)
}

/// Alt + Left or Right: the gap one place left or right of `from`; None at
/// the ends.
pub fn nudge_slot(n: usize, from: usize, left: bool) -> Option<usize> {
    if left {
        (from > 0).then(|| from - 1)
    } else {
        (from + 1 < n).then_some(from + 2)
    }
}

/// The row's whole order with the group's windows in the order `members`
/// gives, each in one of the places the group's windows held. Every other
/// window keeps its place.
pub fn row_order(row: &[isize], members: &[isize]) -> Vec<isize> {
    let mut next = members.iter();
    row.iter()
        .map(|hwnd| match members.contains(hwnd) {
            true => *next.next().unwrap_or(hwnd),
            false => *hwnd,
        })
        .collect()
}

/// The Reorder that puts the open group's windows (`members`, as shown)
/// in `order`, a list of places among them. `full` is the app's row with
/// the group unfolded.
pub fn reorder(full: &ArrangeGroup, members: &[isize], order: &[usize]) -> ArrangeAction {
    let wanted: Vec<isize> = order.iter().map(|at| members[*at]).collect();
    let row: Vec<isize> = full.windows.iter().map(|w| w.hwnd).collect();
    ArrangeAction::Reorder {
        group: full.key.clone(),
        order: row_order(&row, &wanted),
    }
}

/// Where the line goes that shows the gap: between two cards in a row, on
/// the edge between two shown parts in a fan, and just outside the ends.
pub fn gap_x(parts: &[Rect], slot: usize, gap: f32) -> f32 {
    match (
        slot.checked_sub(1).and_then(|at| parts.get(at)),
        parts.get(slot),
    ) {
        (None, Some(next)) => next.left() - gap / 2.0,
        (Some(before), None) => before.right() + gap / 2.0,
        (Some(before), Some(next)) if before.right() <= next.left() => {
            (before.right() + next.left()) / 2.0
        }
        (Some(_), Some(next)) => next.left(),
        (None, None) => 0.0,
    }
}

#[cfg(test)]
#[path = "reorder_tests.rs"]
mod tests;
