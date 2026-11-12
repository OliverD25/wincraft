//! The server groups in the drawn list: their lines, and the arrow keys
//! that walk on from the PC chats to the server lines and rows. The server
//! is a fake; nothing reaches ssh.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::Key;

use super::deck::{sample, Deck};
use crate::core::traits::WinCraftPlugin;
use crate::core::ui_bridge::ActionKind;
use crate::plugins::claude_deck::servers::{self, Runner};
use crate::plugins::claude_deck::window::Pick;
use crate::plugins::claude_deck::{ClaudeDeck, Settings};

struct Fake;

impl Runner for Fake {
    fn run(&self, _host: &str, _script: &str) -> Result<String, String> {
        Ok("NOW 100\nCPU 34\nMEM 1000 390\nSAME a b 1 2\nDONE\n".to_string())
    }
}

/// A list of the sample PC chats and one fake server that answered.
fn with_server(name: &str) -> Deck {
    let mut deck = Deck::new(sample(), Settings::default());
    deck.state.servers = servers::Deck::with_runner(Arc::new(Fake));
    deck.state
        .servers
        .tick(&[name.to_string()], Arc::new(|| {}));
    let start = Instant::now();
    while deck.state.servers.views()[0].1.checked.is_none() {
        assert!(start.elapsed() < Duration::from_secs(5), "the fake answers");
        std::thread::sleep(Duration::from_millis(20));
    }
    deck.settle();
    deck
}

#[test]
fn a_server_group_is_drawn_under_the_pc_chats() {
    let mut deck = with_server("rdekeys1");
    let painted = deck.settle();
    assert!(painted.has("keys1"), "{:?}", painted.texts);
    assert!(painted.has("No Claude chats"), "{:?}", painted.texts);
}

#[test]
fn the_arrow_keys_reach_the_server_line_after_the_pc_chats() {
    let mut deck = with_server("rdekeys2");
    deck.key(Key::End);
    assert_eq!(
        deck.state.selected,
        Some(servers::server_pick("rdekeys2")),
        "End goes to the last line, the server's"
    );
    let target = deck
        .state
        .servers
        .target(&servers::server_pick("rdekeys2"))
        .cloned()
        .expect("a target");
    assert_eq!(target.server, "rdekeys2");
    assert_eq!(target.folder, None, "the home folder");
    deck.key(Key::ArrowUp);
    assert!(
        matches!(
            deck.state.selected,
            Some(Pick::Chat(_)) | Some(Pick::Group(_))
        ),
        "Up goes back into the PC list"
    );
    assert_ne!(deck.state.selected, Some(servers::server_pick("rdekeys2")));
}

#[test]
fn the_palette_lists_one_terminal_command_per_server() {
    let mut plugin = ClaudeDeck::new();
    let command = plugin
        .palette_commands()
        .into_iter()
        .find(|command| command.list)
        .expect("the terminal command");
    let empty = plugin
        .command_list(ActionKind::Palette, command.id)
        .expect("a list");
    assert!(empty.rows.is_empty());
    assert!(empty.empty.contains("Homelab"));
    plugin.on_settings_changed(&serde_json::json!({ "servers": "rdehome1, rdehome2" }));
    let list = plugin
        .command_list(ActionKind::Palette, command.id)
        .expect("a list");
    let titles: Vec<&str> = list.rows.iter().map(|row| row.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "ClaudeDeck: Open a terminal on home1",
            "ClaudeDeck: Open a terminal on home2"
        ]
    );
    assert_eq!(list.rows[1].id, "rdehome2");
    // A row that is not a listed server opens nothing (and in a unit test
    // nothing ever starts ssh).
    plugin.on_list_pick(ActionKind::Palette, command.id, "-oProxyCommand=x");
}
