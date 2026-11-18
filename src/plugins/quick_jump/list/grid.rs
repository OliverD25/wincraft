//! Quick access (design 4a): the pinned windows as cards in rows of four,
//! in number order, each with its live picture, title and app, and the key
//! that reaches it in a keycap under it. Free numbers have no card. ↑↓←→
//! select, Enter or a double click jumps, 1–0 jumps, Ctrl+←→ move a card
//! to the number before or after it, Ctrl+↑↓ or a drag move it onto
//! another card's number (the cards between shift one and slide aside),
//! Del or a middle click unpins (the card fades, the others keep their
//! numbers), F2 opens the details, a right click, the Menu key or
//! Shift+F10 opens the card's menu, Esc closes.

use std::time::Instant;

use egui::{pos2, vec2, Align2, Key, Painter, Rect, Sense, SetOpenCommand, Stroke, StrokeKind, Ui};

use crate::core::animations;
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::icons::{self, Icon};

use super::super::matching::State;
use super::super::pins::{slot_of_digit, SLOTS};
use super::super::shared::{Request, View};
use super::card::{draw_card, Look};
use super::card_menu::{self, Choice};
use super::motion::{card_keys, gap_at, slide_key};
use super::{details_panel, draft, ListUi, Outcome, DETAILS};

/// Cards in a row.
pub const COLUMNS: usize = 4;
const PAD_X: f32 = 20.0;
const PAD_TOP: f32 = 18.0;
/// The app icon left of a card's title and app line.
pub(super) const ICON: f32 = 32.0;
/// How far the title and app line start right of the icon's left edge.
pub(super) const TEXT_SHIFT: f32 = ICON + 10.0;
const PAD_BOTTOM: f32 = 22.0;
const HEADER: f32 = 28.0;
const HEADER_GAP: f32 = 18.0;
const CARD_W: f32 = 300.0;
pub(super) const CARD_H: f32 = 250.0;
pub(super) const PICTURE_H: f32 = 175.0;
const COLUMN_GAP: f32 = 20.0;
const ROW_GAP: f32 = 28.0;
pub(super) const CAP: f32 = 30.0;
pub(super) const CAP_GAP: f32 = 10.0;

fn row_height() -> f32 {
    CARD_H + CAP_GAP + CAP
}

/// The window's size for `count` pins: as wide as the cards in a row (two
/// at least, four at most), as tall as their rows; wider with the details.
pub fn window_size(count: usize, details: bool) -> egui::Vec2 {
    let top = PAD_TOP + HEADER + HEADER_GAP;
    let columns = count.clamp(2, COLUMNS) as f32;
    let width = 2.0 * PAD_X + columns * CARD_W + (columns - 1.0) * COLUMN_GAP;
    if count == 0 && !details {
        return vec2(
            width,
            top + super::empty::body_height() + super::empty::BOTTOM,
        );
    }
    let rows = count.div_ceil(COLUMNS).max(1) as f32;
    let height = top + rows * row_height() + (rows - 1.0) * ROW_GAP + PAD_BOTTOM;
    if details {
        vec2(width + DETAILS, height.max(640.0))
    } else {
        vec2(width, height)
    }
}

/// A key Quick access acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Left,
    Right,
    CtrlUp,
    CtrlDown,
    CtrlLeft,
    CtrlRight,
    Enter,
    Digit(u8),
    Delete,
    F2,
    /// R: re-pin the window that was in front, on a card that is not open.
    Repin,
    Escape,
}

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Select(usize),
    /// The card at `from` goes to number `to` (a place from 0).
    Move {
        from: usize,
        to: usize,
    },
    Jump(usize),
    Remove(usize),
    Details(usize),
    Repin(usize),
    CloseDetails,
    Close,
    Nothing,
}

/// The card an arrow leads to from `at`, in rows of four. Down from a row
/// with a shorter row under it lands on the last card.
fn neighbour(key: ListKey, at: usize, count: usize) -> usize {
    let last = count.saturating_sub(1);
    match key {
        ListKey::Up | ListKey::CtrlUp => at.checked_sub(COLUMNS).unwrap_or(at),
        ListKey::Down | ListKey::CtrlDown => {
            if at / COLUMNS < last / COLUMNS {
                (at + COLUMNS).min(last)
            } else {
                at
            }
        }
        ListKey::Left | ListKey::CtrlLeft => at.saturating_sub(1),
        ListKey::Right | ListKey::CtrlRight => (at + 1).min(last),
        _ => at,
    }
}

