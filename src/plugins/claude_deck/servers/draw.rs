//! The server groups, drawn under the PC chats in the same scroll area:
//! each server's line (online dot, name, CPU, RAM and chat count, or
//! "Offline · last seen ..."), its chats as ordinary rows, and its lines
//! of quiet and hidden chats. A click on a chat opens a terminal there;
//! its menu has "Open a terminal there", pin, hide, and the copy items.

use std::time::Duration;

use egui::text::TextWrapping;
use egui::{pos2, vec2, Color32, Painter, Rect, Response, Sense, Ui};
use windows_sys::Win32::Foundation::HWND;

use crate::core::clipboard;
use crate::core::theme;
use crate::plugins::claude_deck::hover;
use crate::plugins::claude_deck::menu::{COPY, FORK, HIDE, PIN, SHOW, UNPIN};
use crate::plugins::claude_deck::model::{ChatLine, ColdLine, Fold, HiddenLine, Line};
use crate::plugins::claude_deck::rows::{self, Look, GROUP, GROUP_GAP, ROW, ROW_TWO};
use crate::plugins::claude_deck::window::{DeckUi, Pick};
use crate::ui::widgets::text;

use super::view::{self, ServerLine, Shape};
use super::{group_name, terminal};

pub const OPEN_TERMINAL: &str = "Open a terminal there";
pub const COPY_PATH: &str = "Copy the folder path";
/// Rows of a server that does not answer are drawn this see-through.
pub const OFFLINE_OPACITY: f32 = 0.55;
const PAD: f32 = 10.0;
const DOT: f32 = 7.0;
const ICON: f32 = 14.0;
const GAP: f32 = 6.0;
const SMALL: f32 = 12.0;

/// The height of a line, its gap above included.
fn height(line: &ServerLine, two: bool) -> f32 {
    match line {
        ServerLine::Chat { .. } if two => ROW_TWO,
        ServerLine::Chat { .. } => ROW,
        _ => GROUP + GROUP_GAP,
    }
}

/// The servers' lines, as the window's state gives them.
pub fn lines_of(state: &DeckUi, look: &Look) -> (Vec<(String, super::HostView)>, Vec<ServerLine>) {
    let mut views = state.servers.views();
    super::busy::warn_servers(&state.chats, &mut views);
    let shape = Shape {
        pinned: &state.saved.pinned,
        hidden: &state.saved.hidden,
        quiet_open: &state.servers.quiet_open,
        hidden_open: &state.servers.hidden_open,
        fold_cold: look.settings.fold_cold,
        now: look.now,
    };
    let lines = view::lines(&views, &shape);
    (views, lines)
}

