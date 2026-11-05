//! Quick access (design 4a): the pinned windows as cards in rows of four,
//! in pin order, each with its live picture, title and app, and the key
//! that reaches it in a keycap under it. ↑↓←→ select, Enter or a double
//! click jumps, 1–0 jumps, Ctrl+arrows or a drag reorder, Del unpins (the
//! gap closes), F2 opens the details, Esc closes.

use egui::{pos2, vec2, Align2, Key, Painter, Rect, Sense, Stroke, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::icons::{self, Icon};

use super::super::matching::{SlotView, State};
use super::super::pins::slot_of_digit;
use super::super::shared::{Request, View};
use super::{app_icon, details_panel, draft, name_card, ListUi, Outcome, DETAILS, NOT_OPEN};

/// Cards in a row.
pub const COLUMNS: usize = 4;
const PAD_X: f32 = 20.0;
const PAD_TOP: f32 = 18.0;
const PAD_BOTTOM: f32 = 22.0;
const HEADER: f32 = 28.0;
const HEADER_GAP: f32 = 18.0;
const CARD_W: f32 = 300.0;
const CARD_H: f32 = 250.0;
const PICTURE_H: f32 = 175.0;
const COLUMN_GAP: f32 = 20.0;
const ROW_GAP: f32 = 28.0;
const CAP: f32 = 30.0;
const CAP_GAP: f32 = 10.0;
const EMPTY: &str = "No windows yet. Press Win+` then A in any window to add it.";

fn row_height() -> f32 {
    CARD_H + CAP_GAP + CAP
}

/// The window's size for `count` pins: as wide as the cards in a row (two
/// at least, four at most), as tall as their rows; wider with the details.
pub fn window_size(count: usize, details: bool) -> egui::Vec2 {
    let top = PAD_TOP + HEADER + HEADER_GAP;
    if count == 0 && !details {
        return vec2(560.0, top + 24.0 + PAD_BOTTOM);
    }
    let columns = count.clamp(2, COLUMNS) as f32;
    let rows = count.div_ceil(COLUMNS).max(1) as f32;
    let width = 2.0 * PAD_X + columns * CARD_W + (columns - 1.0) * COLUMN_GAP;
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
    Escape,
}

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Select(usize),
    Move { from: usize, to: usize },
    Jump(usize),
    Remove(usize),
    Details(usize),
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

pub fn step(key: ListKey, selected: usize, count: usize, details: bool) -> Step {
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
        ListKey::CtrlUp | ListKey::CtrlDown | ListKey::CtrlLeft | ListKey::CtrlRight => {
            match neighbour(key, selected, count) {
                to if to == selected => Step::Nothing,
                to => Step::Move { from: selected, to },
            }
        }
        ListKey::Enter => Step::Jump(selected),
        ListKey::Digit(digit) => match slot_of_digit(digit) {
            Some(slot) if slot < count => Step::Jump(slot),
            _ => Step::Nothing,
        },
        ListKey::Delete => Step::Remove(selected),
        ListKey::F2 => Step::Details(selected),
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
    let count = view.slots.len();
    let typing = ui.ctx().memory(|memory| memory.focused().is_some());
    let full = ui.max_rect();
    let details = state.details.is_some();
    let area = if details {
        Rect::from_min_max(full.min, pos2(full.right() - DETAILS, full.bottom()))
    } else {
        full
    };

    // Keys, unless a field of the details panel has them.
    for key in pressed_keys(ui, typing, details) {
        match step(key, state.selected, count, details) {
            Step::Select(slot) => state.selected = slot,
            Step::Move { from, to } => {
                out.push(Outcome::Send(Request::Move { from, to }));
                state.selected = to;
                state.details = None;
            }
            Step::Jump(slot) => {
                out.push(Outcome::Send(Request::Jump(slot)));
                out.push(Outcome::Close);
            }
            Step::Remove(slot) => {
                out.push(Outcome::Send(Request::Remove(slot)));
                state.details = None;
            }
            Step::Details(slot) => state.details = draft(view, slot),
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
    header(ui, &painter, (left, right), area.top() + PAD_TOP, count);
    let origin = pos2(left, area.top() + PAD_TOP + HEADER + HEADER_GAP);
    if count == 0 {
        painter.text(
            origin,
            Align2::LEFT_TOP,
            EMPTY,
            theme::regular(14.0),
            tokens.text_secondary,
        );
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

    let pointer = ui.ctx().pointer_interact_pos();
    let mut rects = Vec::new();
    for (index, card) in view.slots.iter().enumerate() {
        let rect = card_rect(origin, index);
        rects.push(rect);
        let response = ui.interact(
            rect,
            ui.id().with(("quick-access-card", index)),
            Sense::click_and_drag(),
        );
        if response.clicked() {
            // A click on the card that is selected already opens its
            // details, so they can be reached without F2 too.
            let again = state.selected == index;
            state.selected = index;
            let other = state.details.as_ref().is_some_and(|d| d.slot != index);
            if other || again {
                state.details = draft(view, index);
            }
        }
        if response.double_clicked() {
            out.push(Outcome::Send(Request::Jump(index)));
            out.push(Outcome::Close);
        }
        if response.drag_started() {
            state.drag = Some(index);
        }
        let selected = state.selected == index;
        draw_card(
            ui,
            &painter,
            rect,
            card,
            selected,
            response.hovered(),
            state,
        );
    }
    if let Some(from) = state.drag {
        if let Some(at) = pointer {
            let target = rects.iter().position(|rect| rect.contains(at));
            if let Some(rect) = target.map(|index| rects[index]) {
                painter.vline(
                    rect.left() - COLUMN_GAP / 2.0,
                    rect.y_range(),
                    Stroke::new(2.0, tokens.accent),
                );
            }
            if ui.ctx().input(|i| i.pointer.any_released()) {
                if let Some(to) = target.filter(|to| *to != from) {
                    out.push(Outcome::Send(Request::Move { from, to }));
                    state.selected = to;
                }
                state.drag = None;
            }
        }
        if !ui.ctx().input(|i| i.pointer.any_down()) {
            state.drag = None;
        }
    }

    if details {
        let panel = Rect::from_min_max(pos2(full.right() - DETAILS, full.top()), full.max);
        details_panel(ui, view, panel, state, out);
    }
}

/// The pin icon, "Quick access", how many are pinned, and the key hints.
fn header(ui: &Ui, painter: &Painter, (left, right): (f32, f32), top: f32, count: usize) {
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
        format!("{count} pinned"),
        theme::regular(15.0),
        tokens.text_disabled,
    );
    let mut x = right;
    for (key, label) in [("Esc", "Close"), ("Del", "Unpin"), ("1\u{2013}9", "Switch")] {
        let words = painter.text(
            pos2(x, middle),
            Align2::RIGHT_CENTER,
            label,
            theme::regular(14.0),
            tokens.text_secondary,
        );
        let text_w = painter
            .layout_no_wrap(key.to_string(), theme::regular(13.0), tokens.text_secondary)
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
            Stroke::new(1.0, theme::with_alpha(tokens.text_primary, 0.14)),
            StrokeKind::Inside,
        );
        painter.text(
            cap.center(),
            Align2::CENTER_CENTER,
            key,
            theme::regular(13.0),
            tokens.text_secondary,
        );
        x = cap.left() - 16.0;
    }
}

/// The light outline of a card's picture box, when no live picture fills it.
fn frame_box(painter: &Painter, picture: Rect, tokens: &Tokens) {
    painter.rect_stroke(
        picture,
        6,
        Stroke::new(1.0, theme::with_alpha(tokens.text_primary, 0.08)),
        StrokeKind::Inside,
    );
}

fn draw_card(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    card: &SlotView,
    selected: bool,
    hovered: bool,
    state: &mut ListUi,
) {
    let tokens = Tokens::get(ui.ctx());
    let (fill, edge) = if selected {
        (theme::with_alpha(tokens.accent, 0.10), tokens.accent)
    } else if hovered {
        (tokens.hover_bg, theme::with_alpha(tokens.text_primary, 0.0))
    } else {
        (
            theme::with_alpha(tokens.text_primary, 0.04),
            theme::with_alpha(tokens.text_primary, 0.0),
        )
    };
    painter.rect(rect, 9, fill, Stroke::new(2.0, edge), StrokeKind::Inside);
    let inner = rect.shrink(10.0);
    let picture = Rect::from_min_size(inner.min, vec2(inner.width(), PICTURE_H));
    let dim = if card.state == State::NotOpen {
        NOT_OPEN
    } else {
        1.0
    };
    let pixels_per_point = ui.ctx().pixels_per_point();
    match (card.state, card.hwnd) {
        (State::Open | State::InFront, Some(hwnd)) => {
            let scale = state.picture_scale;
            match state
                .pictures
                .show_scaled(hwnd, picture, pixels_per_point, scale)
            {
                // A frame that hugs the picture, so its width shows.
                Some(shown) => {
                    painter.rect_filled(shown.expand(1.0), 4, tokens.panel_bg);
                    painter.rect_stroke(
                        shown.expand(1.0),
                        4,
                        Stroke::new(1.0, theme::with_alpha(tokens.text_primary, 0.12)),
                        StrokeKind::Outside,
                    );
                }
                None => {
                    painter.rect_filled(picture, 6, tokens.panel_bg);
                    name_card(painter, picture, card.title_line(), &tokens, 1.0);
                    frame_box(painter, picture, &tokens);
                }
            }
        }
        _ => {
            painter.rect_filled(picture, 6, theme::with_alpha(tokens.panel_bg, NOT_OPEN));
            app_icon(
                ui,
                painter,
                state,
                card,
                Rect::from_center_size(picture.center(), vec2(32.0, 32.0)),
                NOT_OPEN,
            );
            frame_box(painter, picture, &tokens);
        }
    }
    let width = inner.width() - 4.0;
    if selected || hovered {
        // A card can be dragged to another place.
        icons::paint(
            painter,
            Rect::from_min_size(
                pos2(inner.right() - 16.0, picture.bottom() + 14.0),
                vec2(16.0, 16.0),
            ),
            Icon::Grip,
            tokens.text_disabled,
        );
    }
    let title = super::clip_text(
        painter,
        card.title_line(),
        theme::regular(18.0),
        width - 22.0,
    );
    painter.text(
        pos2(inner.left() + 2.0, picture.bottom() + 10.0),
        Align2::LEFT_TOP,
        title,
        theme::regular(18.0),
        theme::with_alpha(tokens.text_primary, dim),
    );
    let second = match card.state {
        State::NotOpen => format!("{} \u{b7} not open", card.app_name),
        _ if card.place.is_empty() => card.app_name.clone(),
        _ => format!("{} \u{b7} {}", card.app_name, card.place),
    };
    let second = super::clip_text(painter, &second, theme::regular(13.0), width);
    painter.text(
        pos2(inner.left() + 2.0, picture.bottom() + 36.0),
        Align2::LEFT_TOP,
        second,
        theme::regular(13.0),
        tokens.text_secondary,
    );
    let cap = Rect::from_center_size(
        pos2(rect.center().x, rect.bottom() + CAP_GAP + CAP / 2.0),
        vec2(CAP, CAP),
    );
    let (cap_edge, cap_text) = if selected {
        (tokens.accent, tokens.accent)
    } else {
        (
            theme::with_alpha(tokens.text_primary, 0.2),
            tokens.text_primary,
        )
    };
    painter.rect(
        cap,
        6,
        theme::with_alpha(tokens.text_primary, 0.05),
        Stroke::new(1.0, cap_edge),
        StrokeKind::Inside,
    );
    painter.hline(
        cap.x_range().shrink(3.0),
        cap.bottom() - 1.5,
        Stroke::new(3.0, cap_edge),
    );
    painter.text(
        cap.center() - vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        card.digit.to_string(),
        theme::semibold(15.0),
        cap_text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_move_in_rows_of_four() {
        // Six cards: 0 1 2 3 / 4 5.
        assert_eq!(step(ListKey::Right, 0, 6, false), Step::Select(1));
        assert_eq!(step(ListKey::Right, 5, 6, false), Step::Select(5));
        assert_eq!(step(ListKey::Left, 0, 6, false), Step::Select(0));
        assert_eq!(step(ListKey::Down, 1, 6, false), Step::Select(5));
        assert_eq!(
            step(ListKey::Down, 3, 6, false),
            Step::Select(5),
            "shorter row"
        );
        assert_eq!(step(ListKey::Down, 5, 6, false), Step::Select(5));
        assert_eq!(step(ListKey::Up, 5, 6, false), Step::Select(1));
        assert_eq!(step(ListKey::Up, 2, 6, false), Step::Select(2));
        assert_eq!(
            step(ListKey::Down, 9, 6, false),
            Step::Select(5),
            "past the end"
        );
    }

    #[test]
    fn ctrl_arrows_reorder_and_the_other_keys_jump_unpin_or_close() {
        assert_eq!(
            step(ListKey::CtrlRight, 0, 6, false),
            Step::Move { from: 0, to: 1 }
        );
        assert_eq!(
            step(ListKey::CtrlDown, 2, 6, false),
            Step::Move { from: 2, to: 5 }
        );
        assert_eq!(step(ListKey::CtrlLeft, 0, 6, false), Step::Nothing);
        assert_eq!(step(ListKey::Enter, 2, 6, false), Step::Jump(2));
        assert_eq!(step(ListKey::Digit(3), 0, 6, false), Step::Jump(2));
        assert_eq!(step(ListKey::Digit(9), 0, 6, false), Step::Nothing);
        assert_eq!(step(ListKey::Digit(0), 0, 10, false), Step::Jump(9));
        assert_eq!(step(ListKey::Delete, 4, 6, false), Step::Remove(4));
        assert_eq!(step(ListKey::F2, 1, 6, false), Step::Details(1));
        assert_eq!(step(ListKey::Escape, 0, 6, true), Step::CloseDetails);
        assert_eq!(step(ListKey::Escape, 0, 6, false), Step::Close);
        assert_eq!(step(ListKey::Enter, 0, 0, false), Step::Nothing);
        assert_eq!(step(ListKey::Escape, 0, 0, false), Step::Close);
    }

    #[test]
    fn the_window_fits_the_rows_of_cards() {
        assert_eq!(window_size(0, false).x, 560.0);
        assert_eq!(window_size(1, false).x, 660.0, "two columns at least");
        assert_eq!(window_size(4, false).x, 1300.0);
        assert_eq!(window_size(10, false).x, 1300.0);
        assert!(window_size(5, false).y > window_size(4, false).y);
        assert_eq!(window_size(3, true).x, 980.0 + DETAILS);
        let second = card_rect(pos2(0.0, 0.0), 4);
        assert_eq!(second.left(), 0.0);
        assert!(second.top() > CARD_H + CAP);
    }
}
