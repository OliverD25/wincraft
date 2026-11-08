//! Quick access (design 4a): the pinned windows as cards in rows of four,
//! in pin order, each with its live picture, title and app, and the key
//! that reaches it in a keycap under it. ↑↓←→ select, Enter or a double
//! click jumps, 1–0 jumps, Ctrl+arrows or a drag reorder (the other cards
//! slide aside), Del or a middle click unpins (the card fades and the gap
//! closes), F2 opens the details, a right click, the Menu key or Shift+F10
//! opens the card's menu, Esc closes.

use std::time::Instant;

use egui::{pos2, vec2, Align2, Key, Painter, Rect, Sense, SetOpenCommand, Stroke, StrokeKind, Ui};

use crate::core::animations;
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::icons::{self, Icon};

use super::super::matching::{SlotView, State};
use super::super::pins::slot_of_digit;
use super::super::shared::{Request, View};
use super::card_menu::{self, Choice};
use super::motion::{card_keys, gap_at, slide_key};
use super::{app_icon, details_panel, draft, name_card, ListUi, Outcome, DETAILS, NOT_OPEN};

/// Cards in a row.
pub const COLUMNS: usize = 4;
const PAD_X: f32 = 20.0;
const PAD_TOP: f32 = 18.0;
/// The app icon left of a card's title and app line.
const ICON: f32 = 32.0;
/// How far the title and app line start right of the icon's left edge.
const TEXT_SHIFT: f32 = ICON + 10.0;
const PAD_BOTTOM: f32 = 22.0;
const HEADER: f32 = 28.0;
const HEADER_GAP: f32 = 18.0;
const CARD_W: f32 = 300.0;
pub(super) const CARD_H: f32 = 250.0;
const PICTURE_H: f32 = 175.0;
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
        match step(key, state.selected, count, details) {
            Step::Select(slot) => state.selected = slot,
            Step::Move { from, to } => move_card(state, &keys, (from, to), now, out),
            Step::Jump(slot) => jump(slot, out),
            Step::Remove(slot) => unpin(view, state, &keys, slot, out),
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
    let order = state.motion.order(count);
    let dragged = state
        .motion
        .drag
        .and_then(|drag| order.get(drag.gap).copied());
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
            let other = state.details.as_ref().is_some_and(|d| d.slot != index);
            if other || again {
                state.details = draft(view, index);
            }
        }
        if response.double_clicked() {
            jump(index, out);
        }
        // A middle click unpins, as Del does.
        if response.middle_clicked() {
            unpin(view, state, &keys, place, out);
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(at) = response.interact_pointer_pos() {
                state.motion.start(place, at - target.min);
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
            .show(|ui| choice = card_menu::show(ui, card, place, count));
        if choice == Some(Choice::Edit) {
            state.selected = index;
            state.details = draft(view, index);
        }
        match choice.and_then(|choice| choice.request(place)) {
            Some(Request::Jump(_)) => jump(index, out),
            Some(Request::Move { from, to }) => move_card(state, &keys, (from, to), now, out),
            Some(Request::Remove(slot)) => unpin(view, state, &keys, slot, out),
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
                if let Some(request) = state.motion.release(&keys, now) {
                    state.selected = drag.gap;
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

fn jump(slot: usize, out: &mut Vec<Outcome>) {
    out.push(Outcome::Send(Request::Jump(slot)));
    out.push(Outcome::Close);
}

/// Sends a move, and the cards stand in their new order at once.
fn move_card(
    state: &mut ListUi,
    keys: &[isize],
    (from, to): (usize, usize),
    now: Instant,
    out: &mut Vec<Outcome>,
) {
    out.push(Outcome::Send(Request::Move { from, to }));
    state.motion.moved(keys, from, to, now);
    state.selected = to.min(keys.len().saturating_sub(1));
    state.details = None;
}

/// Unpins the card at place `slot`, as Del, a middle click and the menu
/// do: the engine shows the notice with Undo, and the card fades out.
fn unpin(view: &View, state: &mut ListUi, keys: &[isize], slot: usize, out: &mut Vec<Outcome>) {
    out.push(Outcome::Send(Request::Remove(slot)));
    state.details = None;
    let leaving = animations::on()
        .then(|| state.motion.order(keys.len()).get(slot).copied())
        .flatten()
        .and_then(|index| {
            let rect = state.motion.slides.drawn_last(slide_key(keys[index]))?;
            Some((view.slots.get(index)?.clone(), rect))
        });
    state.motion.removed(keys, slot, leaving, Instant::now());
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
/// Esc while the list is empty.
pub(super) fn hints(count: usize) -> Vec<(&'static str, &'static str)> {
    let mut hints = vec![("Esc", "Close")];
    if count > 0 {
        hints.extend([
            ("Del", "Unpin"),
            ("F2", "Details"),
            ("1\u{2013}0", "Switch"),
        ]);
    }
    hints
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
        count_line(count),
        theme::regular(14.0),
        tokens.text_disabled,
    );
    let mut x = right;
    for (key, label) in hints(count) {
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

/// The light outline of a card's picture box, when no live picture fills it.
fn frame_box(painter: &Painter, picture: Rect, tokens: &Tokens) {
    painter.rect_stroke(
        picture,
        6,
        Stroke::new(1.0, theme::with_alpha(tokens.text_primary, 0.08)),
        StrokeKind::Inside,
    );
}

/// A card that is not open names its window in the quieter colour.
fn title_colour(state: State, tokens: &Tokens) -> egui::Color32 {
    if state == State::NotOpen {
        tokens.text_secondary
    } else {
        tokens.text_primary
    }
}

/// How one card is drawn.
#[derive(Clone, Copy)]
struct Look {
    selected: bool,
    hovered: bool,
    /// Whether its live picture may show.
    live: bool,
    /// How much of the card shows, for one that fades out.
    alpha: f32,
}

fn draw_card(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    card: &SlotView,
    look: Look,
    state: &mut ListUi,
) {
    let tokens = Tokens::get(ui.ctx());
    let Look {
        selected, hovered, ..
    } = look;
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
    let pixels_per_point = ui.ctx().pixels_per_point();
    match (card.state, card.hwnd) {
        (State::Open | State::InFront, Some(hwnd)) => {
            let scale = state.picture_scale;
            let shown = look.live.then(|| {
                state
                    .pictures
                    .show_faded(hwnd, picture, pixels_per_point, scale, look.alpha)
            });
            match shown.flatten() {
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
            let _ = app_icon(
                ui,
                painter,
                state,
                card,
                Rect::from_center_size(picture.center(), vec2(48.0, 48.0)),
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
            tokens.text_secondary,
        );
    }
    let alpha = icon_alpha(card.state);
    let icon = Rect::from_min_size(
        pos2(inner.left() + 2.0, picture.bottom() + 14.0),
        vec2(ICON, ICON),
    );
    if !app_icon(ui, painter, state, card, icon, alpha) {
        letter_icon(painter, icon, &card.app_name, &tokens, alpha);
    }
    let text_left = inner.left() + 2.0 + TEXT_SHIFT;
    let width = width - TEXT_SHIFT;
    let title = super::clip_text(
        painter,
        card.title_line(),
        theme::regular(18.0),
        width - 22.0,
    );
    painter.text(
        pos2(text_left, picture.bottom() + 10.0),
        Align2::LEFT_TOP,
        title,
        theme::regular(18.0),
        title_colour(card.state, &tokens),
    );
    let second = match card.state {
        State::NotOpen => format!("{} \u{b7} not open", card.app_name),
        _ if card.place.is_empty() => card.app_name.clone(),
        _ => format!("{} \u{b7} {}", card.app_name, card.place),
    };
    let second = super::clip_text(painter, &second, theme::regular(13.0), width);
    painter.text(
        pos2(text_left, picture.bottom() + 36.0),
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

/// A closed window's icon is faded like the rest of its card.
fn icon_alpha(state: State) -> f32 {
    match state {
        State::Open | State::InFront => 1.0,
        _ => NOT_OPEN,
    }
}

/// The first letter of the app, for an app whose exe gave no icon.
fn first_letter(app: &str) -> String {
    app.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_owned())
}

fn letter_icon(painter: &Painter, rect: Rect, app: &str, tokens: &Tokens, alpha: f32) {
    painter.rect_filled(
        rect,
        7,
        theme::with_alpha(tokens.text_primary, 0.10 * alpha),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        first_letter(app),
        theme::semibold(16.0),
        tokens.text_secondary.gamma_multiply(alpha),
    );
}

#[cfg(test)]
mod tests;
