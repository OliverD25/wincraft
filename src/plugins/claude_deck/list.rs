//! The list in ClaudeDeck's window: each line drawn, what a click, a
//! double-click, a drag or a right-click on it does, the rename field of a
//! group, and the hover panel. Every change goes to the saved arrangement
//! of ClaudeDeck's own list; no real chat is ever changed.

use std::time::Duration;

use egui::epaint::Shadow;
use egui::{
    pos2, vec2, Id, Key, LayerId, Order, Painter, Pos2, Rect, Response, Sense, SetOpenCommand, Ui,
};
use windows_sys::Win32::Foundation::HWND;

use crate::core::clipboard;
use crate::core::theme;
use crate::core::ui_bridge::HostChannel;

use super::arrange::families_of;
use super::drop::{self, kinds, Placed};
use super::edit;
use super::hover;
use super::menu::{self, Choice, Context, Item};
use super::model::{Chat, ChatLine, Fold, GroupLine, Line};
use super::rows::{self, GroupState, Look};
use super::select;
use super::window::{bring_forward, DeckUi, Pick, Rename};

const LIST_TOP_PAD: f32 = 4.0;
const LIST_BOTTOM_PAD: f32 = 6.0;
/// A dragged row sits this far right of its place, as on the design page.
const LIFT: f32 = 10.0;
/// Near the top or bottom edge, a drag scrolls the list.
const EDGE: f32 = 24.0;
const EDGE_STEP: f32 = 8.0;
const RENAME_SIZE: f32 = 12.0;
const RENAME_LIMIT: usize = 60;

/// What drawing a line needs besides the line.
struct Draw<'a> {
    painter: &'a Painter,
    look: &'a Look<'a>,
    menu: &'a Context<'a>,
}

/// A menu pick, with what applying it needs.
struct Act {
    choice: Choice,
    pick: Pick,
    /// The chat's window and name, for Open and Copy.
    window: Option<isize>,
    title: String,
    /// The group the line is in.
    group: Option<String>,
}

/// One frame of the list.
pub(super) fn show(
    ui: &mut Ui,
    lines: &[Line],
    look: &Look,
    state: &mut DeckUi,
    to_host: &HostChannel,
) {
    let kinds = kinds(lines);
    let groups: Vec<(String, String)> = state
        .saved
        .groups
        .iter()
        .map(|group| (group.id.clone(), group.name.clone()))
        .collect();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let width = ui.available_width();
            let total: f32 = lines
                .iter()
                .enumerate()
                .map(|(at, line)| rows::height(line, at == 0, look.settings.text))
                .sum::<f32>()
                + LIST_TOP_PAD
                + LIST_BOTTOM_PAD;
            let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
            let painter = ui.painter_at(area);
            let mut top = area.top() + LIST_TOP_PAD;
            let mut placed = Vec::with_capacity(lines.len());
            let mut resting: Option<(Rect, &Chat)> = None;
            let mut acts: Vec<Act> = Vec::new();
            let mut group: Option<&str> = None;
            let mut menu_found = false;
            let mut numbered = 0;
            state.digit_rows.clear();
            for (at, (line, kind)) in lines.iter().zip(&kinds).enumerate() {
                let height = rows::height(line, at == 0, look.settings.text);
                let whole = Rect::from_min_size(pos2(area.left(), top), vec2(width, height));
                top += height;
                let rect = rows::body(line, whole);
                placed.push(Placed {
                    kind: kind.clone(),
                    top: rect.top(),
                    bottom: rect.bottom(),
                });
                match line {
                    Line::Group(g) => group = Some(&g.key),
                    Line::Cold(_) | Line::Hidden(_) => group = None,
                    Line::Chat(_) => {}
                }
                let pick = match line {
                    Line::Group(g) => Some(Pick::Group(g.key.clone())),
                    Line::Chat(c) => Some(Pick::Chat(c.chat.id.clone())),
                    _ => None,
                };
                menu_found |= pick.is_some() && pick == state.menu_for;
                if !ui.is_rect_visible(rect) {
                    end_hidden_rename(line, state);
                    continue;
                }
                match line {
                    Line::Group(g) => {
                        let menu = Context {
                            groups: &groups,
                            can_open: false,
                            colour: &|colour| rows::colour_of(colour, look.tokens),
                        };
                        let draw = Draw {
                            painter: &painter,
                            look,
                            menu: &menu,
                        };
                        group_line(ui, &draw, rect, g, state, &mut acts);
                    }
                    Line::Cold(cold) => {
                        let response = ui.interact(rect, ui.id().with("cd-cold"), Sense::click());
                        rows::cold(&painter, rect, cold, look, response.hovered());
                        if response.clicked() {
                            state.saved.cold_open = !state.saved.cold_open;
                        }
                    }
                    Line::Hidden(hidden) => {
                        let response = ui.interact(rect, ui.id().with("cd-hidden"), Sense::click());
                        rows::hidden(&painter, rect, hidden, look, response.hovered());
                        if response.clicked() {
                            state.hidden_open = !state.hidden_open;
                        }
                    }
                    Line::Chat(c) => {
                        let others: Vec<(String, String)> = groups
                            .iter()
                            .filter(|(id, _)| Some(id.as_str()) != group)
                            .cloned()
                            .collect();
                        let menu = Context {
                            groups: &others,
                            can_open: c.chat.window.is_some(),
                            colour: &|colour| rows::colour_of(colour, look.tokens),
                        };
                        let key = select::digit_for(true, c.stand_in || c.hidden, &mut numbered);
                        if key.is_some() {
                            state.digit_rows.push(c.chat.id.clone());
                        }
                        let row = Row {
                            rect,
                            line: c,
                            group,
                            key: key.filter(|_| state.keyboard),
                        };
                        let draw = Draw {
                            painter: &painter,
                            look,
                            menu: &menu,
                        };
                        if chat_line(ui, &draw, &row, state, &mut acts, to_host) {
                            resting = Some((rect, &c.chat));
                        }
                    }
                }
            }
            if !menu_found {
                state.menu_for = None;
            }
            dragging(ui, area, lines, &placed, look, state);
            if let Some((row, chat)) = resting.filter(|_| state.drag.is_none()) {
                let still = ui.ctx().input(|i| i.pointer.time_since_last_movement());
                if still >= hover::DELAY {
                    hover::show(ui.ctx(), row, ui.clip_rect(), chat, look);
                } else {
                    ui.ctx()
                        .request_repaint_after(Duration::from_secs_f32(hover::DELAY - still));
                }
            }
            for act in acts {
                apply(act, state, to_host);
            }
        });
}

