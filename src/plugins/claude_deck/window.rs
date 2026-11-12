//! ClaudeDeck's own window, drawn with egui on the UI thread: a slim frame
//! with no Windows title bar, the header, and the list. It is dragged by
//! the header, resized by its edges, and comes back at the same place and
//! size. While it is open it asks the host for fresh data every few
//! seconds; it repaints only for that, for the times, and for the turning
//! ring of a working chat when Windows allows animations.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::viewport::ResizeDirection;
use egui::{pos2, vec2, Color32, CursorIcon, Key, Rect, Sense, Ui};
use serde_json::Value;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::core::claude_sessions::{self, Session};
use crate::core::claude_windows::{self, ChatWindow};
use crate::core::theme::{self, Tokens};
use crate::core::traits::{PluginWindow, WindowFrame};
use crate::core::ui_bridge::{ArrangeAction, HostChannel, HostRequest};
use crate::core::{animations, claude_jobs, window_place};
use crate::ui::dwm_thumbs;
use crate::ui::settings;
use crate::ui::widgets::{focus, text};

use super::header::{self, Click};
use super::model::{self, Archived, Chat, Line, Mark, View};
use super::place::{self, Place};
use super::rows::{self, Look};
use super::{Handle, ID, ONLY_OPEN_KEY, STAY_ON_TOP_KEY};

pub const WINDOW_ID: &str = "deck";
const TITLE: &str = "WinCraft \u{b7} ClaudeDeck";
const SIZE: [f32; 2] = [360.0, 480.0];
const MIN_SIZE: [f32; 2] = [220.0, 90.0];
/// How often the host is asked to read the chats again; the reader itself
/// reads at most this often too.
const ASK_EVERY: Duration = claude_sessions::READ_EVERY;
/// How often the list is built again from what was read.
const BUILD_EVERY: Duration = Duration::from_secs(1);
/// The Working ring: one turn in this time, drawn in this many steps.
const TURN: Duration = Duration::from_secs(2);
const RING_STEP: Duration = Duration::from_millis(100);
/// How close to the edge the mouse resizes the window.
const EDGE: f32 = 5.0;
const LIST_TOP_PAD: f32 = 4.0;
const LIST_BOTTOM_PAD: f32 = 6.0;
const EMPTY_SIZE: f32 = 13.0;
const EMPTY_PAD: f32 = 28.0;

const EMPTY: &str = "No Claude chats yet.\nChats show here when Claude Code runs.";
const NONE_OPEN: &str = "No chat has a window open.";

#[derive(Default)]
struct DeckUi {
    window: isize,
    placed: bool,
    focus_wanted: bool,
    closed: bool,
    asked: Option<Instant>,
    built: Option<Instant>,
    chats: Vec<Chat>,
    archived: Archived,
    animate: bool,
    folds: View,
    selected: Option<String>,
    scroll_to_selected: bool,
    border: Option<Color32>,
    kept: Option<Place>,
    started: Option<Instant>,
}

pub struct DeckWindow {
    shared: Handle,
    state: Arc<Mutex<DeckUi>>,
}

impl DeckWindow {
    pub fn new(shared: Handle) -> Self {
        Self {
            shared,
            state: Arc::new(Mutex::new(DeckUi::default())),
        }
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-claude-deck")
}

impl PluginWindow for DeckWindow {
    fn id(&self) -> &'static str {
        WINDOW_ID
    }