/// Draws the server groups below what the list drew so far.
pub fn section(ui: &mut Ui, look: &Look, state: &mut DeckUi) {
    state.servers.keyed.clear();
    if !state.servers.is_on() {
        return;
    }
    let (views, lines) = lines_of(state, look);
    let two = look.settings.text;
    let width = ui.available_width();
    let total: f32 = lines.iter().map(|line| height(line, two)).sum::<f32>() + GAP;
    let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
    let painter = ui.painter_at(area);
    let mut top = area.top();
    let mut resting = None;
    for (at, line) in lines.iter().enumerate() {
        let whole = Rect::from_min_size(pos2(area.left(), top), vec2(width, height(line, two)));
        top += whole.height();
        let rect = match line {
            ServerLine::Chat { .. } => whole,
            _ => Rect::from_min_max(pos2(whole.left(), whole.bottom() - GROUP), whole.max),
        };
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let id = ui.id().with(("cd-server-line", at));
        match line {
            ServerLine::Server(index) => {
                let (server, host) = &views[*index];
                let pick = super::server_pick(server);
                let selected = state.selected.as_ref() == Some(&pick);
                state.servers.keyed.push((
                    pick,
                    super::Target {
                        server: server.clone(),
                        folder: None,
                        title: server.clone(),
                    },
                ));
                if selected && std::mem::take(&mut state.scroll_to_selected) {
                    ui.scroll_to_rect(rect, None);
                }
                let response = ui.interact(rect, id, Sense::hover());
                let looks = (response.hovered(), selected);
                let waiting = view::waiting_on(server, &state.servers.waits);
                server_line(&painter, rect, (server, waiting), host, look, looks);
                let words = view::hover_text(server, host, &state.servers.waits, look.now);
                response.on_hover_ui(|ui| server_hover(ui, host, &words));
            }
            ServerLine::Chat {
                chat,
                server,
                greyed,
                pinned,
                hidden,
            } => {
                let response = ui.interact(rect, id, Sense::click());
                let row = ChatLine {
                    chat: (**chat).clone(),
                    stand_in: false,
                    depth: 0,
                    rails: Vec::new(),
                    last: false,
                    fold: Fold::None,
                    pinned: *pinned,
                    hidden: false,
                };
                let mut row_painter = painter.clone();
                if *greyed {
                    row_painter.multiply_opacity(OFFLINE_OPACITY);
                }
                let pick = Pick::Chat(chat.id.clone());
                let selected = state.selected.as_ref() == Some(&pick);
                state.servers.keyed.push((
                    pick.clone(),
                    super::Target {
                        server: server.clone(),
                        folder: chat.folder.clone(),
                        title: chat.title.clone(),
                    },
                ));
                if selected && std::mem::take(&mut state.scroll_to_selected) {
                    ui.scroll_to_rect(rect, None);
                }
                rows::chat(
                    &row_painter,
                    rect,
                    &row,
                    look,
                    response.hovered(),
                    selected,
                    None,
                );
                if response.clicked() {
                    state.selected = Some(pick);
                }
                if response.clicked() {
                    terminal::open(server, chat.folder.as_deref());
                }
                chat_menu(&response, state, server, chat, *pinned, *hidden);
                if response.hovered() {
                    resting = Some((rect, chat.clone()));
                }
            }
            ServerLine::Quiet {
                server,
                chats,
                days,
                open,
            } => {
                let response = ui.interact(rect, id, Sense::click());
                let cold = ColdLine {
                    chats: *chats,
                    days: *days,
                    open: *open,
                };
                rows::cold(&painter, rect, &cold, look, response.hovered());
                if response.clicked() && !state.servers.quiet_open.remove(server) {
                    state.servers.quiet_open.insert(server.clone());
                }
            }
            ServerLine::Hidden {
                server,
                chats,
                open,
            } => {
                let response = ui.interact(rect, id, Sense::click());
                let hidden = HiddenLine {
                    chats: *chats,
                    open: *open,
                };
                rows::hidden(&painter, rect, &hidden, look, response.hovered());
                if response.clicked() && !state.servers.hidden_open.remove(server) {
                    state.servers.hidden_open.insert(server.clone());
                }
            }
        }
    }
    let menu_open = egui::Popup::is_any_open(ui.ctx());
    if let Some((row, mut chat)) = resting.filter(|_| !menu_open) {
        let still = ui.ctx().input(|i| i.pointer.time_since_last_movement());
        if still >= hover::DELAY {
            // A click opens a terminal, so the panel must not say "No open
            // window"; the window number is never used for a server chat.
            chat.window = Some(0);
            hover::show(ui.ctx(), row, ui.clip_rect(), &chat, look);
        } else {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f32(hover::DELAY - still));
        }
    }
}

/// The right-click menu of a server chat.
fn chat_menu(
    response: &Response,
    state: &mut DeckUi,
    server: &str,
    chat: &crate::plugins::claude_deck::model::Chat,
    pinned: bool,
    hidden: bool,
) {
    let window = state.window;
    let saved = &mut state.saved;
    egui::Popup::context_menu(response).show(|ui| {
        let item = |ui: &mut Ui, label: &str, keys: &str| {
            let mut button = egui::Button::new(label);
            if !keys.is_empty() {
                button = button.shortcut_text(keys);
            }
            ui.add(button).clicked()
        };
        if item(ui, OPEN_TERMINAL, "Enter") {
            terminal::open(server, chat.folder.as_deref());
            ui.close();
        }
        if let Some(fork) = &chat.fork {
            if item(ui, FORK, "") {
                terminal::open_fork(server, fork);
                ui.close();
            }
        }
        if item(ui, if pinned { UNPIN } else { PIN }, "") {
            if pinned {
                saved.pinned.remove(&chat.id);
            } else {
                saved.pinned.insert(chat.id.clone());
            }
            ui.close();
        }
        if item(ui, if hidden { SHOW } else { HIDE }, "") {
            if hidden {
                saved.hidden.remove(&chat.id);
            } else {
                saved.hidden.insert(chat.id.clone());
            }
            ui.close();
        }
        if item(ui, COPY, "Ctrl+C") {
            copy(window, &chat.title);
            ui.close();
        }
        ui.separator();
        let path = chat.folder.clone();
        if ui
            .add_enabled(path.is_some(), egui::Button::new(COPY_PATH))
            .clicked()
        {
            copy(window, path.as_deref().unwrap_or_default());
            ui.close();
        }
    });
}