/// Opens the menu on a right-click or when the keyboard asked for it,
/// keeps it where it opened, and returns the pick.
fn popup(
    response: &Response,
    state: &mut DeckUi,
    pick: &Pick,
    items: &[Item],
    menu: &Context,
) -> Option<Choice> {
    let by_key = state.menu_for.as_ref() == Some(pick);
    let opens = response.secondary_clicked() || by_key;
    if opens {
        state.menu_for = None;
        state.selected = Some(pick.clone());
        state.menu_at = Some(match response.interact_pointer_pos() {
            Some(at) if response.secondary_clicked() => at,
            _ => pos2(response.rect.left() + 40.0, response.rect.bottom()),
        });
    }
    let set = opens.then_some(SetOpenCommand::Bool(true));
    let at = state.menu_at.unwrap_or(response.rect.center());
    let mut chosen = None;
    egui::Popup::menu(response)
        .open_memory(set)
        .at_position(at)
        .show(|ui| chosen = menu::show(ui, items, menu));
    chosen
}

fn group_line(
    ui: &mut Ui,
    draw: &Draw,
    rect: Rect,
    line: &GroupLine,
    state: &mut DeckUi,
    acts: &mut Vec<Act>,
) {
    let look = draw.look;
    let pick = Pick::Group(line.key.clone());
    let response = ui.interact(
        rect,
        ui.id().with(("cd-group", &line.key)),
        Sense::click_and_drag(),
    );
    let renaming = state.renaming.as_ref().is_some_and(|r| r.id == line.key);
    let looks = GroupState {
        hovered: response.hovered(),
        selected: state.selected.as_ref() == Some(&pick),
        renaming,
    };
    let room = rows::group(draw.painter, rect, line, look, looks);
    if renaming {
        rename_field(ui, room, state, look);
        return;
    }
    if looks.selected && std::mem::take(&mut state.scroll_to_selected) {
        ui.scroll_to_rect(rect, None);
    }
    if response.clicked() {
        edit::fold_group(&mut state.saved, &line.key);
        state.selected = Some(pick.clone());
    }
    if response.double_clicked() {
        start_rename(state, &line.key, &line.name);
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        state.drag = Some(pick.clone());
    }
    let items = menu::group_items(line.folded);
    if let Some(choice) = popup(&response, state, &pick, &items, draw.menu) {
        acts.push(Act {
            choice,
            pick,
            window: None,
            title: line.name.clone(),
            group: Some(line.key.clone()),
        });
    }
}

pub(super) fn start_rename(state: &mut DeckUi, id: &str, name: &str) {
    state.renaming = Some(Rename {
        id: id.to_string(),
        text: name.to_string(),
        fresh: true,
        focused: false,
    });
}

