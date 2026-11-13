//! The Servers tab in the drawn window: the third tab with its red count,
//! the cards, and the keys. The server is a fake; nothing reaches ssh.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Key, Modifiers};

use super::deck::{sample, Deck};
use crate::plugins::claude_deck::news::Tab;
use crate::plugins::claude_deck::servers::tab::{view, Pick};
use crate::plugins::claude_deck::servers::{self, Runner};
use crate::plugins::claude_deck::Settings;

struct Fake;

impl Runner for Fake {
    fn run(&self, _host: &str, _script: &str) -> Result<String, String> {
        Ok("NOW 100\nCPU 34\nMEM 1000 390\nUP 90000\n\
            CLP 7 1.0 2048 65 /tmp/lost\tclaude --resume\n\
            BUSY 299 85.1 1668 redis-server\t/usr/bin/redis-server\n\
            SVC system nginx.service failed failed\n\
            DOCK web\trunning\tUp 3 days\n\
            DONE\n"
            .to_string())
    }
}

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

fn ctrl_tab(deck: &mut Deck, shift: bool) {
    deck.key_with(
        Key::Tab,
        Modifiers {
            ctrl: true,
            command: true,
            shift,
            ..Modifiers::NONE
        },
    );
}

#[test]
fn the_servers_tab_shows_its_problems_and_ctrl_tab_goes_round_three_tabs() {
    let mut deck = with_server("rdetab1");
    let painted = deck.settle();
    assert!(
        painted.texts.iter().any(|t| t == view::TAB),
        "{:?}",
        painted.texts
    );
    assert!(
        painted.texts.iter().any(|t| t == "2"),
        "a failed service and an orphan: {:?}",
        painted.texts
    );
    ctrl_tab(&mut deck, false);
    assert_eq!(deck.state.news.tab, Tab::News);
    ctrl_tab(&mut deck, false);
    assert_eq!(deck.state.news.tab, Tab::Servers);
    let painted = deck.settle();
    for words in [
        "tab1",
        "CPU 34%",
        view::CLAUDE,
        view::ORPHAN,
        view::BUSIEST,
        "redis-server",
        view::SERVICES,
        "nginx.service",
        "web",
    ] {
        assert!(painted.has(words), "{words}: {:?}", painted.texts);
    }
    ctrl_tab(&mut deck, false);
    assert_eq!(deck.state.news.tab, Tab::Chats, "round to the first");
    ctrl_tab(&mut deck, true);
    assert_eq!(deck.state.news.tab, Tab::Servers, "Shift goes back");
}

#[test]
fn the_tab_is_gone_while_no_server_is_listed() {
    let mut deck = Deck::new(sample(), Settings::default());
    let painted = deck.settle();
    assert!(!painted.texts.iter().any(|t| t == view::TAB));
    ctrl_tab(&mut deck, false);
    ctrl_tab(&mut deck, false);
    assert_eq!(deck.state.news.tab, Tab::Chats, "two tabs only");
    deck.state.news.tab = Tab::Servers;
    deck.settle();
    assert_eq!(deck.state.news.tab, Tab::Chats, "falls back to Chats");
}

#[test]
fn the_keys_move_over_the_cards_and_fold_them() {
    let mut deck = with_server("rdetab2");
    deck.state.news.tab = Tab::Servers;
    deck.settle();
    deck.key(Key::Home);
    let card = Pick::Card("rdetab2".to_string());
    assert_eq!(deck.state.servers.tab.selected, Some(card.clone()));
    deck.key(Key::ArrowDown);
    assert!(
        matches!(
            deck.state.servers.tab.selected,
            Some(Pick::Proc {
                pid: 7,
                claude: true,
                ..
            })
        ),
        "the section title is skipped: {:?}",
        deck.state.servers.tab.selected
    );
    deck.key(Key::ArrowLeft);
    assert_eq!(
        deck.state.servers.tab.selected,
        Some(card.clone()),
        "Left goes to the header"
    );
    deck.key(Key::ArrowLeft);
    assert!(deck.state.servers.tab.folded.contains("rdetab2"));
    let painted = deck.settle();
    assert!(
        !painted.has("nginx.service"),
        "a folded card hides its rows"
    );
    deck.key(Key::Space);
    assert!(!deck.state.servers.tab.folded.contains("rdetab2"));
    deck.key(Key::End);
    assert!(matches!(
        deck.state.servers.tab.selected,
        Some(Pick::Service { .. })
    ));
    // Enter on a header opens a terminal there; a unit test never starts ssh.
    deck.key(Key::Home);
    deck.key(Key::Enter);
}

#[test]
fn a_click_on_a_header_folds_the_card() {
    let mut deck = with_server("rdetab3");
    deck.state.news.tab = Tab::Servers;
    let painted = deck.settle();
    deck.click(painted.at("tab3"));
    assert!(deck.state.servers.tab.folded.contains("rdetab3"));
    let painted = deck.settle();
    deck.click(painted.at("tab3"));
    assert!(!deck.state.servers.tab.folded.contains("rdetab3"));
}

#[test]
fn the_chats_tab_short_line_opens_the_servers_tab_at_the_card() {
    let mut deck = with_server("rdetab4");
    let painted = deck.settle();
    assert!(painted.has("tab4"), "{:?}", painted.texts);
    assert!(painted.has("No Claude chats"), "{:?}", painted.texts);
    assert!(
        !painted.has("nginx.service"),
        "services are on the Servers tab only"
    );
    deck.state.servers.tab.folded.insert("rdetab4".to_string());
    deck.click(painted.at("tab4"));
    assert_eq!(deck.state.news.tab, Tab::Servers);
    assert_eq!(
        deck.state.servers.tab.selected,
        Some(Pick::Card("rdetab4".to_string()))
    );
    assert!(
        !deck.state.servers.tab.folded.contains("rdetab4"),
        "unfolded"
    );
    deck.state.news.tab = Tab::Chats;
    deck.state.servers.tab.selected = None;
    deck.settle();
    deck.key(Key::End);
    assert_eq!(deck.state.selected, Some(servers::server_pick("rdetab4")));
    deck.key(Key::Enter);
    assert_eq!(deck.state.news.tab, Tab::Servers, "Enter does the same");
    assert_eq!(
        deck.state.servers.tab.selected,
        Some(Pick::Card("rdetab4".to_string()))
    );
}