/// What `key` does, with the cards' `numbers` in order; cards are counted
/// by their index.
pub fn step(key: ListKey, selected: usize, numbers: &[usize], details: bool) -> Step {
    let count = numbers.len();
    if key == ListKey::Escape {
        return if details {
            Step::CloseDetails
        } else {
            Step::Close
        };
    }
    if count == 0 {
        return Step::Nothing;
    }
    let selected = selected.min(count - 1);
    match key {
        ListKey::Up | ListKey::Down | ListKey::Left | ListKey::Right => {
            Step::Select(neighbour(key, selected, count))
        }
        ListKey::CtrlLeft | ListKey::CtrlRight => {
            let number = numbers[selected];
            let to = match key {
                ListKey::CtrlLeft => number.checked_sub(1),
                _ => Some(number + 1).filter(|&to| to < SLOTS),
            };
            to.map_or(Step::Nothing, |to| Step::Move { from: selected, to })
        }
        ListKey::CtrlUp | ListKey::CtrlDown => match neighbour(key, selected, count) {
            to if to == selected => Step::Nothing,
            to => Step::Move {
                from: selected,
                to: numbers[to],
            },
        },
        ListKey::Enter => Step::Jump(selected),
        ListKey::Digit(digit) => slot_of_digit(digit)
            .and_then(|slot| numbers.iter().position(|&number| number == slot))
            .map_or(Step::Nothing, Step::Jump),
        ListKey::Delete => Step::Remove(selected),
        ListKey::F2 => Step::Details(selected),
        ListKey::Repin => Step::Repin(selected),
        ListKey::Escape => Step::Nothing,
    }
}

