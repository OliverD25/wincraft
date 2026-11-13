//! Drawing the Servers tab: one card per server, styled like the News
//! tab's blocks. The header has the fold arrow, the server icon, the name,
//! the online dot and the health; a click folds the card, and its menu
//! opens a terminal there or copies the name. Under it the section titles
//! and the rows: chats as on the Chats tab, processes and services. The
//! rows of a server that does not answer are see-through.

use egui::{pos2, vec2, Color32, CursorIcon, Painter, Rect, Response, Sense, Stroke, Ui};

use crate::core::theme::{self, Tokens};
use crate::plugins::claude_deck::menu::COPY;
use crate::plugins::claude_deck::news::draw::chevron;
use crate::plugins::claude_deck::rows::Look;
use crate::plugins::claude_deck::window::DeckUi;
use crate::ui::widgets::text;

use super::super::draw::{
    colours, copy, height as chat_height, rest_hover, row, server_hover, server_icon,
    OFFLINE_OPACITY, OPEN_TERMINAL,
};
use super::super::load::{Proc, Service, State};
use super::super::view::{self as group_view, ServerLine, Shape};
use super::super::{group_name, terminal, HostView};
use super::view::{self, Line};
use super::{Act, Enter};

const CARD: f32 = 28.0;
const CARD_TOP: f32 = 6.0;
const SECTION: f32 = 22.0;
const ROW: f32 = 22.0;
const PAD: f32 = 10.0;
/// Rows sit under the server's name.
pub(super) const INDENT: f32 = PAD + 10.0 + 7.0;
const ARROW: f32 = 10.0;
const ICON: f32 = 14.0;
const GAP: f32 = 7.0;
const DOT: f32 = 7.0;
const NAME: f32 = 13.0;
const SMALL: f32 = 12.0;
const BOTTOM: f32 = 6.0;

fn line_height(line: &Line, first: bool, two: bool) -> f32 {
    match line {
        Line::Card { .. } if first => CARD,
        Line::Card { .. } => CARD_TOP + CARD,
        Line::Section(_) => SECTION,
        Line::Chat(chat) => chat_height(chat, two),
        Line::Proc { .. } | Line::Service { .. } | Line::Work { .. } | Line::Ended { .. } => ROW,
    }
}

/// The server a line belongs to: the card it is under.
fn servers_of(lines: &[Line]) -> Vec<usize> {
    let mut at = 0;
    lines
        .iter()
        .map(|line| {
            if let Line::Card { at: card, .. } = line {
                at = *card;
            }
            at
        })
        .collect()
}

/// What a line acts on, for the keys.
fn act_of(line: &Line, server: &str) -> Act {
    let (enter, copy) = match line {
        Line::Card { .. } => (Enter::Terminal(None), server.to_string()),
        Line::Chat(ServerLine::Chat { chat, .. }) => {
            (Enter::Terminal(chat.folder.clone()), chat.title.clone())
        }
        Line::Chat(ServerLine::Quiet { .. }) => (Enter::FoldQuiet, String::new()),
        Line::Chat(ServerLine::Hidden { .. }) => (Enter::FoldHidden, String::new()),
        Line::Proc { proc, .. } => (Enter::Nothing, proc.command.clone()),
        Line::Service { service, .. } => (Enter::Nothing, service.name.clone()),
        Line::Work { job, .. } => (
            job.chats
                .first()
                .map_or(Enter::Nothing, |(id, _)| Enter::ShowChat(id.clone())),
            job.command.clone(),
        ),
        Line::Ended { end, .. } => (
            end.chat.clone().map_or(Enter::Nothing, Enter::ShowChat),
            end.command.clone(),
        ),
        Line::Section(_) | Line::Chat(ServerLine::Server(_)) => (Enter::Nothing, String::new()),
    };
    Act {
        server: server.to_string(),
        enter,
        copy,
    }
}

