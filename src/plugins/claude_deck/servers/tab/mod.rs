//! ClaudeDeck's Servers tab: one folding card per listed server, in the
//! setting's order, with its health on the header and the sections Chats,
//! Claude processes, Busiest and Services. It only reads: the actions are
//! a terminal on the server, a fork of a chat, and the copy items. The tab
//! shows a red count of problems (offline servers, failed or restarting
//! services and containers, `claude` processes with no chat).
//!
//! - `view`: the lines, the words and the problems (pure).
//! - `draw`: drawing the cards.
//! - `keys`: the keyboard on the tab.

pub mod draw;
pub mod keys;
pub mod view;

use std::collections::BTreeSet;

/// What the arrow keys stop on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Card(String),
    Chat(String),
    /// The quiet chats' line of a server.
    Quiet(String),
    /// The hidden chats' line of a server.
    Hidden(String),
    Proc {
        server: String,
        pid: u32,
        claude: bool,
    },
    Service {
        server: String,
        name: String,
    },
}

/// What Enter does on a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enter {
    /// Opens a terminal on the server, in this folder or the home folder.
    Terminal(Option<String>),
    FoldQuiet,
    FoldHidden,
    Nothing,
}

/// What a line acts on, kept while drawing for the keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Act {
    pub server: String,
    pub enter: Enter,
    /// What Ctrl+C copies.
    pub copy: String,
}

/// The tab's own state.
#[derive(Default)]
pub struct TabUi {
    /// The servers whose cards the user folded.
    pub folded: BTreeSet<String>,
    pub selected: Option<Pick>,
    pub scroll_to_selected: bool,
    /// The lines the arrow keys reach, top to bottom, as drawn last.
    pub picks: Vec<(Pick, Act)>,
}

impl TabUi {
    /// Unfolds a server's card and puts the keyboard on its header, so the
    /// next frame scrolls to it.
    pub fn focus(&mut self, server: &str) {
        self.folded.remove(server);
        self.selected = Some(Pick::Card(server.to_string()));
        self.scroll_to_selected = true;
    }

    pub fn fold(&mut self, server: &str, folded: bool) {
        if folded {
            self.folded.insert(server.to_string());
        } else {
            self.folded.remove(server);
        }
    }

    pub fn act(&self, pick: &Pick) -> Option<&Act> {
        self.picks
            .iter()
            .find(|(keyed, _)| keyed == pick)
            .map(|(_, act)| act)
    }
}

#[cfg(test)]
mod tests;