fn pressed_keys(ui: &Ui, typing: bool, details: bool) -> Vec<ListKey> {
    ui.ctx().input(|i| {
        let mut keys = Vec::new();
        if i.key_pressed(Key::Escape) {
            keys.push(ListKey::Escape);
        }
        if typing {
            return keys;
        }
        let ctrl = i.modifiers.ctrl;
        let pairs = [
            (Key::ArrowUp, ListKey::Up, ListKey::CtrlUp),
            (Key::ArrowDown, ListKey::Down, ListKey::CtrlDown),
            (Key::ArrowLeft, ListKey::Left, ListKey::CtrlLeft),
            (Key::ArrowRight, ListKey::Right, ListKey::CtrlRight),
            (Key::Delete, ListKey::Delete, ListKey::Delete),
            (Key::F2, ListKey::F2, ListKey::F2),
        ];
        for (key, plain, with_ctrl) in pairs {
            if i.key_pressed(key) {
                keys.push(if ctrl { with_ctrl } else { plain });
            }
        }
        if i.key_pressed(Key::Enter) && !details {
            keys.push(ListKey::Enter);
        }
        if i.key_pressed(Key::R) && !ctrl && !i.modifiers.alt {
            keys.push(ListKey::Repin);
        }
        let digits = [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for (digit, key) in digits.into_iter().enumerate() {
            if i.key_pressed(key) && !ctrl {
                keys.push(ListKey::Digit(digit as u8));
            }
        }
        keys
    })
}

/// Where card `index` sits, from the top left of the first card.
pub fn card_rect(origin: egui::Pos2, index: usize) -> Rect {
    let column = (index % COLUMNS) as f32;
    let row = (index / COLUMNS) as f32;
    Rect::from_min_size(
        pos2(
            origin.x + column * (CARD_W + COLUMN_GAP),
            origin.y + row * (row_height() + ROW_GAP),
        ),
        vec2(CARD_W, CARD_H),
    )
}

pub(super) fn contents(ui: &mut Ui, view: &View, state: &mut ListUi, out: &mut Vec<Outcome>) {
    let tokens = Tokens::get(ui.ctx());
    let numbers = numbers(view);
    let count = numbers.len();
    let typing = ui.ctx().memory(|memory| memory.focused().is_some());
    let full = ui.max_rect();
    let details = state.details.is_some();
    let area = if details {
        Rect::from_min_max(full.min, pos2(full.right() - DETAILS, full.bottom()))
    } else {
        full
    };
    let keys = card_keys(view);
    let now = Instant::now();
    state.motion.begin(&keys, now);
    // A menu that is open has the keys: Esc closes it and nothing else.
    let menu_open = egui::Popup::is_any_open(ui.ctx());
    let open_menu = !menu_open && menu_key(ui, state, typing);

    // Keys, unless a field of the details panel has them.
    let pressed = if menu_open {
        Vec::new()
    } else {
        pressed_keys(ui, typing, details)
    };
    for key in pressed {
        if key == ListKey::Escape && state.motion.dragging() {
            state.motion.cancel();
            continue;
        }
        match step(key, state.selected, &numbers, details) {
            Step::Select(index) => state.selected = index,
            Step::Move { from, to } => move_card(view, state, &keys, (from, to, false), out),
            Step::Jump(index) => jump(view, index, out),
            Step::Remove(index) => unpin(view, state, &keys, index, out),
            Step::Details(index) => state.details = draft(view, index),
            Step::Repin(index) => {
                if let Some(request) = view
                    .slots
                    .get(index)
                    .and_then(|card| card_menu::repin_request(card, card.slot))
                {
                    out.push(Outcome::Send(request));
                }
            }
            Step::CloseDetails => state.details = None,
            Step::Close => out.push(Outcome::Close),
            Step::Nothing => {}
        }
    }
    if count > 0 && state.selected >= count {
        state.selected = count - 1;
    }

    let painter = ui.painter().clone();
    let left = area.left() + PAD_X;
    let right = area.right() - PAD_X;
    let repin = view
        .slots
        .get(state.selected)
        .is_some_and(card_menu::can_repin);
    header(
        ui,
        &painter,
        (left, right),
        area.top() + PAD_TOP,
        (count, repin),
    );
    let origin = pos2(left, area.top() + PAD_TOP + HEADER + HEADER_GAP);
    if count == 0 {
        super::empty::draw(ui, &painter, origin, &view.jump_keys, out);
        if details {
            details_panel(
                ui,
                view,
                Rect::from_min_max(pos2(full.right() - DETAILS, full.top()), full.max),
                state,
                out,
            );
        }
        return;
    }

    let sources: Vec<egui::Vec2> = view
        .slots
        .iter()
        .filter(|card| matches!(card.state, State::Open | State::InFront))
        .filter_map(|card| card.hwnd)
        .filter_map(|hwnd| state.pictures.source_size(hwnd))
        .collect();
    state.picture_scale =
        super::super::pictures::common_scale(&sources, vec2(CARD_W - 20.0, PICTURE_H));

    let animate = animations::on();
    // DWM draws live pictures above everything, so they give way to a menu.
    let live = !menu_open;
    let pointer = ui.ctx().pointer_interact_pos();
    // The gap follows the middle of the dragged card.
    if let (Some(drag), Some(at)) = (state.motion.drag, pointer) {
        let centre = at - drag.grab + vec2(CARD_W, CARD_H) / 2.0;
        state.motion.drag_to(gap_at(origin, centre, count));
    }
    let order = state.motion.order(&numbers);
    let dragged = state.motion.drag.map(|drag| drag.card);
    for (place, &index) in order.iter().enumerate() {
        if dragged == Some(index) {
            continue;
        }
        let card = &view.slots[index];
        let target = card_rect(origin, place);
        let response = ui.interact(
            target,
            ui.id().with(("quick-access-card", index)),
            Sense::click_and_drag(),
        );
        if response.clicked() {
            // A click on the card that is selected already opens its
            // details, so they can be reached without F2 too.
            let again = state.selected == index;
            state.selected = index;
            let other = state.details.as_ref().is_some_and(|d| d.slot != card.slot);
            if other || again {
                state.details = draft(view, index);
            }
        }
        if response.double_clicked() {
            jump(view, index, out);
        }
        // A middle click unpins, as Del does.
        if response.middle_clicked() {
            unpin(view, state, &keys, index, out);
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(at) = response.interact_pointer_pos() {
                state.motion.start(index, place, at - target.min);
                state.selected = index;
                if let Some(hwnd) = card.hwnd {
                    state.pictures.raise(hwnd);
                }
            }
        }
        let opens = response.secondary_clicked() || (open_menu && state.selected == index);
        if opens {
            state.selected = index;
            state.menu_at = Some(match response.interact_pointer_pos() {
                Some(at) if response.secondary_clicked() => at,
                _ => target.center(),
            });
        }
        let set = if opens {
            Some(SetOpenCommand::Bool(true))
        } else if response.clicked() {
            Some(SetOpenCommand::Bool(false))
        } else {
            None
        };
        let mut choice = None;
        egui::Popup::menu(&response)
            .open_memory(set)
            .at_position(state.menu_at.unwrap_or(target.center()))
            .show(|ui| choice = card_menu::show(ui, card));
        if choice == Some(Choice::Edit) {
            state.selected = index;
            state.details = draft(view, index);
        }
        match choice.and_then(|choice| choice.request(card.slot)) {
            Some(Request::Jump(_)) => jump(view, index, out),
            Some(Request::PutAt { to, .. }) => {
                move_card(view, state, &keys, (index, to, true), out)
            }
            Some(Request::Remove(_)) => unpin(view, state, &keys, index, out),
            Some(request) => out.push(Outcome::Send(request)),
            None => {}
        }
        let drawn = state
            .motion
            .slides
            .place(slide_key(keys[index]), target, now, animate);
        let look = Look {
            selected: state.selected == index,
            hovered: response.hovered(),
            live,
            alpha: 1.0,
        };
        draw_card(ui, &painter, drawn, card, look, state);
    }

    // An unpinned card fades out where it was.
    if let Some(gone) = state.motion.leaving.clone() {
        let (rect, alpha) = gone.look(now);
        let mut faded = painter.clone();
        faded.multiply_opacity(alpha);
        let look = Look {
            selected: false,
            hovered: false,
            live,
            alpha,
        };
        draw_card(ui, &faded, rect, &gone.card, look, state);
    }

    // The dragged card rides above the others, following the pointer.
    match (dragged, state.motion.drag, pointer) {
        (Some(index), Some(drag), Some(at)) => {
            let rect = Rect::from_min_size(at - drag.grab, vec2(CARD_W, CARD_H));
            let layer = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("quick-access-drag"),
            ));
            lift(&layer, rect, &tokens);
            let look = Look {
                selected: true,
                hovered: true,
                live,
                alpha: 1.0,
            };
            draw_card(ui, &layer, rect, &view.slots[index], look, state);
            state.motion.slides.float(slide_key(keys[index]), rect);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if !ui.ctx().input(|i| i.pointer.primary_down()) {
                if let Some(request) = state.motion.release(&keys, &numbers, now) {
                    let placed = state.motion.place_of(&numbers, index);
                    state.selected = placed.unwrap_or(drag.gap);
                    state.details = None;
                    out.push(Outcome::Send(request));
                }
            }
        }
        _ if !ui.ctx().input(|i| i.pointer.any_down()) => state.motion.cancel(),
        _ => {}
    }
    if state.motion.moving() {
        ui.ctx().request_repaint();
    }

    if details {
        let panel = Rect::from_min_max(pos2(full.right() - DETAILS, full.top()), full.max);
        details_panel(ui, view, panel, state, out);
    }
}