    /// The hotkey again: brings the window to the front, or closes it when
    /// it is already in front.
    fn open(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            let in_front =
                state.window != 0 && unsafe { GetForegroundWindow() } as isize == state.window;
            if in_front {
                state.closed = true;
            } else {
                state.focus_wanted = true;
            }
        }
    }

    fn close(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.open = false;
        }
        if let Ok(mut state) = self.state.lock() {
            forget(&mut state);
        }
    }

    fn is_open(&self) -> bool {
        self.shared.lock().is_ok_and(|shared| shared.open)
    }

    fn show(&mut self, ctx: &egui::Context, frame: &WindowFrame) {
        let on_top = self
            .shared
            .lock()
            .map(|shared| shared.settings.stay_on_top)
            .unwrap_or(true);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_resizable(true)
            .with_inner_size(SIZE)
            .with_min_inner_size(MIN_SIZE)
            .with_window_level(if on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            });
        if let Some(icon) = &frame.icon {
            builder = builder.with_icon(Arc::clone(icon));
        }
        let shared = Arc::clone(&self.shared);
        let state = Arc::clone(&self.state);
        let palette = frame.palette_hwnd;
        let to_host = Arc::clone(&frame.to_host);
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let (Ok(mut shared), Ok(mut state)) = (shared.lock(), state.lock()) else {
                return;
            };
            let tokens = Tokens::get(ui.ctx());
            find_and_place(&mut state, palette, &tokens, &to_host);
            refresh(&mut state, &to_host);
            let settings = shared.settings.clone();
            let mut view = state.folds.clone();
            view.only_open = settings.only_open;
            view.fold_cold = settings.fold_cold;
            let now = claude_sessions::now_ms();
            let lines = model::lines(&state.chats, &state.archived, &view, now);
            let counts = model::counts(&state.chats, settings.only_open);
            let turn = ring_turn(&state);
            let mut clicked = None;
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(tokens.window_bg))
                .show(ui, |ui| {
                    let full = ui.max_rect();
                    let top = Rect::from_min_size(full.min, vec2(full.width(), header::HEIGHT));
                    clicked = header::show(ui, top, &counts, &settings, &tokens, turn);
                    let list = Rect::from_min_max(pos2(full.left(), top.bottom()), full.max);
                    let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
                    let look = Look {
                        tokens: &tokens,
                        settings: &settings,
                        now,
                        turn,
                    };
                    if lines.is_empty() {
                        empty(&mut list_ui, &tokens, !state.chats.is_empty());
                    } else {
                        list_lines(&mut list_ui, &lines, &look, &mut state, &to_host);
                    }
                });
            resize_by_edges(ui);
            keys(ui, &lines, &mut state, &to_host);
            match clicked {
                Some(Click::OnlyOpen) => {
                    shared.settings.only_open = !settings.only_open;
                    save(&to_host, ONLY_OPEN_KEY, shared.settings.only_open);
                }
                Some(Click::Pin) => {
                    shared.settings.stay_on_top = !settings.stay_on_top;
                    save(&to_host, STAY_ON_TOP_KEY, shared.settings.stay_on_top);
                }
                Some(Click::Close) => state.closed = true,
                None => {}
            }
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                state.closed = true;
            }
            if state.closed {
                shared.open = false;
                forget(&mut state);
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            ui.ctx()
                .request_repaint_after(next_frame(&state, &lines, now));
        });
    }
}

/// Keeps the place, and stops what only an open window needs: the jobs
/// it wanted read and the list of open chat windows.
fn forget(state: &mut DeckUi) {
    if state.window != 0 {
        keep_place(state);
    }
    claude_jobs::want_for_deck(Vec::new());
    claude_windows::publish(Vec::new());
    let kept = state.kept;
    *state = DeckUi {
        kept,
        ..DeckUi::default()
    };
}

fn save(to_host: &HostChannel, key: &str, on: bool) {
    to_host.send(HostRequest::SetSetting {
        plugin: ID.to_string(),
        key: key.to_string(),
        value: Value::Bool(on),
    });
}

/// Finds the window egui made, gives it rounded corners and a border, puts
/// it where it was, and takes the keyboard when the hotkey asked.
fn find_and_place(state: &mut DeckUi, palette: isize, tokens: &Tokens, to_host: &HostChannel) {
    if state.window == 0 {
        state.window = settings::find_window(TITLE, palette);
    }
    if state.window == 0 {
        return;
    }
    if state.border != Some(tokens.border) {
        state.border = Some(tokens.border);
        dwm_thumbs::round_corners(state.window as HWND, theme::colorref(tokens.border));
    }
    if !std::mem::replace(&mut state.placed, true) {
        if state.kept.is_none() {
            state.kept = place::load(&place::path());
        }
        match state.kept {
            Some(kept) => place::apply(state.window, kept),
            None => window_place::centre_on_cursor_monitor(state.window as HWND),
        }
        state.focus_wanted = true;
    }
    if std::mem::take(&mut state.focus_wanted) {
        to_host.send(HostRequest::FocusWindow(state.window));
    }
}

/// Asks the host for fresh data, and builds the chats again from what was
/// read last.
fn refresh(state: &mut DeckUi, to_host: &HostChannel) {
    if state.asked.is_none_or(|at| at.elapsed() >= ASK_EVERY) {
        state.asked = Some(Instant::now());
        to_host.send(HostRequest::ClaudeDeckRefresh);
    }
    if state.built.is_some_and(|at| at.elapsed() < BUILD_EVERY) {
        return;
    }
    state.built = Some(Instant::now());
    state.started.get_or_insert_with(Instant::now);
    state.animate = animations::on();
    let (sessions, _) = claude_sessions::sessions();
    let windows = claude_windows::latest();
    state.chats = chats(&sessions, &windows);
    state.archived = archived(&state.chats);
    keep_place(state);
}