/// One frame of the Servers tab.
pub fn show(ui: &mut Ui, look: &Look, state: &mut DeckUi) {
    let mut views = state.servers.views();
    super::super::busy::warn_servers(&state.chats, &mut views);
    let shape = Shape {
        pinned: &state.saved.pinned,
        hidden: &state.saved.hidden,
        quiet_open: &state.servers.quiet_open,
        hidden_open: &state.servers.hidden_open,
        fold_cold: look.settings.fold_cold,
        now: look.now,
    };
    let folded = state.servers.tab.folded.clone();
    let work = view::Work {
        jobs: &state.servers.jobs,
        history: &state.servers.history,
    };
    let lines = view::lines(&views, &|server| folded.contains(server), &shape, &work);
    let owners = servers_of(&lines);
    state.servers.tab.picks = lines
        .iter()
        .zip(&owners)
        .filter_map(|(line, at)| {
            let pick = line.pick(&views)?;
            Some((pick, act_of(line, &views[*at].0)))
        })
        .collect();
    let two = look.settings.text;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let width = ui.available_width();
            let total: f32 = lines
                .iter()
                .enumerate()
                .map(|(at, line)| line_height(line, at == 0, two))
                .sum::<f32>()
                + BOTTOM;
            let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
            let painter = ui.painter_at(area);
            let mut top = area.top();
            let mut resting = None;
            for (at, line) in lines.iter().enumerate() {
                let h = line_height(line, at == 0, two);
                let whole = Rect::from_min_size(pos2(area.left(), top), vec2(width, h));
                top += h;
                let (server, host) = &views[owners[at]];
                let pick = line.pick(&views);
                let selected = pick.is_some() && state.servers.tab.selected == pick;
                let rect = match line {
                    Line::Card { .. } => {
                        Rect::from_min_max(pos2(whole.left(), whole.bottom() - CARD), whole.max)
                    }
                    Line::Chat(ServerLine::Quiet { .. } | ServerLine::Hidden { .. }) => {
                        Rect::from_min_max(
                            pos2(
                                whole.left(),
                                whole.bottom() - crate::plugins::claude_deck::rows::GROUP,
                            ),
                            whole.max,
                        )
                    }
                    _ => whole,
                };
                if selected && std::mem::take(&mut state.servers.tab.scroll_to_selected) {
                    ui.scroll_to_rect(rect, None);
                }
                if !ui.is_rect_visible(whole) {
                    continue;
                }
                let id = ui.id().with(("cd-servers-tab", at));
                let mut faded = painter.clone();
                if host.checked.is_some() && !host.online {
                    faded.multiply_opacity(OFFLINE_OPACITY);
                }
                match line {
                    Line::Card { folded, .. } => {
                        if at > 0 {
                            painter.hline(
                                whole.x_range(),
                                whole.top() + 0.5,
                                Stroke::new(1.0, look.tokens.hover_bg),
                            );
                        }
                        let response = ui
                            .interact(rect, id, Sense::click())
                            .on_hover_cursor(CursorIcon::PointingHand);
                        card(
                            &painter,
                            rect,
                            (server, host),
                            *folded,
                            look,
                            (response.hovered(), selected),
                        );
                        if response.clicked() {
                            state.servers.tab.selected = pick.clone();
                            state.servers.tab.fold(server, !*folded);
                        }
                        card_menu(&response, state.window, server);
                        let words = super::super::draw::with_job_lines(
                            group_view::hover_text(server, host, &state.servers.waits, look.now),
                            &super::super::jobs::text::on_server(server, &state.servers.jobs),
                            look.now,
                        );
                        response.on_hover_ui(|ui| server_hover(ui, host, &words));
                    }
                    Line::Section(title) => section(&faded, whole, title, look.tokens),
                    Line::Chat(chat) => {
                        let response = row(ui, &painter, (rect, id), chat, look, state, selected);
                        if response.clicked() {
                            state.servers.tab.selected = pick.clone();
                        }
                        if let ServerLine::Chat { chat, .. } = chat {
                            if response.hovered() {
                                resting = Some((rect, chat.clone()));
                            }
                        }
                    }
                    Line::Proc { proc, owner, .. } => {
                        let response = ui.interact(rect, id, Sense::click());
                        proc_row(
                            &faded,
                            rect,
                            proc,
                            owner.as_ref(),
                            look.tokens,
                            (response.hovered(), selected),
                        );
                        if response.clicked() {
                            state.servers.tab.selected = pick.clone();
                        }
                        proc_menu(&response, state.window, proc);
                        let words = view::proc_hover(proc);
                        response.on_hover_text(words);
                    }
                    Line::Work { .. } | Line::Ended { .. } => {
                        let response = ui.interact(rect, id, Sense::click());
                        let looks = (response.hovered(), selected);
                        if let Some(chat) = super::work::row(
                            &faded,
                            rect,
                            line,
                            look,
                            looks,
                            &response,
                            state.window,
                        ) {
                            state.servers.tab.selected = pick.clone();
                            super::super::draw::show_chat(state, &chat);
                        }
                    }
                    Line::Service { service, .. } => {
                        let response = ui.interact(rect, id, Sense::click());
                        service_row(
                            &faded,
                            rect,
                            service,
                            look.tokens,
                            (response.hovered(), selected),
                        );
                        if response.clicked() {
                            state.servers.tab.selected = pick.clone();
                        }
                        let window = state.window;
                        egui::Popup::context_menu(&response).show(|ui| {
                            if ui
                                .add(egui::Button::new(COPY).shortcut_text("Ctrl+C"))
                                .clicked()
                            {
                                copy(window, &service.name);
                                ui.close();
                            }
                        });
                    }
                }
            }
            rest_hover(ui, look, resting);
        });
}

