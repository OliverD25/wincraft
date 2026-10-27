use super::*;

/// A key the view answers through `step`. ↑ and ↓ pick by where the cards
/// are on screen, so the view answers them itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// ← and →: the card before or after, in the order of the cards.
    Prev,
    Next,
    /// Ctrl+← and Ctrl+→: one place within the column.
    MoveLeft,
    MoveRight,
    /// Ctrl+Shift+← and Ctrl+Shift+→: the next monitor's column.
    MonitorLeft,
    MonitorRight,
    /// Ctrl+Shift+↑ and Ctrl+Shift+↓: the band of the next desktop.
    DesktopUp,
    DesktopDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Select(Item),
    Move(Spot),
}

/// What a key does to the window `cursor`: ← and → pick the card before or
/// after it, across columns and bands; Ctrl+← and Ctrl+→ move it one place
/// within its column; Ctrl+Shift+← and Ctrl+Shift+→ move it to the end of
/// the next column of a connected monitor; Ctrl+Shift+↑ and Ctrl+Shift+↓
/// move it to the end of the same monitor's column in the band above or
/// below, or of the primary monitor's when its own is disconnected.
pub fn step(
    bands: &[Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    cursor: usize,
    key: Key,
) -> Option<Step> {
    let here = locate(bands, cursor)?;
    let connected = |(r, c): (usize, usize)| monitors[rows[r][c]].connected;
    let to = |band: usize, cell: (usize, usize), card: usize| {
        Step::Move(Spot {
            band,
            cell,
            index: bands[band].flow_index(cell, cursor, card),
        })
    };
    let end =
        |band: usize, cell: (usize, usize)| to(band, cell, bands[band].cells[cell.0][cell.1].len());
    match key {
        Key::Prev | Key::Next => {
            next_item(bands, Item::Card(cursor), key == Key::Next).map(Step::Select)
        }
        Key::MoveLeft => here
            .index
            .checked_sub(1)
            .map(|index| to(here.band, here.cell, index)),
        Key::MoveRight => {
            let len = bands[here.band].cells[here.cell.0][here.cell.1].len();
            (here.index + 1 < len).then(|| to(here.band, here.cell, here.index + 1))
        }
        Key::MonitorLeft | Key::MonitorRight => {
            let cells: Vec<(usize, usize)> = rows
                .iter()
                .enumerate()
                .flat_map(|(r, row)| (0..row.len()).map(move |c| (r, c)))
                .collect();
            let at = cells.iter().position(|cell| *cell == here.cell)?;
            let next = if key == Key::MonitorRight {
                cells[at + 1..]
                    .iter()
                    .copied()
                    .find(|cell| connected(*cell))
            } else {
                cells[..at]
                    .iter()
                    .rev()
                    .copied()
                    .find(|cell| connected(*cell))
            }?;
            Some(end(here.band, next))
        }
        Key::DesktopUp | Key::DesktopDown => {
            let band = if key == Key::DesktopUp {
                here.band.checked_sub(1)?
            } else {
                here.band + 1
            };
            if band >= bands.len() {
                return None;
            }
            let cell = if connected(here.cell) {
                here.cell
            } else {
                primary_cell(monitors, rows)
            };
            Some(end(band, cell))
        }
    }
}

/// The arrow keys `step` answers, the most modifiers first: a pattern with
/// fewer modifiers also matches when more are held. Plain arrows only pick;
/// moving a window needs Ctrl.
pub(super) fn arrow_keys() -> [(egui::Modifiers, egui::Key, Key); 8] {
    let ctrl_shift = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
    [
        (ctrl_shift, egui::Key::ArrowUp, Key::DesktopUp),
        (ctrl_shift, egui::Key::ArrowDown, Key::DesktopDown),
        (ctrl_shift, egui::Key::ArrowLeft, Key::MonitorLeft),
        (ctrl_shift, egui::Key::ArrowRight, Key::MonitorRight),
        (egui::Modifiers::CTRL, egui::Key::ArrowLeft, Key::MoveLeft),
        (egui::Modifiers::CTRL, egui::Key::ArrowRight, Key::MoveRight),
        (egui::Modifiers::NONE, egui::Key::ArrowLeft, Key::Prev),
        (egui::Modifiers::NONE, egui::Key::ArrowRight, Key::Next),
    ]
}

/// The arrow keys pick a window, ghosts too: ← and → in the order of the
/// cards, ↑ and ↓ on the line above or below. Enter on a ghost switches to
/// its window; nothing else acts on a ghost. Ctrl with ← or → moves a card
/// within its column,
/// Ctrl+Shift with the arrows to the next monitor or desktop, Alt with ← or
/// → gives it the next lower or higher number without moving it anywhere
/// else, Enter switches to it. Esc is handled with the other ways to close.
pub(super) fn keyboard(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    model: &Model,
    monitors: &[ArrangeMonitor],
    frame: &mut Frame,
) {
    let Some(cursor) = shared
        .cursor
        .and_then(|hwnd| group.windows.iter().position(|w| w.hwnd == hwnd))
    else {
        return;
    };
    let keys = arrow_keys();
    let (alt_left, alt_right, pressed, up, down, enter) = ctx.input_mut(|i| {
        // Alt first: a key pattern without modifiers also matches with Alt
        // down.
        let alt_left = i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
        let alt_right = i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight);
        let pressed = keys
            .iter()
            .find(|(modifiers, key, _)| i.consume_key(*modifiers, *key))
            .map(|(_, _, key)| *key);
        (
            alt_left,
            alt_right,
            pressed,
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    let item = if shared.cursor_ghost {
        Item::Ghost(cursor)
    } else {
        Item::Card(cursor)
    };
    if up || down {
        shared.rail_focus = None;
        if let Some(next) = card_above_or_below(&shared.items, item, down) {
            pick(shared, group, Some(next));
        }
        return;
    }
    if enter {
        frame.activate = Some(group.windows[cursor].hwnd);
        return;
    }
    if alt_left || alt_right {
        let step = if alt_left { -1 } else { 1 };
        frame.actions.extend(nudge(group, desktops, cursor, step));
        return;
    }
    // A ghost's window is on another monitor: Ctrl+← and Ctrl+→ give it
    // another taskbar place and leave it there; the keys to another monitor
    // or desktop move it as its own card does.
    if let Item::Ghost(_) = item {
        match pressed {
            Some(key @ (Key::Prev | Key::Next)) => {
                if let Some(next) = next_item(&model.bands, item, key == Key::Next) {
                    pick(shared, group, Some(next));
                }
                return;
            }
            Some(key @ (Key::MoveLeft | Key::MoveRight)) => {
                if let Some(spot) = ghost_step(&model.bands, cursor, key == Key::MoveRight) {
                    frame.actions.extend(drop_item_actions(
                        group,
                        desktops,
                        &model.bands,
                        &model.rows,
                        monitors,
                        item,
                        spot,
                    ));
                }
                return;
            }
            Some(_) => pick(shared, group, Some(Item::Card(cursor))),
            None => return,
        }
    }
    // A move to another desktop looks through all of them, so it also
    // works while they are hidden.
    let bands = if matches!(pressed, Some(Key::DesktopUp | Key::DesktopDown)) {
        &model.all
    } else {
        &model.bands
    };
    if pressed.is_some() {
        shared.rail_focus = None;
    }
    match pressed.and_then(|key| step(bands, &model.rows, monitors, cursor, key)) {
        Some(Step::Select(next)) => pick(shared, group, Some(next)),
        Some(Step::Move(spot)) => {
            if spot.band >= model.bands.len() {
                shared.others.expand();
            }
            shared.rail_focus = rail_at(&model.cols, &model.rows, spot.cell);
            frame.actions.extend(drop_actions(
                group,
                desktops,
                bands,
                &model.rows,
                monitors,
                cursor,
                spot,
            ));
        }
        None => {}
    }
}
