//! Mini mode (design section 8): the window shrinks to a narrow column of
//! status marks, one per chat in list order, with a thin line between
//! groups. A click on a mark brings the chat's window to the front; resting
//! the mouse on one shows the hover panel beside the column, in a small
//! window of its own that never takes the mouse or the keyboard. The
//! button at the top brings the full list back.

use std::time::Duration;

use egui::{pos2, vec2, CursorIcon, Painter, Rect, Sense, Stroke, StrokeKind, Ui};

use crate::core::theme::Tokens;
use crate::core::ui_bridge::HostChannel;

use super::hover;
use super::marks;
use super::model::{Chat, Line, Mark};
use super::rows::Look;
use super::window::{bring_forward, DeckUi, Pick};
use super::MarkStyle;

/// The column's width, as on the design page.
pub const WIDTH: f32 = 30.0;
/// The strip at the top with the button back to the full list.
pub const TOP: f32 = 24.0;
const PAD: f32 = 6.0;
const GAP: f32 = 6.0;
const SEP_WIDTH: f32 = 16.0;
const BUTTON: egui::Vec2 = egui::Vec2::new(22.0, 20.0);
/// The column never grows taller than this; more marks scroll.
const TALLEST: f32 = 720.0;
pub const FULL_TIP: &str = "Full list";

/// One place in the column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A chat, by its place in the list's lines.
    Chat(usize),
    /// A folded group, by its place in the lines: its most urgent mark.
    Group(usize),
    /// The thin line between two groups.
    Gap,
}

/// The column's items in list order: each group's chats (or one mark for a
/// folded group), a line between groups. The cold and hidden chats at the
/// end are left out, as on the design page.
pub fn items(lines: &[Line]) -> Vec<Item> {
    let mut items = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        match line {
            Line::Group(group) => {
                if !items.is_empty() && items.last() != Some(&Item::Gap) {
                    items.push(Item::Gap);
                }
                if group.folded && group.chats > 0 {
                    items.push(Item::Group(at));
                }
            }
            Line::Chat(chat) if !chat.stand_in && !chat.hidden => items.push(Item::Chat(at)),
            Line::Chat(_) => {}
            Line::Cold(_) | Line::Hidden(_) => break,
        }
    }
    if items.last() == Some(&Item::Gap) {
        items.pop();
    }
    items
}

/// The chats the digits 1 to 9 pick in mini mode: the first nine marks.
pub fn first_nine(lines: &[Line]) -> Vec<String> {
    items(lines)
        .iter()
        .filter_map(|item| match item {
            Item::Chat(at) => match &lines[*at] {
                Line::Chat(line) => Some(line.chat.id.clone()),
                _ => None,
            },
            _ => None,
        })
        .take(9)
        .collect()
}

fn item_height(item: &Item) -> f32 {
    match item {
        Item::Gap => 1.0,
        _ => marks::SIZE,
    }
}

/// The tops of the items, from the top of the column under the strip.
pub fn tops(items: &[Item]) -> Vec<f32> {
    let mut y = PAD;
    items
        .iter()
        .map(|item| {
            let top = y;
            y += item_height(item) + GAP;
            top
        })
        .collect()
}

/// The size the window takes in mini mode, in points.
pub fn size(items: &[Item]) -> egui::Vec2 {
    let marks: f32 =
        items.iter().map(item_height).sum::<f32>() + GAP * items.len().saturating_sub(1) as f32;
    vec2(WIDTH, (TOP + 2.0 * PAD + marks).min(TALLEST))
}

