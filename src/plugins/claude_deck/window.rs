//! ClaudeDeck's own window, drawn with egui on the UI thread: a slim frame
//! with no Windows title bar, the header, and the list. It is dragged by
//! the header, resized by its edges, and comes back at the same place and
//! size. While it is open it asks the host for fresh data every few
//! seconds; it repaints only for that, for the times, for the turning
//! ring of a working chat when Windows allows animations, and for the
//! short fade step. The list's lines are built again only when the chats,
//! the arrangement or the view changed.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{pos2, vec2, Color32, Rect, Ui};
use serde_json::Value;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};

use crate::core::claude_sessions;
use crate::core::claude_windows;
use crate::core::theme::{self, Tokens};
use crate::core::traits::{PluginWindow, WindowFrame};
use crate::core::ui_bridge::{ArrangeAction, HostChannel, HostRequest};
use crate::core::{animations, claude_jobs, dpi_follow, window_place};
use crate::ui::dwm_thumbs;
use crate::ui::settings;
use crate::ui::widgets::{focus, text};

use super::arrange::{arrange, hidden_chats};
use super::data;
use super::edges;
use super::fade::Fade;
use super::header::{self, Click};
use super::keys;
use super::list;
use super::mini;
use super::model::{self, Archived, Chat, Line, Mark, View};
use super::modes::{fade_step, give_back, keep_min_size, min_size, switch_mode, tell_tray};
use super::place::{self, Place};
use super::quick;
use super::rows::Look;
use super::select;
use super::servers::{self, Deck};
use super::status::Seen;
use super::store::{self, Saved};
use super::taskbar;
use super::{Handle, ID, MINI_KEY, ONLY_OPEN_KEY, STAY_ON_TOP_KEY};

pub const WINDOW_ID: &str = "deck";
const TITLE: &str = "WinCraft \u{b7} ClaudeDeck";
pub(super) const SIZE: [f32; 2] = [360.0, 480.0];
pub(super) const MIN_SIZE: [f32; 2] = [220.0, 90.0];
/// How often the host is asked to read the chats again; the reader itself
/// reads at most this often too.
const ASK_EVERY: Duration = claude_sessions::READ_EVERY;
/// How often the list is built again from what was read.
const BUILD_EVERY: Duration = Duration::from_secs(1);
/// The Working ring: one turn in this time, drawn in this many steps.
const TURN: Duration = Duration::from_secs(2);
/// How often the list looks which window is in front.
const FRONT_EVERY: Duration = Duration::from_millis(400);
const RING_STEP: Duration = Duration::from_millis(100);
const EMPTY_SIZE: f32 = 13.0;
const EMPTY_PAD: f32 = 28.0;

const EMPTY: &str = "No Claude chats yet.\nChats show here when Claude Code runs.";
const NONE_OPEN: &str = "No chat has a window open.";

/// A line the keyboard or a drag acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Pick {
    Chat(String),
    Group(String),
}

/// A group whose name is being changed.
pub(super) struct Rename {
    pub id: String,
    pub text: String,
    /// The text is to be selected whole on the next frame.
    pub fresh: bool,
    /// The field has had the keyboard; losing it again saves.
    pub focused: bool,
}

/// The lines built last, and what they were built from.
#[derive(Default)]
pub(super) struct Cache {
    built: Option<Instant>,
    saved: Saved,
    view: (bool, bool, bool),
    pub(super) lines: Arc<Vec<Line>>,
    pub(super) counts: Vec<(Mark, usize)>,
    pub(super) waiting: usize,
    pub(super) waiting_ids: Vec<String>,
}

