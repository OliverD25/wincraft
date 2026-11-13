//! The keyboard on the tabs (design sections 13 and 14). Ctrl+Tab and
//! Ctrl+Shift+Tab go round Chats, News and Servers (while a server is
//! listed) on any tab; the Servers tab's own keys are in `servers::tab`. On News:
//! Up and Down move over the group lines, the chats' block lines and the
//! replies ("N earlier replies" is skipped), Home and End go to the first
//! and the last, Esc gives the keyboard back. On a group line Right
//! unfolds, Left folds, Enter or Space does both. On a block line Right
//! opens the chat one step more (collapsed, then the default, then every
//! reply), Left collapses it to its block line, Space does what the arrow
//! does, Enter opens the chat.
//! On a reply Space or Right opens it in place, Left closes it, Enter
//! opens the chat. The digits 1 to 9 stay on the Chats tab.

use egui::{Key, Ui};

use crate::core::ui_bridge::HostChannel;

use super::super::keys::{self, Asked};
use super::super::model::Line as ChatLine;
use super::super::window::DeckUi;
use super::draw::{open_block, open_item};
use super::view::{self, Folds, Head};
use super::Tab;

#[derive(Default)]
struct Pressed {
    down: bool,
    up: bool,
    home: bool,
    end: bool,
    left: bool,
    right: bool,
    space: bool,
    enter: bool,
    esc: bool,
    mini: bool,
    quick: bool,
}

/// The keys of one frame, on whichever tab is on.
pub(in crate::plugins::claude_deck) fn keys(
    ui: &Ui,
    lines: &[ChatLine],
    state: &mut DeckUi,
    to_host: &HostChannel,
    menu_was_open: bool,
    now: u64,
) -> Asked {
    let busy = menu_was_open
        || egui::Popup::is_any_open(ui.ctx())
        || state.renaming.is_some()
        || state.drag.is_some();
    if let Some(back) = switch_pressed(ui).filter(|_| !busy) {
        let tabs = tabs(state.servers.is_on());
        let at = tabs
            .iter()
            .position(|tab| *tab == state.news.tab)
            .unwrap_or(0);
        let next = if back { at + tabs.len() - 1 } else { at + 1 };
        state.news.tab = tabs[next % tabs.len()];
        state.keyboard = true;
        return Asked::default();
    }
    match state.news.tab {
        Tab::Chats => keys::keys(ui, lines, state, to_host, menu_was_open),
        Tab::News if busy => Asked::default(),
        Tab::News => news_keys(ui, state, to_host, now),
        Tab::Servers => crate::plugins::claude_deck::servers::tab::keys::keys(ui, state, busy),
    }
}

/// The tabs in their order: Servers only while a server is listed.
pub fn tabs(servers_on: bool) -> &'static [Tab] {
    if servers_on {
        &[Tab::Chats, Tab::News, Tab::Servers]
    } else {
        &[Tab::Chats, Tab::News]
    }
}

/// Ctrl+Tab (Some(false)) or Ctrl+Shift+Tab (Some(true)).
fn switch_pressed(ui: &Ui) -> Option<bool> {
    ui.ctx().input(|i| {
        i.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key: Key::Tab,
                pressed: true,
                modifiers,
                ..
            } if modifiers.ctrl => Some(modifiers.shift),
            _ => None,
        })
    })
}

fn read(ui: &Ui) -> Pressed {
    ui.ctx().input(|i| Pressed {
        down: i.key_pressed(Key::ArrowDown),
        up: i.key_pressed(Key::ArrowUp),
        home: i.key_pressed(Key::Home),
        end: i.key_pressed(Key::End),
        left: i.key_pressed(Key::ArrowLeft),
        right: i.key_pressed(Key::ArrowRight),
        space: i.key_pressed(Key::Space),
        enter: i.key_pressed(Key::Enter),
        esc: i.key_pressed(Key::Escape),
        mini: i.modifiers.command && i.key_pressed(Key::M),
        quick: i.modifiers.command && i.key_pressed(Key::Comma),
    })
}

/// The key after (or before, `up`) the current one; the first or the
/// last when none is picked.
pub fn step(picks: &[String], current: Option<&str>, up: bool) -> Option<String> {
    if picks.is_empty() {
        return None;
    }
    let at = current.and_then(|key| picks.iter().position(|p| p == key));
    let next = match (up, at) {
        (false, Some(at)) => (at + 1).min(picks.len() - 1),
        (false, None) => 0,
        (true, Some(at)) => at.saturating_sub(1),
        (true, None) => picks.len() - 1,
    };
    picks.get(next).cloned()
}

fn news_keys(ui: &Ui, state: &mut DeckUi, to_host: &HostChannel, now: u64) -> Asked {
    let pressed = read(ui);
    let asked = Asked {
        give_back: pressed.esc,
        mini: pressed.mini,
        quick: pressed.quick,
    };
    let news = &mut state.news;
    let picks = news.picks.clone();
    let next = if pressed.up || pressed.down {
        step(&picks, news.selected.as_deref(), pressed.up)
    } else if pressed.home {
        picks.first().cloned()
    } else if pressed.end {
        picks.last().cloned()
    } else {
        None
    };
    if let Some(next) = next {
        news.selected = Some(next);
        news.scroll_to_selected = true;
        state.keyboard = true;
    }
    let Some(picked) = news.selected.clone().filter(|key| picks.contains(key)) else {
        return asked;
    };
    if let Some(head) = Head::of_key(&picked) {
        let folded = news.is_folded(head);
        if pressed.left && !folded {
            news.fold(head, true);
        } else if pressed.right && folded {
            news.fold(head, false);
        } else if pressed.enter || pressed.space {
            news.fold(head, !folded);
        }
        return asked;
    }
    if let Some((_, chat)) = view::of_block_key(&picked) {
        let chat = chat.to_string();
        let fold = news.chat_fold(&chat);
        if pressed.right {
            news.fold_chat(&chat, fold.right());
        } else if pressed.left {
            news.fold_chat(&chat, fold.left());
        } else if pressed.space {
            news.fold_chat(&chat, fold.arrow());
        } else if pressed.enter {
            let item = news.news.all().find(|item| item.chat == chat).cloned();
            if let Some(item) = item {
                open_block(state, &item, to_host, now);
            }
        }
        return asked;
    }
    let open = news.open.as_deref() == Some(picked.as_str());
    if (pressed.space || pressed.right) && !open {
        news.toggle(&picked, now);
    } else if pressed.left && open {
        news.open = None;
    }
    if pressed.enter {
        let item = news.news.all().find(|item| item.key == picked).cloned();
        if let Some(item) = item {
            open_item(state, &item, to_host, now);
        }
    }
    asked
}
