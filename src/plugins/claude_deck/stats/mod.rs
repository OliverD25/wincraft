//! ClaudeDeck's Stats tab and the thin token bar under each chat row:
//! how many tokens each chat used today and in the last 7 days, weighed
//! by each model's API price (`core::claude_usage`). Only chats whose
//! transcripts are on this PC are counted; server chats are not.
//!
//! - `view`: the numbers, names and words (pure).
//! - `draw`: drawing the tab.
//! - `bar`: the thin bar under a chat row.
//!
//! The reader runs while the window reads its chats and either the tab is
//! open or the token bar is on; it stops when the window closes.

pub mod bar;
pub mod draw;
pub mod view;

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::claude_sessions::Session;
use crate::core::claude_usage::{Reader, Snapshot};

/// Which span the tab shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Spans {
    Today,
    #[default]
    Week,
}

/// The Stats part of the window's state.
#[derive(Default)]
pub struct StatsUi {
    reader: Option<Reader>,
    /// The newest summary read.
    pub latest: Option<Arc<Snapshot>>,
    pub span: Spans,
    /// The token bar setting, as drawn last.
    pub bar_on: bool,
    /// The tab was open at the last tick.
    tab_was_open: bool,
    /// ClaudeDeck's chat id for each Claude Code session id.
    pub chat_of: HashMap<String, String>,
    /// Each chat's share of the 7-day weight against the biggest chat.
    shares: HashMap<String, f32>,
}

impl StatsUi {
    /// Called each time the window reads its chats. Starts, feeds or stops
    /// the reader and takes its newest summary.
    pub fn tick(&mut self, sessions: &[Session], tab_open: bool) {
        self.chat_of = sessions
            .iter()
            .map(|s| (s.cli_session_id.clone(), s.id.clone()))
            .collect();
        let wanted = tab_open || self.bar_on;
        if !wanted {
            self.reader = None;
            self.tab_was_open = false;
            return;
        }
        if self.reader.is_none() {
            self.reader = start_reader();
        }
        if let Some(reader) = &self.reader {
            if tab_open && !self.tab_was_open {
                reader.now_please();
            } else {
                reader.want();
            }
            if let Some(latest) = reader.latest() {
                self.latest = Some(latest);
            }
        }
        self.tab_was_open = tab_open;
        self.share_again();
    }

    /// Sets the summary by hand, for tests.
    #[cfg(test)]
    pub fn set_latest(&mut self, snapshot: Snapshot) {
        self.latest = Some(Arc::new(snapshot));
        self.share_again();
    }

    #[cfg(test)]
    pub fn is_reading(&self) -> bool {
        self.reader.is_some()
    }

    /// About a hundred chats: cheap enough for every read of the chats.
    fn share_again(&mut self) {
        if let Some(latest) = &self.latest {
            self.shares = view::shares(&latest.week, &self.chat_of);
        }
    }

    /// A chat's bar length, 0 to 1; None when it used nothing this week.
    pub fn share(&self, chat_id: &str) -> Option<f32> {
        self.shares.get(chat_id).copied()
    }
}

/// Left picks Today, Right picks 7 days.
pub fn keys(ui: &egui::Ui, stats: &mut StatsUi) {
    ui.ctx().input(|i| {
        if i.key_pressed(egui::Key::ArrowLeft) {
            stats.span = Spans::Today;
        }
        if i.key_pressed(egui::Key::ArrowRight) {
            stats.span = Spans::Week;
        }
    });
}

/// The real reader; none in tests, so they never read the user's chats.
fn start_reader() -> Option<Reader> {
    if cfg!(test) {
        None
    } else {
        Reader::start()
    }
}

#[cfg(test)]
mod tests;