#[derive(Default)]
pub(super) struct DeckUi {
    pub window: isize,
    placed: bool,
    focus_wanted: bool,
    closed: bool,
    asked: Option<Instant>,
    pub(super) built: Option<Instant>,
    pub chats: Vec<Chat>,
    archived: Archived,
    pub(super) animate: bool,
    /// The user's arrangement, and the copy last written to its file
    /// (None until the file was read).
    pub saved: Saved,
    written: Option<Saved>,
    writable: bool,
    pub hidden_open: bool,
    pub selected: Option<Pick>,
    pub scroll_to_selected: bool,
    pub drag: Option<Pick>,
    pub renaming: Option<Rename>,
    /// A rename ended this frame: its Enter or Esc is not the list's.
    pub renamed: bool,
    /// The keyboard asked for this line's menu.
    pub menu_for: Option<Pick>,
    pub menu_at: Option<egui::Pos2>,
    pub menu_key_down: bool,
    border: Option<Color32>,
    pub(super) kept: Option<Place>,
    pub(super) started: Option<Instant>,
    seen: Seen,
    /// The keyboard is in use: the rows show their digits.
    pub keyboard: bool,
    /// The chat ids the digits 1 to 9 pick, top to bottom.
    pub digit_rows: Vec<String>,
    /// The hotkey opened the window: select the row it should.
    select_on_open: bool,
    /// The window in front when the list last looked, once chats are in.
    front_seen: Option<isize>,
    /// The window in front when the hotkey was pressed.
    pub(super) before: Option<isize>,
    /// The mode the window has now; None before the first frame.
    pub(super) mini_on: Option<bool>,
    pub(super) mini_size: Option<egui::Vec2>,
    pub(super) quick: quick::Quick,
    /// The mode and scale the smallest size was last sent for.
    pub(super) min_for: Option<(bool, f32)>,
    pub(super) fade: Fade,
    pub(super) waiting_before: Vec<String>,
    pub(super) cache: Cache,
    pub(super) tip_sent: Option<Option<String>>,
    /// The homelab servers; dropping it stops reading them.
    pub(super) servers: Deck,
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
    /// it is already in front. Brought by the hotkey, it selects the chat
    /// that waits longest and remembers the window that was in front.
    fn open(&mut self) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let by_hotkey = shared.by_hotkey.take();
        let keep_open = std::mem::take(&mut shared.keep_open);
        if let Ok(mut state) = self.state.lock() {
            let in_front =
                state.window != 0 && unsafe { GetForegroundWindow() } as isize == state.window;
            if in_front && !keep_open {
                state.closed = true;
            } else {
                state.focus_wanted = true;
            }
            if let Some(before) = by_hotkey.filter(|_| !in_front) {
                state.before = Some(before).filter(|hwnd| *hwnd != state.window);
                state.select_on_open = true;
                state.keyboard = true;
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
        let mini = self.shared.lock().is_ok_and(|shared| shared.settings.mini);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_resizable(true)
            .with_inner_size(SIZE)
            .with_min_inner_size(min_size(mini))
            .with_window_level(if on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            });
        builder = builder.with_icon(taskbar::window_icon());
        let shared = Arc::clone(&self.shared);
        let state = Arc::clone(&self.state);
        let palette = frame.palette_hwnd;
        let to_host = Arc::clone(&frame.to_host);
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let (Ok(mut shared), Ok(mut state)) = (shared.lock(), state.lock()) else {
                return;
            };
            // egui can draw the window once more after it was closed. That
            // frame must not find the dying window and mark it as placed.
            if !shared.open {
                return;
            }
            let tokens = Tokens::get(ui.ctx());
            find_and_place(&mut state, palette, &tokens, &to_host);
            let repaint = ui.ctx().clone();
            let wake = Arc::new(move || repaint.request_repaint_of(viewport_id()));
            state.servers.tick(&shared.settings.servers, wake);
            refresh(&mut state, &to_host);
            let settings = shared.settings.clone();
            let now = claude_sessions::now_ms();
            build(&mut state, &settings, now);
            let lines = Arc::clone(&state.cache.lines);
            let counts = state.cache.counts.clone();
            if state.select_on_open && !state.chats.is_empty() {
                state.select_on_open = false;
                state.selected = select::open_pick(&lines, state.selected.as_ref());
                state.scroll_to_selected = true;
            }
            follow_front(&mut state, &lines);
            keep_min_size(ui.ctx(), &mut state, settings.mini);
            switch_mode(ui.ctx(), &mut state, settings.mini, &lines);
            let turn = ring_turn(&state);
            let menu_was_open = egui::Popup::is_any_open(ui.ctx());
            let mut clicked = None;
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(tokens.window_bg))
                .show(ui, |ui| {
                    let full = ui.max_rect();
                    let look = Look {
                        tokens: &tokens,
                        settings: &settings,
                        now,
                        turn,
                    };
                    if settings.mini {
                        if mini::show(ui, &lines, &look, &mut state, &to_host) {
                            clicked = Some(Click::Mini);
                        }
                        return;
                    }
                    let top = Rect::from_min_size(full.min, vec2(full.width(), header::HEIGHT));
                    clicked =
                        header::show(ui, top, &counts, &settings, &tokens, turn, &mut state.quick);
                    let list = Rect::from_min_max(pos2(full.left(), top.bottom()), full.max);
                    let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
                    if !servers::draw::has_lines(&lines, &state) {
                        empty(&mut list_ui, &tokens, !state.chats.is_empty());
                    } else {
                        list::show(&mut list_ui, &lines, &look, &mut state, &to_host);
                    }
                });
            if settings.mini {
                state.digit_rows = mini::first_nine(&lines);
            } else {
                edges::resize(ui);
            }
            if ui.ctx().input(|i| i.pointer.any_click() || !i.focused) {
                state.keyboard = false;
            }
            let asked = keys::keys(ui, &lines, &mut state, &to_host, menu_was_open);
            if asked.give_back {
                give_back(&mut state, &to_host);
            }
            if asked.mini {
                clicked = Some(Click::Mini);
            }
            if asked.quick || std::mem::take(&mut shared.quick_wanted) {
                state.quick.by_key = true;
            }
            if let Some(pick) = quick::show(ui, &mut state, &settings, &tokens) {
                clicked = quick::apply(&mut shared.settings, pick, &to_host).or(clicked);
            }
            commit(&mut state);
            match clicked {
                Some(Click::OnlyOpen) => {
                    shared.settings.only_open = !settings.only_open;
                    save(&to_host, ONLY_OPEN_KEY, shared.settings.only_open);
                }
                Some(Click::Pin) => {
                    shared.settings.stay_on_top = !settings.stay_on_top;
                    save(&to_host, STAY_ON_TOP_KEY, shared.settings.stay_on_top);
                }
                Some(Click::Mini) => {
                    shared.settings.mini = !settings.mini;
                    save(&to_host, MINI_KEY, shared.settings.mini);
                    ui.ctx().request_repaint();
                }
                Some(Click::Close) => state.closed = true,
                None => {}
            }
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                state.closed = true;
            }
            if state.closed {
                shared.open = false;
                tell_tray(&mut shared, &mut state, None);
                forget(&mut state);
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                return;
            }
            let tip = select::tray_tip(settings.tray_count, state.cache.waiting);
            tell_tray(&mut shared, &mut state, tip);
            let fade_next = fade_step(ui.ctx(), &mut state, settings.fade);
            let next = next_frame(&state, &lines, now);
            ui.ctx().request_repaint_after(
                fade_next
                    .map_or(next, |fade| fade.min(next))
                    .min(FRONT_EVERY),
            );
        });
    }
}