/// A group being renamed was scrolled out of view: its name is saved and
/// the rename ends, so the list's keys work again.
fn end_hidden_rename(line: &Line, state: &mut DeckUi) {
    let Line::Group(group) = line else {
        return;
    };
    if let Some(rename) = state.renaming.take_if(|r| r.id == group.key) {
        edit::rename(&mut state.saved, &rename.id, &rename.text);
    }
}

/// The name of a group being renamed: Enter or a click elsewhere saves,
/// Esc puts the old name back.
fn rename_field(ui: &mut Ui, room: Rect, state: &mut DeckUi, look: &Look) {
    let Some(rename) = state.renaming.as_mut() else {
        return;
    };
    let edit = egui::TextEdit::singleline(&mut rename.text)
        .id_salt(("cd-rename", rename.id.as_str()))
        .font(theme::semibold(RENAME_SIZE))
        .text_color(look.tokens.text_primary)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::symmetric(4, 0))
        .vertical_align(egui::Align::Center)
        .char_limit(RENAME_LIMIT)
        .desired_width(room.width() - 8.0);
    let response = ui.put(room, edit);
    let has_focus = response.has_focus();
    if !rename.focused && !has_focus {
        response.request_focus();
    }
    if std::mem::take(&mut rename.fresh) {
        if let Some(mut text) = egui::TextEdit::load_state(ui.ctx(), response.id) {
            let end = egui::text::CCursor::new(rename.text.chars().count());
            text.cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    end,
                )));
            text.store(ui.ctx(), response.id);
        }
    }
    let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    let was_focused = rename.focused;
    rename.focused |= has_focus;
    if esc {
        state.renaming = None;
        state.renamed = true;
    } else if enter || (was_focused && !has_focus) {
        state.renamed = true;
        let (id, text) = (rename.id.clone(), rename.text.clone());
        edit::rename(&mut state.saved, &id, &text);
        state.renaming = None;
    }
}

/// A chat row and where it is.
struct Row<'a> {
    rect: Rect,
    line: &'a ChatLine,
    /// The group it is in; None under the cold and hidden lines.
    group: Option<&'a str>,
    /// The digit shown before the mark.
    key: Option<u8>,
}

/// One chat row. True when the mouse rests on it, for the hover panel.
fn chat_line(
    ui: &mut Ui,
    draw: &Draw,
    row: &Row,
    state: &mut DeckUi,
    acts: &mut Vec<Act>,
    to_host: &HostChannel,
) -> bool {
    let line = row.line;
    let look = draw.look;
    let pick = Pick::Chat(line.chat.id.clone());
    let draggable = row.group.is_some() && line.depth == 0 && !line.hidden;
    let sense = if draggable {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let id = ui.id().with(("cd-chat", &line.chat.id, line.hidden));
    let response = ui.interact(row.rect, id, sense);
    let hovered = response.hovered();
    let selected = state.selected.as_ref() == Some(&pick);
    let mut row_painter = draw.painter.clone();
    if state.drag.as_ref() == Some(&pick) {
        row_painter.multiply_opacity(0.4);
    }
    rows::chat(
        &row_painter,
        row.rect,
        line,
        look,
        hovered,
        selected,
        row.key,
    );
    if selected && std::mem::take(&mut state.scroll_to_selected) {
        ui.scroll_to_rect(row.rect, None);
    }
    if response.drag_started_by(egui::PointerButton::Primary) && draggable {
        state.drag = Some(pick.clone());
        state.selected = Some(pick.clone());
    }
    if response.clicked() {
        let on_arrow = line.fold != Fold::None
            && response
                .interact_pointer_pos()
                .is_some_and(|at| rows::chevron_rect(row.rect, line.depth).contains(at));
        if on_arrow || line.stand_in {
            edit::fold_chat(&mut state.saved, &line.chat.id);
        } else {
            state.selected = Some(pick.clone());
            bring_forward(line.chat.window, to_host);
        }
    }
    let items = menu::chat_items(line, row.group.is_some());
    if let Some(choice) = popup(&response, state, &pick, &items, draw.menu) {
        acts.push(Act {
            choice,
            pick,
            window: line.chat.window,
            title: line.chat.title.clone(),
            group: row.group.map(str::to_string),
        });
    }
    hovered && !line.stand_in
}

/// While a chat or group is dragged: the drop line where it would land,
/// the row lifted under the mouse, scrolling at the edges, and the drop
/// when the button is let go. Esc stops the drag.
fn dragging(
    ui: &Ui,
    area: Rect,
    lines: &[Line],
    placed: &[Placed],
    look: &Look,
    state: &mut DeckUi,
) {
    let Some(pick) = state.drag.clone() else {
        return;
    };
    let (down, at, esc) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.latest_pos(),
            i.key_pressed(Key::Escape),
        )
    });
    let Some(at) = at.filter(|_| !esc) else {
        if esc || !down {
            state.drag = None;
            // The Esc that stopped the drag is not the list's.
            state.renamed |= esc;
        }
        return;
    };
    let spot = match &pick {
        Pick::Chat(_) => drop::chat_spot(placed, at.y).map(|(spot, y)| (Some(spot), None, y)),
        Pick::Group(_) => drop::group_spot(placed, at.y).map(|(spot, y)| (None, Some(spot), y)),
    };
    if !down {
        state.drag = None;
        match (&pick, spot) {
            (Pick::Chat(key), Some((Some(spot), _, _))) => {
                edit::place_chat(&mut state.saved, key, &spot)
            }
            (Pick::Group(id), Some((_, Some(spot), _))) => {
                edit::place_group(&mut state.saved, id, &spot)
            }
            _ => {}
        }
        return;
    }
    let layer = LayerId::new(Order::Tooltip, Id::new("cd-drag"));
    let painter = ui.ctx().layer_painter(layer).with_clip_rect(ui.clip_rect());
    if let Some((_, _, y)) = spot {
        rows::drop_line(&painter, area, y, look.tokens);
    }
    lifted(&painter, area, at, lines, &pick, look);
    let clip = ui.clip_rect();
    if at.y < clip.top() + EDGE {
        ui.scroll_with_delta(vec2(0.0, EDGE_STEP));
    } else if at.y > clip.bottom() - EDGE {
        ui.scroll_with_delta(vec2(0.0, -EDGE_STEP));
    }
    ui.ctx().request_repaint();
}