/// The back of a selected or hovered line.
pub(super) fn back(
    painter: &Painter,
    rect: Rect,
    tokens: &Tokens,
    (hovered, selected): (bool, bool),
) {
    if selected {
        painter.rect_filled(rect, 4.0, tokens.selection_bg);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
}

/// Lays out words cut to one line of `width`.
fn words(
    painter: &Painter,
    words: &str,
    font: egui::FontId,
    colour: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = text::job(words, font, colour, None);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    painter.layout_job(job)
}

/// Puts a galley with its left edge at `left`, centred on `middle`.
/// Returns its width.
fn put(painter: &Painter, left: f32, middle: f32, galley: std::sync::Arc<egui::Galley>) -> f32 {
    let size = galley.size();
    painter.galley(
        pos2(left, middle - size.y / 2.0),
        galley,
        Color32::PLACEHOLDER,
    );
    size.x
}

/// A card's header: the arrow, the server icon, the name, the online dot,
/// and at the right the health, "Restart needed" in amber, or the offline
/// words.
fn card(
    painter: &Painter,
    rect: Rect,
    (server, host): (&str, &HostView),
    folded: bool,
    look: &Look,
    looks: (bool, bool),
) {
    let tokens = look.tokens;
    back(painter, rect, tokens, looks);
    let middle = rect.center().y;
    let mut x = rect.left() + PAD;
    chevron(
        painter,
        pos2(x, middle - ARROW / 2.0),
        folded,
        tokens.text_secondary,
    );
    x += ARROW + GAP;
    let (icon, dot) = colours(host.online, tokens);
    server_icon(painter, pos2(x, middle - ICON / 2.0), icon);
    x += ICON + GAP;
    let mut right = rect.right() - PAD;
    if view::needs_restart(host) {
        let restart = words(
            painter,
            view::RESTART,
            theme::semibold(SMALL),
            tokens.warning,
            f32::INFINITY,
        );
        right -= restart.size().x;
        put(painter, right, middle, restart);
        right -= GAP;
    }
    let health = view::card_text(host, look.now);
    let room = (right - x - 80.0).max(40.0);
    let health = words(
        painter,
        &health,
        theme::regular(SMALL),
        tokens.text_secondary,
        room,
    );
    right -= health.size().x;
    put(painter, right, middle, health);
    let name = words(
        painter,
        &group_name(server),
        theme::semibold(NAME),
        tokens.text_primary,
        right - 2.0 * GAP - DOT - x,
    );
    x += put(painter, x, middle, name);
    painter.circle_filled(pos2(x + GAP + DOT / 2.0, middle), DOT / 2.0, dot);
}

/// A section's title: "Claude processes".
fn section(painter: &Painter, rect: Rect, title: &str, tokens: &Tokens) {
    let galley = words(
        painter,
        title,
        theme::semibold(SMALL),
        tokens.text_secondary,
        rect.width(),
    );
    put(painter, rect.left() + INDENT, rect.bottom() - 9.0, galley);
}

/// A row's left words, a small note after them, and the right words.
pub(super) fn row_words(
    painter: &Painter,
    rect: Rect,
    left: f32,
    (name, name_colour): (&str, Color32),
    note: &str,
    right_words: &str,
    tokens: &Tokens,
) {
    let middle = rect.center().y;
    let right = words(
        painter,
        right_words,
        theme::regular(SMALL),
        tokens.text_secondary,
        rect.width() / 2.0,
    );
    let right_left = rect.right() - PAD - right.size().x;
    put(painter, right_left, middle, right);
    let room = right_left - GAP - left;
    let name = words(painter, name, theme::regular(NAME), name_colour, room);
    let used = put(painter, left, middle, name);
    let note_room = room - used - GAP;
    if !note.is_empty() && note_room > 20.0 {
        let note = words(
            painter,
            note,
            theme::regular(SMALL),
            tokens.text_secondary,
            note_room,
        );
        put(painter, left + used + GAP, middle, note);
    }
}

/// A process row: the chat's title (or "No chat (orphan)" in red) or the
/// process's name, the folder or PID, and CPU, RAM and run time.
fn proc_row(
    painter: &Painter,
    rect: Rect,
    proc: &Proc,
    owner: Option<&Option<String>>,
    tokens: &Tokens,
    looks: (bool, bool),
) {
    back(painter, rect, tokens, looks);
    let name = match owner {
        Some(Some(title)) => (title.as_str(), tokens.text_primary),
        Some(None) => (view::ORPHAN, tokens.danger),
        None => (proc.name.as_str(), tokens.text_primary),
    };
    let note = view::proc_note(proc);
    row_words(
        painter,
        rect,
        rect.left() + INDENT,
        name,
        &note,
        &view::proc_right(proc),
        tokens,
    );
}

/// The dot of a service's state.
fn state_colour(state: State, tokens: &Tokens) -> Color32 {
    match state {
        State::Running => tokens.success,
        State::Stopped => tokens.text_disabled,
        State::Failed => tokens.danger,
        State::Restarting => tokens.warning,
    }
}

/// A service row: a state dot, the name, its kind, and its state.
fn service_row(
    painter: &Painter,
    rect: Rect,
    service: &Service,
    tokens: &Tokens,
    looks: (bool, bool),
) {
    back(painter, rect, tokens, looks);
    let middle = rect.center().y;
    let left = rect.left() + INDENT;
    painter.circle_filled(
        pos2(left + DOT / 2.0, middle),
        DOT / 2.0,
        state_colour(service.state, tokens),
    );
    let colour = if service.state.is_problem() {
        tokens.danger
    } else {
        tokens.text_primary
    };
    row_words(
        painter,
        rect,
        left + DOT + GAP,
        (&service.name, colour),
        view::kind_words(service.kind),
        &view::service_right(service),
        tokens,
    );
}

/// The menu of a card's header.
fn card_menu(response: &Response, window: isize, server: &str) {
    egui::Popup::context_menu(response).show(|ui| {
        if ui
            .add(egui::Button::new(OPEN_TERMINAL).shortcut_text("Enter"))
            .clicked()
        {
            terminal::open(server, None);
            ui.close();
        }
        if ui
            .add(egui::Button::new(COPY).shortcut_text("Ctrl+C"))
            .clicked()
        {
            copy(window, server);
            ui.close();
        }
    });
}

/// The menu of a process row.
fn proc_menu(response: &Response, window: isize, proc: &Proc) {
    egui::Popup::context_menu(response).show(|ui| {
        if ui
            .add(egui::Button::new(view::COPY_COMMAND).shortcut_text("Ctrl+C"))
            .clicked()
        {
            copy(window, &proc.command);
            ui.close();
        }
        if ui.button(view::COPY_PID).clicked() {
            copy(window, &proc.pid.to_string());
            ui.close();
        }
    });
}
