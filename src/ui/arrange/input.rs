use super::*;

/// A place in a row while laying out: a window, or the gap where the window
/// being dragged would land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Window(usize),
    Gap,
}

/// The rows as they look during a drag: the dragged window taken out, and a
/// gap at `target`, given as (row, index among the remaining windows).
pub fn preview(
    rows: &[Row],
    dragged: Option<usize>,
    target: Option<(usize, usize)>,
) -> Vec<Vec<Slot>> {
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut slots: Vec<Slot> = row
                .members
                .iter()
                .copied()
                .filter(|member| Some(*member) != dragged)
                .map(Slot::Window)
                .collect();
            if let Some((target_row, index)) = target {
                if target_row == row_index {
                    slots.insert(index.min(slots.len()), Slot::Gap);
                }
            }
            slots
        })
        .collect()
}

/// Every window, row by row, after `dragged` is dropped at `target`.
pub fn dropped_order(rows: &[Row], dragged: usize, target: (usize, usize)) -> Vec<usize> {
    preview(rows, Some(dragged), Some(target))
        .into_iter()
        .flatten()
        .map(|slot| match slot {
            Slot::Window(member) => member,
            Slot::Gap => dragged,
        })
        .collect()
}

/// Where the pointer would drop a window, from last frame's layout: the row
/// under it (or the nearest) and how many of that row's cards come before
/// it, on the lines above or to its left on its own line.
pub(super) fn drop_target(
    pointer: egui::Pos2,
    row_rects: &[Rect],
    card_rects: &[Vec<Rect>],
) -> Option<(usize, usize)> {
    let row = row_rects
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |r: &Rect| {
                if r.y_range().contains(pointer.y) {
                    0.0
                } else {
                    (r.center().y - pointer.y).abs()
                }
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(index, _)| index)?;
    let index = card_rects
        .get(row)
        .map(|cards| {
            cards
                .iter()
                .filter(|card| {
                    card.max.y <= pointer.y
                        || (card.y_range().contains(pointer.y) && card.center().x < pointer.x)
                })
                .count()
        })
        .unwrap_or(0);
    Some((row, index))
}

/// What a drop means: a move to the target row's desktop when that differs,
/// then the whole group's new order.
pub(super) fn drop_actions(
    group: &ArrangeGroup,
    rows: &[Row],
    dragged: usize,
    target: (usize, usize),
) -> Vec<ArrangeAction> {
    let mut actions = Vec::new();
    let from = rows.iter().position(|row| row.members.contains(&dragged));
    let to = &rows[target.0];
    if from != Some(target.0) && !to.desktop.is_empty() {
        actions.push(ArrangeAction::MoveToDesktop {
            hwnd: group.windows[dragged].hwnd,
            desktop: to.desktop.clone(),
        });
    }
    let shown: Vec<isize> = dropped_order(rows, dragged, target)
        .into_iter()
        .map(|member| group.windows[member].hwnd)
        .collect();
    let now: Vec<isize> = group.windows.iter().map(|w| w.hwnd).collect();
    // Windows that are not in the rows (another desktop's, or another
    // monitor's in a limited strip) keep their places.
    let order = merge_order(&now, &shown);
    if order != now || !actions.is_empty() {
        actions.push(ArrangeAction::Reorder {
            group: group.key.clone(),
            order,
        });
    }
    actions
}