fn chats(sessions: &[Session], windows: &[ChatWindow]) -> Vec<Chat> {
    let now = claude_sessions::now_ms();
    sessions
        .iter()
        .map(|session| {
            let window = claude_windows::of(windows, &session.id);
            let jobs = claude_jobs::running(&session.cli_session_id, now);
            model::chat(session, window, jobs, now)
        })
        .collect()
}

/// The titles of archived chats that forks hang under. Read only when a
/// fork's chat is not in the list.
fn archived(chats: &[Chat]) -> Archived {
    let listed: HashSet<&str> = chats.iter().map(|chat| chat.id.as_str()).collect();
    let missing: HashSet<&str> = chats
        .iter()
        .filter_map(|chat| chat.parent.as_deref())
        .filter(|parent| !listed.contains(parent))
        .collect();
    if missing.is_empty() {
        return Archived::new();
    }
    claude_sessions::sessions_with_archived()
        .into_iter()
        .filter(|session| missing.contains(session.id.as_str()))
        .map(|session| (session.id, session.title))
        .collect()
}

/// Saves the window's place and size when they changed.
fn keep_place(state: &mut DeckUi) {
    let Some(now) = place::read(state.window) else {
        return;
    };
    if state.kept != Some(now) {
        state.kept = Some(now);
        place::save(&place::path(), now);
    }
}

fn ring_turn(state: &DeckUi) -> f32 {
    match state.started {
        Some(at) if state.animate => {
            (at.elapsed().as_secs_f32() % TURN.as_secs_f32()) / TURN.as_secs_f32()
        }
        _ => 0.0,
    }
}

/// When to paint next: soon while a ring turns, each second while a time
/// counts seconds, else when fresh data is due.
fn next_frame(state: &DeckUi, lines: &[Line], now: u64) -> Duration {
    let shown = lines.iter().filter_map(|line| match line {
        Line::Chat(chat) => Some(&chat.chat),
        _ => None,
    });
    let mut wait = ASK_EVERY;
    for chat in shown {
        if state.animate && chat.mark == Mark::Working {
            return RING_STEP;
        }
        if chat.since.is_some_and(|at| now.saturating_sub(at) < 60_000) {
            wait = BUILD_EVERY;
        }
    }
    wait
}

fn empty(ui: &mut Ui, tokens: &Tokens, filtered: bool) {
    let words = if filtered { NONE_OPEN } else { EMPTY };
    let mut job = text::job(
        words,
        theme::regular(EMPTY_SIZE),
        tokens.text_secondary,
        None,
    );
    job.halign = egui::Align::Center;
    job.wrap.max_width = (ui.max_rect().width() - 36.0).max(0.0);
    let galley = ui.painter().layout_job(job);
    let rect = ui.max_rect();
    ui.painter().galley(
        pos2(rect.center().x, rect.top() + EMPTY_PAD),
        galley,
        Color32::PLACEHOLDER,
    );
}

/// The lines in a scrolling list, and what a click on one does.
fn list_lines(ui: &mut Ui, lines: &[Line], look: &Look, state: &mut DeckUi, to_host: &HostChannel) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let width = ui.available_width();
            let total: f32 = lines
                .iter()
                .enumerate()
                .map(|(at, line)| rows::height(line, at == 0))
                .sum::<f32>()
                + LIST_TOP_PAD
                + LIST_BOTTOM_PAD;
            let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
            let painter = ui.painter_at(area);
            let mut top = area.top() + LIST_TOP_PAD;
            for (at, line) in lines.iter().enumerate() {
                let height = rows::height(line, at == 0);
                let whole = Rect::from_min_size(pos2(area.left(), top), vec2(width, height));
                top += height;
                let rect = rows::body(line, whole);
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let id = ui.id().with(("claude-deck-line", at));
                let response = ui.interact(rect, id, Sense::click());
                let hovered = response.hovered();
                match line {
                    Line::Group(group) => {
                        rows::group(&painter, rect, group, look, hovered);
                        if response.clicked() {
                            flip(&mut state.folds.folded_groups, &group.key);
                        }
                    }
                    Line::Cold(cold) => {
                        rows::cold(&painter, rect, cold, look, hovered);
                        if response.clicked() {
                            state.folds.cold_open = !state.folds.cold_open;
                        }
                    }
                    Line::Chat(chat) => {
                        let selected = state.selected.as_deref() == Some(chat.chat.id.as_str());
                        rows::chat(&painter, rect, chat, look, hovered, selected);
                        if selected && std::mem::take(&mut state.scroll_to_selected) {
                            ui.scroll_to_rect(rect, None);
                        }
                        if !response.clicked() {
                            continue;
                        }
                        let on_arrow = chat.fold != model::Fold::None
                            && response.interact_pointer_pos().is_some_and(|at| {
                                rows::chevron_rect(rect, chat.depth).contains(at)
                            });
                        if on_arrow || chat.stand_in {
                            flip(&mut state.folds.folded_chats, &chat.chat.id);
                        } else {
                            state.selected = Some(chat.chat.id.clone());
                            bring_forward(chat.chat.window, to_host);
                        }
                    }
                }
            }
        });
}

