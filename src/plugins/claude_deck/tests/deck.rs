//! A whole ClaudeDeck window frame on the headless screen: the header,
//! the list or the mini column, and the keys, the way the window runs
//! them, on chats made here.

use std::sync::Arc;

use egui::{Event, Key, Modifiers, Pos2, Ui};

use super::screen::{Painted, Screen};
use super::*;
use crate::core::config::ThemeChoice;
use crate::core::ui_bridge::{self, Bridge, HostRequest};
use crate::plugins::claude_deck::header::{self, Click};
use crate::plugins::claude_deck::rows::Look;
use crate::plugins::claude_deck::window::{self, DeckUi};
use crate::plugins::claude_deck::{keys, list, mini, Settings};

pub struct Deck {
    pub screen: Screen,
    pub state: DeckUi,
    pub bridge: Bridge,
    pub settings: Settings,
    pub clicked: Vec<Click>,
    pub asked: Vec<keys::Asked>,
}

/// Chats of three projects: a chat in "New", a folder with a fork, a cold
/// chat and a stand-in; the second project has a chat with a window.
pub fn sample() -> Vec<Chat> {
    let mut working = chat("b1", Some(r"E:\b\Beta"), 5);
    working.mark = Mark::Working;
    working.jobs = 2;
    working.window = Some(1234);
    working.text = Some("Reading the files".into());
    working.full = Some("Reading the files, then the tests.".into());
    let mut asks = chat("a2", Some(r"E:\a\Alpha"), 3);
    asks.mark = Mark::AsksYou;
    asks.text = Some("Which one?".into());
    let mut child = fork("f", "a1", 4);
    child.folder = Some(r"E:\a\Alpha".into());
    let mut orphan = fork("x", "gone", 6);
    orphan.folder = Some(r"E:\b\Beta".into());
    vec![
        chat("n", None, 1),
        chat("a1", Some(r"E:\a\Alpha"), 2),
        asks,
        child,
        working,
        orphan,
        cold("old", Some(r"E:\a\Alpha"), 5),
        chat("h", Some(r"E:\b\Beta"), 7),
    ]
}

impl Deck {
    pub fn new(chats: Vec<Chat>, settings: Settings) -> Self {
        let mut state = DeckUi::default();
        state.chats = chats;
        state.saved = arrange(&state.saved, &state.chats, NOW);
        let mut deck = Self {
            screen: Screen::new(ThemeChoice::Dark),
            state,
            bridge: ui_bridge::create(),
            settings,
            clicked: Vec::new(),
            asked: Vec::new(),
        };
        deck.build();
        deck
    }

    pub fn build(&mut self) {
        window::build(&mut self.state, &self.settings, NOW);
    }

    pub fn lines(&self) -> Arc<Vec<Line>> {
        Arc::clone(&self.state.cache.lines)
    }

    pub fn requests(&self) -> Vec<HostRequest> {
        self.bridge.host_rx.try_iter().collect()
    }

    pub fn frame(&mut self, events: Vec<Event>) -> Painted {
        self.settle_with(events)
    }

    fn settle_with(&mut self, events: Vec<Event>) -> Painted {
        let Self {
            screen,
            state,
            bridge,
            settings,
            clicked,
            asked,
        } = self;
        let mut draw = |ui: &mut Ui| window_frame(ui, state, bridge, settings, clicked, asked);
        screen.frame(events, &mut draw)
    }

    pub fn settle(&mut self) -> Painted {
        let mut last = self.frame(Vec::new());
        for _ in 0..2 {
            last = self.frame(Vec::new());
        }
        last
    }

    pub fn click(&mut self, at: Pos2) -> Painted {
        self.pointer(at, egui::PointerButton::Primary)
    }

    pub fn right_click(&mut self, at: Pos2) -> Painted {
        self.pointer(at, egui::PointerButton::Secondary)
    }

    fn pointer(&mut self, at: Pos2, button: egui::PointerButton) -> Painted {
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![Event::PointerMoved(at)]);
        let press = |pressed| Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(vec![press(true)]);
        self.frame(vec![press(false)]);
        self.frame(Vec::new())
    }

    pub fn key(&mut self, key: Key) -> Painted {
        self.key_with(key, Modifiers::NONE)
    }

    pub fn key_with(&mut self, key: Key, modifiers: Modifiers) -> Painted {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        let painted = self.frame(vec![
            Event::ModifiersChanged(modifiers),
            event(true),
            event(false),
        ]);
        self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        painted
    }

    /// Lets the pointer rest where it is, frame after frame.
    pub fn rest(&mut self, at: Pos2, frames: usize) -> Painted {
        self.frame(vec![Event::PointerMoved(at)]);
        let mut last = self.frame(Vec::new());
        for _ in 0..frames {
            last = self.frame(Vec::new());
        }
        last
    }
}

/// What the window's panel does in one frame, without the native window.
fn window_frame(
    ui: &mut Ui,
    state: &mut DeckUi,
    bridge: &Bridge,
    settings: &Settings,
    clicked: &mut Vec<Click>,
    asked: &mut Vec<keys::Asked>,
) {
    let tokens = crate::core::theme::Tokens::get(ui.ctx());
    window::build(state, settings, NOW);
    let lines = Arc::clone(&state.cache.lines);
    let counts = state.cache.counts.clone();
    let turn = window::ring_turn(state);
    let menu_was_open = egui::Popup::is_any_open(ui.ctx());
    let mut hit = None;
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(tokens.window_bg))
        .show(ui, |ui| {
            let full = ui.max_rect();
            let look = Look {
                tokens: &tokens,
                settings,
                now: NOW,
                turn,
            };
            if settings.mini {
                if mini::show(ui, &lines, &look, state, &bridge.to_host) {
                    hit = Some(Click::Mini);
                }
                return;
            }
            let top = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), header::HEIGHT));
            hit = header::show(ui, top, &counts, settings, &tokens, turn);
            let rest = egui::Rect::from_min_max(egui::pos2(full.left(), top.bottom()), full.max);
            let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(rest));
            if lines.is_empty() {
                window::empty(&mut list_ui, &tokens, !state.chats.is_empty());
            } else {
                list::show(&mut list_ui, &lines, &look, state, &bridge.to_host);
            }
        });
    if settings.mini {
        state.digit_rows = mini::first_nine(&lines);
    }
    if ui.ctx().input(|i| i.pointer.any_click() || !i.focused) {
        state.keyboard = false;
    }
    let got = keys::keys(ui, &lines, state, &bridge.to_host, menu_was_open);
    if got != keys::Asked::default() {
        asked.push(got);
    }
    hit.into_iter().for_each(|click| clicked.push(click));
}