/// Builds the lines again when the chats, the arrangement or the view
/// changed since they were built last.
pub(super) fn build(state: &mut DeckUi, settings: &super::Settings, now: u64) {
    let view_now = (settings.only_open, settings.fold_cold, state.hidden_open);
    let cache = &state.cache;
    let same = cache.built.is_some() && cache.built == state.built && cache.view == view_now;
    if same && cache.saved == state.saved {
        return;
    }
    let view = View {
        only_open: settings.only_open,
        fold_cold: settings.fold_cold,
        hidden_open: state.hidden_open,
        saved: state.saved.clone(),
    };
    let lines = model::lines(&state.chats, &state.archived, &view, now);
    let listed = listed(state);
    state.cache = Cache {
        built: state.built,
        saved: view.saved,
        view: view_now,
        lines: Arc::new(lines),
        counts: model::counts(&listed, settings.only_open),
        waiting: select::waiting(&listed, settings.only_open),
        waiting_ids: select::waiting_ids(&listed),
    };
}

/// Keeps the place, and stops what only an open window needs: the jobs
/// it wanted read and the list of open chat windows.
fn forget(state: &mut DeckUi) {
    if state.window != 0 {
        keep_place(state);
        taskbar::release(state.window);
    }
    claude_windows::set_wanted(false);
    claude_jobs::want_for_deck(Vec::new());
    claude_windows::publish(Vec::new());
    commit(state);
    *state = DeckUi {
        kept: state.kept,
        saved: std::mem::take(&mut state.saved),
        written: state.written.take(),
        writable: state.writable,
        // The row used last is selected when the hotkey opens it again.
        selected: state.selected.take(),
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
    if state.window != 0 && unsafe { IsWindow(state.window as HWND) } == 0 {
        // A window from before the last close: the new one is not placed yet.
        state.window = 0;
        state.placed = false;
    }
    if state.window == 0 {
        state.window = settings::find_window(TITLE, palette);
    }
    if state.window == 0 {
        return;
    }
    dpi_follow::follow(state.window);
    if state.border != Some(tokens.border) {
        state.border = Some(tokens.border);
        dwm_thumbs::round_corners(state.window as HWND, theme::colorref(tokens.border));
    }
    if !std::mem::replace(&mut state.placed, true) {
        taskbar::own_button(state.window);
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
        claude_windows::set_wanted(true);
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
    let now = claude_sessions::now_ms();
    state.seen.note(&windows, claude_windows::foreground(), now);
    let (chats, waits) = data::chats(
        &sessions,
        &windows,
        &state.seen,
        state.servers.aliases(),
        now,
    );
    state.chats = chats;
    state.servers.waits = waits;
    servers::busy::warn_pc(&mut state.chats, &state.servers.views());
    state.archived = data::archived(&state.chats);
    if state.written.is_none() {
        let (saved, writable) = store::load(&store::path()).start();
        state.written = Some(saved.clone());
        state.saved = saved;
        state.writable = writable;
    }
    // Before the first read there are no chats yet; arranging then would
    // count every saved chat as gone.
    if !state.chats.is_empty() {
        state.saved = arrange(&state.saved, &state.chats, now);
    }
    keep_place(state);
}

/// Writes the arrangement when it changed.
pub(super) fn commit(state: &mut DeckUi) {
    if !state.writable || state.written.as_ref() == Some(&state.saved) || state.written.is_none() {
        return;
    }
    store::save(&store::path(), &state.saved);
    state.written = Some(state.saved.clone());
}

/// The chats that are not hidden, for the header's counts.
pub(super) fn listed(state: &DeckUi) -> Vec<Chat> {
    let hidden = hidden_chats(&state.chats, &state.saved.hidden);
    state
        .chats
        .iter()
        .enumerate()
        .filter(|(at, _)| !hidden.contains(at))
        .map(|(_, chat)| chat.clone())
        .collect()
}

/// When another window comes to the front and it is a shown chat's window,
/// that chat's row becomes the marked one. The user then sees in the list
/// which chat they work in. A menu, a rename or a drag is left alone.
fn follow_front(state: &mut DeckUi, lines: &[Line]) {
    if state.chats.is_empty() {
        return;
    }
    let front = unsafe { GetForegroundWindow() } as isize;
    if state.front_seen.replace(front) == Some(front) {
        return;
    }
    let busy = state.drag.is_some() || state.renaming.is_some() || state.menu_for.is_some();
    if let Some(pick) = select::front_pick(lines, front).filter(|_| !busy) {
        if state.selected.as_ref() != Some(&pick) {
            state.selected = Some(pick);
            state.scroll_to_selected = true;
        }
    }
}

/// Saves the window's place and size when they changed; never the
/// column's, so the full place comes back after mini mode.
pub(super) fn keep_place(state: &mut DeckUi) {
    if state.mini_on == Some(true) {
        return;
    }
    let Some(now) = place::read(state.window) else {
        return;
    };
    if state.kept != Some(now) {
        state.kept = Some(now);
        place::save(&place::path(), now);
    }
}

pub(super) fn ring_turn(state: &DeckUi) -> f32 {
    match state.started {
        Some(at) if state.animate => {
            (at.elapsed().as_secs_f32() % TURN.as_secs_f32()) / TURN.as_secs_f32()
        }
        _ => 0.0,
    }
}

/// When to paint next: soon while a ring turns, each second while a time
/// counts seconds, else when fresh data is due.
pub(super) fn next_frame(state: &DeckUi, lines: &[Line], now: u64) -> Duration {
    let shown = lines.iter().filter_map(|line| match line {
        Line::Chat(chat) => Some(&chat.chat),
        _ => None,
    });
    // The first read takes a moment; look again soon until it is in.
    let mut wait = if state.chats.is_empty() {
        BUILD_EVERY
    } else {
        ASK_EVERY
    };
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

pub(super) fn empty(ui: &mut Ui, tokens: &Tokens, filtered: bool) {
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

/// Brings a chat's window to the front through the plugin, which checks
/// that it is a Claude window and that this is not a test copy. A chat
/// with no window does nothing.
/// Opens WinCraft's rename box for the chat's window, as **Rename…** does
/// in WindowDeck. The name is the window's, kept by WindowNamer.
pub(super) fn rename_window(window: Option<isize>, to_host: &HostChannel) {
    if let Some(hwnd) = window {
        to_host.send(HostRequest::Arrange {
            plugin: ID.to_string(),
            action: ArrangeAction::AskName(hwnd),
        });
    }
}

pub(super) fn bring_forward(window: Option<isize>, to_host: &HostChannel) {
    if let Some(hwnd) = window {
        to_host.send(HostRequest::Arrange {
            plugin: ID.to_string(),
            action: ArrangeAction::Activate(hwnd),
        });
    }
}