fn copy(window: isize, words: &str) {
    if !clipboard::put_text(window as HWND, words) {
        log::warn!("ClaudeDeck: could not open the clipboard");
    }
}

/// The server icon of a server's line and its hover panel: two stacked
/// rounded boxes with a short line in each, in a 14 px square.
fn server_icon(painter: &Painter, at: egui::Pos2, colour: Color32) {
    let stroke = egui::Stroke::new(1.3, colour);
    for top in [2.0, 7.4] {
        let rect = Rect::from_min_size(pos2(at.x + 2.0, at.y + top), vec2(10.0, 4.6));
        painter.rect_stroke(rect, 1.2, stroke, egui::StrokeKind::Middle);
        let y = rect.center().y;
        painter.line_segment(
            [pos2(rect.left() + 2.2, y), pos2(rect.left() + 4.6, y)],
            stroke,
        );
    }
}

/// The icon's colour and the status dot's: online or not.
fn colours(online: bool, tokens: &theme::Tokens) -> (Color32, Color32) {
    if online {
        (tokens.text_secondary, tokens.success)
    } else {
        (tokens.text_disabled, tokens.text_disabled)
    }
}

/// A server's line: the server icon, the name, a round status dot, and at
/// the right the CPU, RAM and chat count, or the offline words. Unlike the
/// user's groups it has no colour mark and no menu: its name comes from
/// the setting.
fn server_line(
    painter: &Painter,
    rect: Rect,
    (server, waiting): (&str, usize),
    host: &super::HostView,
    look: &Look,
    (hovered, selected): (bool, bool),
) {
    let tokens = look.tokens;
    if selected {
        painter.rect_filled(rect, 0.0, tokens.selection_bg);
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
    let middle = rect.center().y;
    let left = rect.left() + PAD;
    let (icon, dot) = colours(host.online, tokens);
    server_icon(painter, pos2(left, middle - ICON / 2.0), icon);
    let right_words = view::line_text(host, waiting, look.now);
    let right_colour = if waiting > 0 && host.online {
        crate::plugins::claude_deck::marks::colour(
            crate::plugins::claude_deck::model::Mark::WaitsOnServer,
            tokens,
        )
    } else {
        tokens.text_secondary
    };
    let right_job = text::job(&right_words, theme::regular(SMALL), right_colour, None);
    let right_galley = painter.layout_job(right_job);
    let right = rect.right() - PAD - right_galley.size().x;
    painter.galley(
        pos2(right, middle - right_galley.size().y / 2.0),
        right_galley,
        Color32::PLACEHOLDER,
    );
    let name_left = left + ICON + GAP;
    let mut job = text::job(
        &group_name(server),
        theme::semibold(SMALL),
        tokens.text_secondary,
        None,
    );
    job.wrap = TextWrapping {
        max_width: (right - 2.0 * GAP - DOT - name_left).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let name = painter.layout_job(job);
    let name_right = name_left + name.size().x;
    painter.galley(
        pos2(name_left, middle - name.size().y / 2.0),
        name,
        Color32::PLACEHOLDER,
    );
    painter.circle_filled(pos2(name_right + GAP + DOT / 2.0, middle), DOT / 2.0, dot);
}

/// The hover panel of a server's line: the icon, the first line and the
/// status dot, then the other lines.
fn server_hover(ui: &mut Ui, host: &super::HostView, words: &str) {
    let tokens = theme::Tokens::get(ui.ctx());
    let (icon, dot) = colours(host.online, &tokens);
    let mut lines = words.lines();
    let first = lines.next().unwrap_or_default();
    // "home2 · online", the dot, then "· checked 20 s ago".
    let parts: Vec<&str> = first.splitn(3, " \u{b7} ").collect();
    let (head, rest) = match parts.as_slice() {
        [name, state, rest] => (format!("{name} \u{b7} {state}"), Some(*rest)),
        _ => (first.to_string(), None),
    };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::hover());
        server_icon(ui.painter(), rect.min, icon);
        ui.label(head);
        let (rect, _) = ui.allocate_exact_size(vec2(DOT, ICON), Sense::hover());
        ui.painter().circle_filled(rect.center(), DOT / 2.0, dot);
        if let Some(rest) = rest {
            ui.label(format!("\u{b7} {rest}"));
        }
    });
    for line in lines {
        ui.label(line);
    }
}

/// Whether the list has anything to show: PC lines or server groups.
pub fn has_lines(lines: &[Line], state: &DeckUi) -> bool {
    !lines.is_empty() || state.servers.is_on()
}
