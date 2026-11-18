//! ClaudeDeck's Stats tab and the token mark on each chat row:
//! how many tokens each chat used today and in the last 7 days, weighed
//! by each model's API price (`core::claude_usage`). Only chats whose
//! transcripts are on this PC are counted; server chats are not.
//!
//! - `view`: the numbers, names and words (pure).
//! - `split`: the Top chats bars cut by model, and their colours.
//! - `draw`: drawing the tab.
//! - `bar`: the token mark on a chat row.
//!
//! The reader runs while the window reads its chats and either the tab is
//! open or the token mark is on; it stops when the window closes.

pub mod bar;
pub mod draw;
pub mod split;
pub mod view;

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::claude_sessions::Session;
use crate::core::claude_usage::{Reader, Snapshot};

use super::token_mark::Counts;
use view::Share;

/// Each listed chat's token mark, for both spans. The window hands it to
/// the rows each frame.
#[derive(Clone, Debug, Default)]
pub struct RowShares {
    week: HashMap<String, Share>,
    today: HashMap<String, Share>,
}

impl RowShares {
    /// A chat's mark; None when it used nothing in that time.
    pub fn get(&self, chat_id: &str, counts: Counts) -> Option<Share> {
        let map = match counts {
            Counts::Today => &self.today,
            Counts::Week => &self.week,
        };
        map.get(chat_id).copied()
    }
}

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
    /// The token mark is on, as drawn last.
    pub bar_on: bool,
    /// The tab was open at the last tick.
    tab_was_open: bool,
    /// ClaudeDeck's chat id for each Claude Code session id.
    pub chat_of: HashMap<String, String>,
    /// Each chat's token mark, for the rows.
    pub rows: Arc<RowShares>,
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
            self.rows = Arc::new(RowShares {
                week: view::shares(&latest.week, &self.chat_of),
                today: view::shares(&latest.today_span, &self.chat_of),
            });
        }
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
