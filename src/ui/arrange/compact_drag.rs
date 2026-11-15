//! Dragging a card in the compact popup (an app with one or two windows on
//! this desktop) to change their order, as in the full strip. The cards
//! sit in fixed places, so the place under the pointer is where the card
//! lands, and the other card moves aside at once.

use egui::Response;

use super::{drop_actions, log_drop, Drag, Frame, Row, Shared};
use crate::core::ui_bridge::ArrangeGroup;

/// The place among `count` cards that the pointer at `x` is over: the cards
/// start at `left` and are `step` apart. Left of the first card is the
/// first place, right of the last card the last one.
pub fn landing(x: f32, left: f32, step: f32, count: usize) -> usize {
    let last = count.saturating_sub(1);
    let place = ((x - left) / step).floor();
    if place <= 0.0 {
        0
    } else {
        (place as usize).min(last)
    }
}

/// This desktop's windows in the order they show while `drag` (the dragged
/// window, its landing place) is under way.
pub fn order(here: &[usize], drag: Option<(usize, usize)>) -> Vec<usize> {
    let Some((dragged, at)) = drag else {
        return here.to_vec();
    };
    let mut order: Vec<usize> = here.iter().copied().filter(|m| *m != dragged).collect();
    order.insert(at.min(order.len()), dragged);
    order
}

/// A drag starts on a card of this desktop.
pub(super) fn start(shared: &mut Shared, hwnd: isize, response: &Response) {
    if !response.drag_started() {
        return;
    }
    if let Some(at) = response.interact_pointer_pos() {
        shared.drag = Some(Drag {
            hwnd,
            grab: at - response.rect.min,
            ghost: false,
        });
        shared.cursor = Some(hwnd);
    }
}

/// Follows the drag this frame: the dragged window and its landing place,
/// or None when no card of this desktop is dragged. When the button comes
/// up, the new order goes out with the frame's actions. `rows` are the
/// rows the popup shows, this desktop first; its cards start at `left`.
pub(super) fn follow(
    ctx: &egui::Context,
    shared: &mut Shared,
    frame: &mut Frame,
    group: &ArrangeGroup,
    rows: &[Row],
    left: f32,
    step: f32,
) -> Option<(usize, usize)> {
    let here: &[usize] = rows.first().map_or(&[], |row| &row.members);
    let dragged = shared.drag.as_ref().and_then(|drag| {
        here.iter()
            .copied()
            .find(|member| group.windows[*member].hwnd == drag.hwnd)
    });
    let Some(dragged) = dragged else {
        shared.drag = None;
        return None;
    };
    let at = match ctx.pointer_latest_pos() {
        Some(pointer) => landing(pointer.x, left, step, here.len()),
        None => here.iter().position(|m| *m == dragged).unwrap_or(0),
    };
    if ctx.input(|i| i.pointer.primary_down()) {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else {
        let actions = drop_actions(group, rows, dragged, (0, at));
        log_drop(group, &actions);
        frame.actions.extend(actions);
        shared.drag = None;
    }
    // The release frame keeps the new order, so the card does not jump
    // back before the reorder arrives.
    Some((dragged, at))
}