/// Jumps to the card at `index` of the view.
fn jump(view: &View, index: usize, out: &mut Vec<Outcome>) {
    if let Some(card) = view.slots.get(index) {
        out.push(Outcome::Send(Request::Jump(card.slot)));
        out.push(Outcome::Close);
    }
}

fn numbers(view: &View) -> Vec<usize> {
    view.slots.iter().map(|card| card.slot).collect()
}

/// Sends the move of card `index` to number `to` (with `push` as Put at
/// number does), and the cards stand at their new numbers at once.
fn move_card(
    view: &View,
    state: &mut ListUi,
    keys: &[isize],
    (index, to, push): (usize, usize, bool),
    out: &mut Vec<Outcome>,
) {
    let numbers = numbers(view);
    let request = state
        .motion
        .moved(keys, &numbers, (index, to), push, Instant::now());
    out.push(Outcome::Send(request));
    state.selected = state.motion.place_of(&numbers, index).unwrap_or(index);
    state.details = None;
}

/// Unpins the card at `index` of the view, as Del, a middle click and the
/// menu do: the engine shows the notice with Undo, and the card fades out.
fn unpin(view: &View, state: &mut ListUi, keys: &[isize], index: usize, out: &mut Vec<Outcome>) {
    state.details = None;
    let leaving = animations::on()
        .then(|| {
            let rect = state
                .motion
                .slides
                .drawn_last(slide_key(*keys.get(index)?))?;
            Some((view.slots.get(index)?.clone(), rect))
        })
        .flatten();
    let now = Instant::now();
    let numbers = numbers(view);
    if let Some(request) = state.motion.removed(keys, &numbers, (index, leaving), now) {
        out.push(Outcome::Send(request));
    }
}

