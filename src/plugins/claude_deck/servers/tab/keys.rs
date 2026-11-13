//! The keyboard on the Servers tab. Up and Down move over the card
//! headers and the rows, Home and End go to the first and the last, Esc
//! gives the keyboard back. On a header Left folds the card, Right
//! unfolds it, Space does either, and Enter opens a terminal in the
//! server's home folder. On a row Left goes to its card's header. On a
//! chat Enter opens a terminal in its folder, as on the Chats tab; on the
//! quiet or hidden chats' line Enter or Space folds and unfolds it.
//! Ctrl+C copies the name: the server's, the chat's or the service's, or
//! a process's command line.

use egui::{Key, Ui};

use crate::plugins::claude_deck::keys::Asked;
use crate::plugins::claude_deck::window::DeckUi;

use super::super::draw::{copy, toggle};
use super::super::terminal;
use super::{Enter, Pick};

/// The key after (or before, `up`) the current one; the first or the
/// last when none is picked.
pub fn step(picks: &[Pick], current: Option<&Pick>, up: bool) -> Option<Pick> {
    if picks.is_empty() {
        return None;
    }
    let at = current.and_then(|pick| picks.iter().position(|p| p == pick));
    let next = match (up, at) {
        (false, Some(at)) => (at + 1).min(picks.len() - 1),
        (false, None) => 0,
        (true, Some(at)) => at.saturating_sub(1),
        (true, None) => picks.len() - 1,
    };
    picks.get(next).cloned()
}

/// The keys of one frame on the tab. `busy`: a menu or a rename has them.
pub fn keys(ui: &Ui, state: &mut DeckUi, busy: bool) -> Asked {
    if busy {
        return Asked::default();
    }
    let (down, up, home, end, left, right, space, enter, esc, copy_key, mini, quick) =
        ui.ctx().input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                i.modifiers.command && i.key_pressed(Key::M),
                i.modifiers.command && i.key_pressed(Key::Comma),
            )
        });
    let asked = Asked {
        give_back: esc,
        mini,
        quick,
    };
    let tab = &mut state.servers.tab;
    let picks: Vec<Pick> = tab.picks.iter().map(|(pick, _)| pick.clone()).collect();
    let next = if up || down {
        step(&picks, tab.selected.as_ref(), up)
    } else if home {
        picks.first().cloned()
    } else if end {
        picks.last().cloned()
    } else {
        None
    };
    if let Some(next) = next {
        tab.selected = Some(next);
        tab.scroll_to_selected = true;
        state.keyboard = true;
    }
    let Some(picked) = tab.selected.clone().filter(|pick| picks.contains(pick)) else {
        return asked;
    };
    let Some(act) = tab.act(&picked).cloned() else {
        return asked;
    };
    if copy_key && !act.copy.is_empty() {
        copy(state.window, &act.copy);
    }
    if let Pick::Card(server) = &picked {
        let folded = tab.folded.contains(server);
        if left && !folded {
            tab.fold(server, true);
        } else if right && folded {
            tab.fold(server, false);
        } else if space {
            tab.fold(server, !folded);
        }
    } else if left {
        tab.selected = Some(Pick::Card(act.server.clone()));
        tab.scroll_to_selected = true;
    }
    match act.enter {
        Enter::Terminal(folder) if enter => terminal::open(&act.server, folder.as_deref()),
        Enter::FoldQuiet if enter || space => toggle(&mut state.servers.quiet_open, &act.server),
        Enter::FoldHidden if enter || space => toggle(&mut state.servers.hidden_open, &act.server),
        _ => {}
    }
    asked
}
