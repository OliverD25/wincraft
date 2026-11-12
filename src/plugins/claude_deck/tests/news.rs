//! The tabs and the News tab in a whole window frame: the strip and its
//! count, a click and the keys on replies, chats' blocks and groups,
//! opening a chat, the dot on chat rows and the count in mini mode.

use egui::{Key, Modifiers};

use super::deck::{sample, Deck};
use super::*;
use crate::core::ui_bridge::{ArrangeAction, HostRequest};
use crate::plugins::claude_deck::header::Click;
use crate::plugins::claude_deck::news::collect::{Item, News};
use crate::plugins::claude_deck::news::view::Folds;
use crate::plugins::claude_deck::news::{load, view, Tab};
use crate::plugins::claude_deck::{mini, Settings};

fn item(key: &str, ago: u64, text: &str, unseen: bool) -> Item {
    Item {
        key: key.to_string(),
        chat: "b1".to_string(),
        title: format!("Title {key}"),
        project: Some("beta".into()),
        server: None,
        folder: None,
        window: Some(1234),
        at: NOW - ago,
        text: text.to_string(),
        unseen,
        mark: None,
        status: Mark::Idle(Age::Fresh),
    }
}

fn long_reply() -> String {
    (1..=24)
        .map(|n| format!("Line {n} of the reply."))
        .collect::<Vec<_>>()
        .join("\n")
}

fn news() -> News {
    let mut asks = item("wait:a2", MIN, "Which one?", true);
    asks.mark = Some(Mark::AsksYou);
    asks.chat = "a2".into();
    News {
        pinned: vec![asks],
        recent: vec![
            item("r1", 4 * MIN, &long_reply(), true),
            item("r2", 2 * 60 * MIN, "The lesson screen works.", false),
        ],
        older: vec![item("o1", 2 * DAY_MS, "Published the gallery page.", true)],
    }
}