/// The Menu key went down, or Shift+F10 was pressed: the selected card's
/// menu opens.
fn menu_key(ui: &Ui, state: &mut ListUi, typing: bool) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_APPS};
    // egui has no Menu key, so its state is read from Windows. It is the
    // state of this thread's keys, so only a focused window acts on it.
    let down = unsafe { GetKeyState(i32::from(VK_APPS)) } < 0;
    let pressed = down && !state.menu_key_down;
    state.menu_key_down = down;
    let (focused, shift_f10) = ui.ctx().input(|i| {
        let shift_f10 = i.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: Key::F10, pressed: true, modifiers, .. } if modifiers.shift)
        });
        (i.focused, shift_f10)
    });
    !typing && ((pressed && focused) || shift_f10)
}

/// The dragged card's shadow and its solid ground, so the cards it passes
/// over do not show through.
fn lift(painter: &Painter, rect: Rect, tokens: &Tokens) {
    if tokens.shadows {
        let shadow = egui::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: tokens.palette_shadow,
        };
        painter.add(shadow.as_shape(rect, 9));
    }
    painter.rect_filled(rect, 9, tokens.window_bg);
}

/// "2 of 10", next to the title.
pub(super) fn count_line(count: usize) -> String {
    format!("{count} of {}", super::super::pins::SLOTS)
}

/// The key hints at the header's right end, from the right: nothing but
/// Esc while the list is empty, and R only while the picked card is not
/// open.
pub(super) fn hints(count: usize, repin: bool) -> Vec<(&'static str, &'static str)> {
    let mut hints = vec![("Esc", "Close")];
    if count > 0 {
        hints.extend([
            ("Del", "Unpin"),
            ("F2", "Details"),
            ("1\u{2013}0", "Switch"),
        ]);
        if repin {
            hints.push(("R", "Re-pin"));
        }
    }
    hints
}

/// The pin icon, "Quick access", how many are pinned, and the key hints.
fn header(
    ui: &Ui,
    painter: &Painter,
    (left, right): (f32, f32),
    top: f32,
    (count, repin): (usize, bool),
) {
    let tokens = Tokens::get(ui.ctx());
    let middle = top + HEADER / 2.0;
    icons::paint(
        painter,
        Rect::from_center_size(pos2(left + 11.0, middle), vec2(18.0, 18.0)),
        Icon::Pin,
        tokens.accent,
    );
    let title = painter.text(
        pos2(left + 32.0, middle),
        Align2::LEFT_CENTER,
        "Quick access",
        theme::semibold(20.0),
        tokens.text_primary,
    );
    painter.text(
        pos2(title.right() + 12.0, middle),
        Align2::LEFT_CENTER,
        count_line(count),
        theme::regular(14.0),
        tokens.text_disabled,
    );
    let mut x = right;
    for (key, label) in hints(count, repin) {
        let words = painter.text(
            pos2(x, middle),
            Align2::RIGHT_CENTER,
            label,
            theme::regular(13.0),
            tokens.text_secondary,
        );
        let text_w = painter
            .layout_no_wrap(key.to_string(), theme::regular(12.0), tokens.text_secondary)
            .size()
            .x;
        let cap = Rect::from_min_size(
            pos2(
                words.left() - 7.0 - (text_w + 14.0).max(24.0),
                middle - 12.0,
            ),
            vec2((text_w + 14.0).max(24.0), 24.0),
        );
        painter.rect(
            cap,
            5,
            theme::with_alpha(tokens.text_primary, 0.05),
            Stroke::new(1.0, tokens.border),
            StrokeKind::Inside,
        );
        painter.text(
            cap.center(),
            Align2::CENTER_CENTER,
            key,
            theme::regular(12.0),
            tokens.text_secondary,
        );
        x = cap.left() - 16.0;
    }
}

#[cfg(test)]
mod tests;