fn flip(set: &mut HashSet<String>, key: &str) {
    if !set.remove(key) {
        set.insert(key.to_string());
    }
}

/// Brings a chat's window to the front through the plugin, which checks
/// that it is a Claude window and that this is not a test copy. A chat
/// with no window does nothing.
fn bring_forward(window: Option<isize>, to_host: &HostChannel) {
    if let Some(hwnd) = window {
        to_host.send(HostRequest::Arrange {
            plugin: ID.to_string(),
            action: ArrangeAction::Activate(hwnd),
        });
    }
}

/// Up and Down pick a chat; Enter brings its window to the front.
fn keys(ui: &Ui, lines: &[Line], state: &mut DeckUi, to_host: &HostChannel) {
    let (down, up, enter) = ui.ctx().input(|i| {
        (
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::Enter),
        )
    });
    let chats: Vec<&Chat> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Chat(chat) if !chat.stand_in => Some(&chat.chat),
            _ => None,
        })
        .collect();
    if chats.is_empty() {
        return;
    }
    let at = state
        .selected
        .as_deref()
        .and_then(|id| chats.iter().position(|chat| chat.id == id));
    let next = match (down, up, at) {
        (true, _, Some(at)) => Some((at + 1).min(chats.len() - 1)),
        (true, _, None) => Some(0),
        (_, true, Some(at)) => Some(at.saturating_sub(1)),
        (_, true, None) => Some(chats.len() - 1),
        _ => None,
    };
    if let Some(next) = next {
        state.selected = Some(chats[next].id.clone());
        state.scroll_to_selected = true;
    }
    if enter {
        if let Some(chat) = at.map(|at| chats[at]) {
            bring_forward(chat.window, to_host);
        }
    }
}

/// The edge or corner the mouse is on, for resizing.
fn edge(rect: Rect, at: egui::Pos2) -> Option<ResizeDirection> {
    let left = at.x - rect.left() < EDGE;
    let right = rect.right() - at.x < EDGE;
    let top = at.y - rect.top() < EDGE;
    let bottom = rect.bottom() - at.y < EDGE;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, ..) => Some(ResizeDirection::West),
        (_, true, ..) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}

fn resize_by_edges(ui: &Ui) {
    let ctx = ui.ctx();
    let rect = ctx.content_rect();
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let Some(direction) = edge(rect, at) else {
        return;
    };
    ctx.set_cursor_icon(cursor(direction));
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edges_and_corners_resize_and_the_middle_does_not() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 400.0));
        assert_eq!(edge(rect, pos2(1.0, 1.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(
            edge(rect, pos2(299.0, 399.0)),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(
            edge(rect, pos2(299.0, 1.0)),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            edge(rect, pos2(1.0, 399.0)),
            Some(ResizeDirection::SouthWest)
        );
        assert_eq!(edge(rect, pos2(150.0, 2.0)), Some(ResizeDirection::North));
        assert_eq!(edge(rect, pos2(150.0, 398.0)), Some(ResizeDirection::South));
        assert_eq!(edge(rect, pos2(2.0, 200.0)), Some(ResizeDirection::West));
        assert_eq!(edge(rect, pos2(298.0, 200.0)), Some(ResizeDirection::East));
        assert_eq!(edge(rect, pos2(150.0, 200.0)), None);
        assert_eq!(cursor(ResizeDirection::North), CursorIcon::ResizeVertical);
        assert_eq!(cursor(ResizeDirection::West), CursorIcon::ResizeHorizontal);
        assert_eq!(cursor(ResizeDirection::NorthWest), CursorIcon::ResizeNwSe);
        assert_eq!(cursor(ResizeDirection::SouthWest), CursorIcon::ResizeNeSw);
    }

    #[test]
    fn archived_titles_are_read_only_for_a_missing_parent() {
        let chat = |id: &str, parent: Option<&str>| Chat {
            id: id.into(),
            title: id.into(),
            folder: None,
            branch: None,
            model: None,
            parent: parent.map(str::to_string),
            created: 0,
            mark: Mark::Unknown,
            since: None,
            jobs: 0,
            window: None,
        };
        assert!(archived(&[chat("a", None), chat("b", Some("a"))]).is_empty());
    }
}