/// ←/→ move the cursor, Ctrl+←/→ move its window within its row, Alt+←/→
/// give it the next lower or higher number, Enter switches to it. Esc is
/// handled with the other ways to close.
pub(super) fn keyboard(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    rows: &[Row],
    actions: &mut Vec<ArrangeAction>,
    activate: &mut Option<isize>,
) {
    let flat: Vec<usize> = rows.iter().flat_map(|row| row.members.clone()).collect();
    let Some(position) = shared
        .cursor
        .and_then(|hwnd| flat.iter().position(|m| group.windows[*m].hwnd == hwnd))
    else {
        return;
    };
    let member = flat[position];
    // Alt first: a key pattern without modifiers also matches with Alt down.
    let (alt_left, alt_right, ctrl_left, ctrl_right, left, right, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    let (up, down) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
        )
    });
    if up || down {
        if let Some(hwnd) = card_above_or_below(&shared.card_hits, group.windows[member].hwnd, down)
        {
            shared.cursor = Some(hwnd);
        }
        return;
    }
    if enter {
        *activate = Some(group.windows[member].hwnd);
        return;
    }
    if alt_left || alt_right {
        let step = if alt_left { -1 } else { 1 };
        actions.extend(nudge(group, desktops, member, step));
        return;
    }
    if left && position > 0 {
        shared.cursor = Some(group.windows[flat[position - 1]].hwnd);
    }
    if right && position + 1 < flat.len() {
        shared.cursor = Some(group.windows[flat[position + 1]].hwnd);
    }
    let Some(row) = rows.iter().position(|row| row.members.contains(&member)) else {
        return;
    };
    let at = rows[row]
        .members
        .iter()
        .position(|m| *m == member)
        .unwrap_or(0);
    if ctrl_left && at > 0 {
        actions.extend(drop_actions(group, rows, member, (row, at - 1)));
    }
    if ctrl_right && at + 1 < rows[row].members.len() {
        actions.extend(drop_actions(group, rows, member, (row, at + 1)));
    }
}

/// Keeps the moves and reorders in `actions` on screen until the plugin's
/// data shows them too, so the steps the host is still doing do not flash by.
pub(super) fn hold_changes(shared: &mut Shared, actions: &[ArrangeAction]) {
    let Some(snapshot) = shared.snapshot.as_ref() else {
        return;
    };
    let Some(group) = shared.group() else {
        return;
    };
    let place = |device: &str| snapshot.monitors.iter().position(|m| m.device == device);
    let Some(new) = hold::Hold::new(group, actions, &place, Instant::now()) else {
        return;
    };
    shared.holds.retain(|old| old.group() != new.group());
    shared.holds.push(new);
}

/// Shows a change at once; the plugin's own data follows and replaces it
/// (see `hold`).
pub(super) fn apply_locally(shared: &mut Shared, action: &ArrangeAction) {
    let Some(key) = shared.key.clone() else {
        return;
    };
    let Some(snapshot) = shared.snapshot.as_mut() else {
        return;
    };
    let monitor_place = |device: &str| snapshot.monitors.iter().position(|m| m.device == device);
    let moved_to = match action {
        ArrangeAction::MoveToMonitor { monitor, .. } => monitor_place(monitor),
        _ => None,
    };
    let Some(group) = snapshot.groups.iter_mut().find(|group| group.key == key) else {
        return;
    };
    match action {
        ArrangeAction::MoveToDesktop { hwnd, desktop } => {
            if let Some(window) = group.windows.iter_mut().find(|w| w.hwnd == *hwnd) {
                window.desktop = desktop.clone();
            }
        }
        ArrangeAction::Reorder { order, .. } => {
            group.windows.sort_by_key(|window| {
                order
                    .iter()
                    .position(|h| *h == window.hwnd)
                    .unwrap_or(usize::MAX)
            });
        }
        ArrangeAction::MoveToMonitor { hwnd, .. } => {
            // The plugin saves right after the move, so the saved monitor
            // follows too and no Moved badge flashes up.
            if let Some(window) = group.windows.iter_mut().find(|w| w.hwnd == *hwnd) {
                window.on_monitor = moved_to;
                window.saved_monitor = moved_to;
            }
        }
        ArrangeAction::Close(hwnd) => group.windows.retain(|window| window.hwnd != *hwnd),
        ArrangeAction::Rename {
            hwnd,
            name: Some(name),
        } => {
            if let Some(window) = group.windows.iter_mut().find(|w| w.hwnd == *hwnd) {
                window.label = name.clone();
                window.renamed = true;
            }
        }
        ArrangeAction::Rename { name: None, .. }
        | ArrangeAction::AskName(_)
        | ArrangeAction::NewWindow { .. }
        | ArrangeAction::Activate(_) => {}
    }
}