/// The dragged line, drawn raised under the mouse.
fn lifted(painter: &Painter, area: Rect, at: Pos2, lines: &[Line], pick: &Pick, look: &Look) {
    let found = lines
        .iter()
        .enumerate()
        .find(|(_, line)| match (line, pick) {
            (Line::Chat(c), Pick::Chat(key)) => c.depth == 0 && !c.hidden && c.chat.id == *key,
            (Line::Group(g), Pick::Group(id)) => g.key == *id,
            _ => false,
        });
    let Some((index, line)) = found else {
        return;
    };
    let height = rows::body(
        line,
        Rect::from_min_size(
            Pos2::ZERO,
            vec2(1.0, rows::height(line, index == 0, look.settings.text)),
        ),
    )
    .height();
    let rect = Rect::from_min_size(
        pos2(area.left() + LIFT, at.y - height / 2.0),
        vec2(area.width() - LIFT, height),
    );
    let tokens = look.tokens;
    if tokens.shadows {
        let shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: tokens.palette_shadow,
        };
        painter.add(shadow.as_shape(rect, 4));
    }
    painter.rect_filled(rect, 4.0, tokens.elevated_bg);
    match line {
        Line::Chat(c) => rows::chat(painter, rect, c, look, false, false, None),
        Line::Group(g) => {
            rows::group(painter, rect, g, look, GroupState::default());
        }
        _ => {}
    }
}

/// Does what a menu pick asks.
fn apply(act: Act, state: &mut DeckUi, to_host: &HostChannel) {
    let saved = &mut state.saved;
    let id = match &act.pick {
        Pick::Chat(id) | Pick::Group(id) => id.clone(),
    };
    match act.choice {
        Choice::Open => bring_forward(act.window, to_host),
        Choice::Pin(on) => edit::pin(saved, &id, on),
        Choice::Hide(on) => edit::hide(saved, &id, on),
        Choice::Copy => {
            if !clipboard::put_text(state.window as HWND, &act.title) {
                log::warn!("ClaudeDeck: could not open the clipboard");
            }
        }
        Choice::FoldForks => edit::fold_chat(saved, &id),
        Choice::MoveTo(group) => edit::place_chat(saved, &id, &edit::Spot::End(group)),
        Choice::NewWith => {
            let new = edit::new_group(saved, act.group.as_deref(), Some(&id));
            start_rename(state, &new, edit::NEW_GROUP_NAME);
        }
        Choice::Rename => start_rename(state, &id, &act.title),
        Choice::FoldGroup => edit::fold_group(saved, &id),
        Choice::FoldAll(folded) => edit::fold_all(saved, folded),
        Choice::Colour(colour) => edit::colour(saved, &id, colour),
        Choice::NewGroup => {
            let new = edit::new_group(saved, Some(&id), None);
            start_rename(state, &new, edit::NEW_GROUP_NAME);
        }
        Choice::Step(up) => edit::step_group(saved, &id, up),
        Choice::Ungroup => {
            let families = families_of(&state.chats);
            edit::ungroup(&mut state.saved, &id, &families);
        }
    }
}