fn news_deck(settings: Settings, name: &str) -> Deck {
    let mut deck = Deck::new(sample(), settings);
    let dir = std::env::temp_dir().join(format!("wincraft-news-ui-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("claude_deck.news.json");
    let _ = std::fs::remove_file(&file);
    deck.state.news.file = Some(file);
    deck.state.news.take(news());
    deck.settle();
    deck
}

fn ctrl_tab(deck: &mut Deck) {
    deck.key_with(
        Key::Tab,
        Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        },
    );
}

fn activated(deck: &Deck, hwnd: isize) -> bool {
    deck.requests().iter().any(|r| {
        matches!(
            r,
            HostRequest::Arrange {
                action: ArrangeAction::Activate(h),
                ..
            } if *h == hwnd
        )
    })
}

#[test]
fn the_strip_shows_both_tabs_with_the_unseen_count_and_ctrl_tab_switches() {
    let mut deck = news_deck(Settings::default(), "strip");
    let painted = deck.settle();
    assert!(painted.texts.iter().any(|t| t == view::CHATS));
    assert!(painted.texts.iter().any(|t| t == view::NEWS));
    assert!(
        painted.texts.iter().any(|t| t == "2"),
        "two unseen replies; the pinned chat is not counted: {:?}",
        painted.texts
    );
    assert_eq!(deck.state.news.tab, Tab::Chats);
    ctrl_tab(&mut deck);
    assert_eq!(deck.state.news.tab, Tab::News);
    let painted = deck.settle();
    assert!(painted.has(view::WAITS) && painted.has(view::RECENT) && painted.has(view::OLDER));
    assert!(
        !painted.has("Published the gallery page"),
        "Older starts folded"
    );
    ctrl_tab(&mut deck);
    assert_eq!(deck.state.news.tab, Tab::Chats);
    let painted = deck.settle();
    deck.click(painted.at(view::NEWS));
    assert_eq!(deck.state.news.tab, Tab::News, "a click on the tab");
}

#[test]
fn a_click_opens_an_item_and_reads_it_and_open_the_chat_brings_its_window() {
    let mut deck = news_deck(Settings::default(), "click");
    deck.state.news.tab = Tab::News;
    let painted = deck.settle();
    assert!(painted.has(&view::earlier(1)), "{:?}", painted.texts);
    assert!(
        !painted.has("The lesson screen works."),
        "a seen reply folds under an unseen one"
    );
    deck.click(painted.at("Line 1 of the reply."));
    assert_eq!(deck.state.news.open.as_deref(), Some("r1"));
    assert_eq!(deck.state.news.count, 1, "opening it read it");
    let painted = deck.settle();
    assert!(
        painted.has("Show all \u{b7} 24 lines"),
        "{:?}",
        painted.texts
    );
    assert!(painted.has(view::OPEN_CHAT));
    deck.requests();
    deck.click(painted.at(view::OPEN_CHAT));
    assert!(activated(&deck, 1234), "the chat's window comes forward");
    let painted = deck.settle();
    deck.click(painted.at("Show all"));
    let painted = deck.settle();
    assert!(!painted.has("Show all"));
    assert!(painted.has("Line 24 of the reply."));
    let saved = load(deck.state.news.file.as_ref().expect("a file"), NOW);
    assert!(saved.has("r1"), "the mark is saved");
    deck.click(painted.at("Line 1 of the reply."));
    assert_eq!(deck.state.news.open, None, "a second click closes it");
    let painted = deck.settle();
    deck.click(painted.at(&view::earlier(1)));
    let painted = deck.settle();
    assert!(painted.has("The lesson screen works."), "the chat unfolds");
    assert!(!painted.has(&view::earlier(1)));
}

#[test]
fn a_click_on_a_block_line_opens_the_chat_and_reads_its_replies() {
    let mut deck = news_deck(Settings::default(), "block");
    deck.state.news.tab = Tab::News;
    let painted = deck.settle();
    deck.requests();
    deck.click(painted.at("Title r1"));
    assert!(activated(&deck, 1234), "the chat's window comes forward");
    assert_eq!(deck.state.news.open, None, "no reply opens in place");
    assert_eq!(
        deck.state.news.count, 0,
        "every reply of the chat counts as seen, the older one too"
    );
}

#[test]
fn older_unfolds_with_a_click_and_the_extras_mark_and_filter() {
    let mut deck = news_deck(Settings::default(), "extras");
    deck.state.news.tab = Tab::News;
    let painted = deck.settle();
    deck.click(painted.at(view::OLDER));
    let painted = deck.settle();
    assert!(
        painted.has("Published the gallery page"),
        "{:?}",
        painted.texts
    );
    deck.click(painted.at(view::ONLY_UNSEEN));
    assert!(deck.state.news.only_unseen);
    let painted = deck.settle();
    assert!(
        !painted.has("The lesson screen works."),
        "a seen reply hides"
    );
    assert!(painted.has("Which one?"), "a pinned chat stays");
    deck.click(painted.at(view::MARK_ALL));
    assert_eq!(deck.state.news.count, 0);
    let painted = deck.settle();
    assert!(painted.has(view::RECENT), "the line of the day stays");
}

#[test]
fn the_keys_move_open_fold_and_enter_opens_the_chat() {
    let mut deck = news_deck(Settings::default(), "keys");
    ctrl_tab(&mut deck);
    deck.settle();
    deck.key(Key::Home);
    assert_eq!(
        deck.state.news.selected.as_deref(),
        Some("head:Waits for you")
    );
    for _ in 0..4 {
        deck.key(Key::ArrowDown);
    }
    assert_eq!(
        deck.state.news.selected.as_deref(),
        Some("chat:Last 24 hours:b1")
    );
    deck.key(Key::ArrowRight);
    assert!(deck.state.news.chat_open("b1"), "Right unfolds the chat");
    deck.key(Key::ArrowLeft);
    assert!(!deck.state.news.chat_open("b1"), "Left folds it");
    deck.key(Key::ArrowDown);
    assert_eq!(deck.state.news.selected.as_deref(), Some("r1"));
    deck.key(Key::Space);
    assert_eq!(deck.state.news.open.as_deref(), Some("r1"));
    deck.key(Key::ArrowLeft);
    assert_eq!(deck.state.news.open, None);
    deck.key(Key::ArrowRight);
    assert_eq!(deck.state.news.open.as_deref(), Some("r1"));
    deck.key(Key::End);
    assert_eq!(deck.state.news.selected.as_deref(), Some("head:Older"));
    deck.key(Key::ArrowRight);
    assert!(!deck.state.news.is_folded(view::Head::Older));
    deck.key(Key::ArrowLeft);
    assert!(deck.state.news.is_folded(view::Head::Older));
    deck.key(Key::Num1);
    assert_eq!(
        deck.state.news.selected.as_deref(),
        Some("head:Older"),
        "the digits stay on the Chats tab"
    );
    deck.key(Key::ArrowUp);
    deck.requests();
    deck.key(Key::Enter);
    assert!(activated(&deck, 1234), "Enter opens the chat");
    deck.key(Key::Escape);
    assert!(deck.asked.iter().any(|asked| asked.give_back));
}

#[test]
fn a_chat_row_with_an_unseen_reply_gets_a_dot() {
    let mut plain = Deck::new(sample(), Settings::default());
    let without = plain.settle().shapes;
    let mut chats = sample();
    chats
        .iter_mut()
        .for_each(|chat| chat.news = chat.id == "b1");
    let mut marked = Deck::new(chats, Settings::default());
    let with = marked.settle().shapes;
    assert_eq!(with, without + 1, "one amber dot");
}

#[test]
fn mini_mode_shows_the_count_and_a_click_opens_the_news_tab() {
    let settings = Settings {
        mini: true,
        ..Settings::default()
    };
    let mut deck = news_deck(settings, "mini");
    let painted = deck.settle();
    assert!(
        painted.texts.iter().any(|t| t == "2"),
        "{:?}",
        painted.texts
    );
    deck.click(painted.at("2"));
    assert_eq!(deck.state.news.tab, Tab::News);
    assert_eq!(deck.clicked, [Click::Mini], "the full window comes back");
    assert_eq!(mini::counted(0), 0.0, "no count, no room");
}