/// One frame of the column. True when the button asks for the full list.
pub(super) fn show(
    ui: &mut Ui,
    lines: &[Line],
    look: &Look,
    state: &mut DeckUi,
    to_host: &HostChannel,
) -> bool {
    let tokens = look.tokens;
    let full = ui.max_rect();
    let strip = Rect::from_min_size(full.min, vec2(full.width(), TOP));
    let drag = ui.interact(strip, ui.id().with("cd-mini-strip"), Sense::drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let button = Rect::from_center_size(strip.center(), BUTTON);
    let back = ui
        .interact(button, ui.id().with("cd-mini-full"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(FULL_TIP);
    let painter = ui.painter_at(full);
    if back.hovered() {
        painter.rect_filled(button, 4.0, tokens.hover_bg);
    }
    paint_full_icon(&painter, button, tokens);

    let items = items(lines);
    let tops = tops(&items);
    let column = Rect::from_min_max(pos2(full.left(), strip.bottom()), full.max);
    let mut resting: Option<(Rect, &Chat)> = None;
    let mut column_ui = ui.new_child(egui::UiBuilder::new().max_rect(column));
    egui::ScrollArea::vertical()
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .auto_shrink(false)
        .show(&mut column_ui, |ui| {
            let height = (size(&items).y - TOP).max(column.height());
            let (area, _) = ui.allocate_exact_size(vec2(WIDTH, height), Sense::hover());
            let painter = ui.painter_at(area);
            for (item, top) in items.iter().zip(&tops) {
                let y = area.top() + top;
                match item {
                    Item::Gap => {
                        let x = area.center().x - SEP_WIDTH / 2.0;
                        painter.hline(x..=x + SEP_WIDTH, y + 0.5, Stroke::new(1.0, tokens.border));
                    }
                    Item::Chat(at) => {
                        let Line::Chat(line) = &lines[*at] else {
                            continue;
                        };
                        let cell = cell(area, y);
                        let pick = Pick::Chat(line.chat.id.clone());
                        let id = ui.id().with(("cd-mini", &line.chat.id));
                        let response = ui.interact(cell, id, Sense::click());
                        let selected = state.selected.as_ref() == Some(&pick);
                        paint_cell(&painter, cell, selected, response.hovered(), tokens);
                        mark(&painter, cell, line.chat.mark, look);
                        if selected && std::mem::take(&mut state.scroll_to_selected) {
                            ui.scroll_to_rect(cell, None);
                        }
                        if response.clicked() {
                            state.selected = Some(pick);
                            bring_forward(line.chat.window, to_host);
                        }
                        if response.hovered() {
                            resting = Some((cell, &line.chat));
                        }
                    }
                    Item::Group(at) => {
                        let Line::Group(group) = &lines[*at] else {
                            continue;
                        };
                        let cell = cell(area, y);
                        let id = ui.id().with(("cd-mini-g", &group.key));
                        let response = ui
                            .interact(cell, id, Sense::hover())
                            .on_hover_text(format!("{} \u{b7} {} chats", group.name, group.chats));
                        paint_cell(&painter, cell, false, response.hovered(), tokens);
                        mark(&painter, cell, group.roll, look);
                    }
                }
            }
        });
    if let Some((cell, chat)) = resting.filter(|_| ui.ctx().input(|i| i.pointer.has_pointer())) {
        let still = ui.ctx().input(|i| i.pointer.time_since_last_movement());
        if still >= hover::DELAY {
            panel(ui.ctx(), cell, chat, look);
        } else {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f32(hover::DELAY - still));
        }
    }
    back.clicked()
}

/// The place of one mark: the column's width, a little taller than the
/// mark.
fn cell(area: Rect, top: f32) -> Rect {
    Rect::from_min_size(
        pos2(area.left() + 2.0, top - GAP / 2.0),
        vec2(WIDTH - 4.0, marks::SIZE + GAP),
    )
}

fn paint_cell(painter: &Painter, cell: Rect, selected: bool, hovered: bool, tokens: &Tokens) {
    if selected {
        painter.rect_filled(cell, 4.0, tokens.selection_bg);
        painter.rect_stroke(
            cell,
            4.0,
            Stroke::new(1.0, tokens.accent),
            StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(cell, 4.0, tokens.hover_bg);
    }
}

fn mark(painter: &Painter, cell: Rect, mark: Mark, look: &Look) {
    let at = cell.center() - vec2(marks::SIZE / 2.0, marks::SIZE / 2.0);
    marks::paint(painter, at, mark, MarkStyle::Shapes, look.tokens, look.turn);
}

/// The button's icon: the mini mode arrow turned round, pointing out.
fn paint_full_icon(painter: &Painter, rect: Rect, tokens: &Tokens) {
    let origin = rect.center() - vec2(6.0, 6.0);
    let at = |x: f32, y: f32| origin + vec2(x, y);
    let stroke = Stroke::new(1.2, tokens.text_secondary);
    painter.line_segment([at(4.5, 2.0), at(4.5, 10.0)], stroke);
    painter.line_segment([at(7.5, 4.0), at(9.5, 6.0)], stroke);
    painter.line_segment([at(9.5, 6.0), at(7.5, 8.0)], stroke);
}

/// The hover panel beside the column, in a small window of its own: the
/// column is too narrow to hold it. It never takes the mouse or the
/// keyboard, and it goes when the mouse leaves the mark.
fn panel(ctx: &egui::Context, cell: Rect, chat: &Chat, look: &Look) {
    let Some(outer) = ctx.input(|i| i.viewport().outer_rect) else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::background());
    let size = hover::layout(&painter, chat, look, hover::OWN_WIDTH).size;
    let monitor = ctx.input(|i| i.viewport().monitor_size);
    let right_room = monitor.is_none_or(|m| outer.right() + GAP + size.x <= m.x);
    let left = if right_room {
        outer.right() + GAP
    } else {
        outer.left() - GAP - size.x
    };
    let builder = egui::ViewportBuilder::default()
        .with_title("WinCraft \u{b7} ClaudeDeck chat")
        .with_decorations(false)
        .with_taskbar(false)
        .with_active(false)
        .with_resizable(false)
        .with_mouse_passthrough(true)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_position(pos2(left, outer.top() + cell.top()))
        .with_inner_size(size);
    let tokens = *look.tokens;
    let settings = look.settings.clone();
    let (now, turn) = (look.now, look.turn);
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("wincraft-claude-deck-mini-hover"),
        builder,
        |ui, _class| {
            let look = Look {
                tokens: &tokens,
                settings: &settings,
                now,
                turn,
            };
            let rect = ui.ctx().content_rect();
            let painter = ui.painter().clone();
            let laid = hover::layout(&painter, chat, &look, hover::OWN_WIDTH);
            hover::paint(&painter, rect, 0, laid, chat, &look);
        },
    );
}
