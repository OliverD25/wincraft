//! The list window drawn and clicked on a headless screen: what shows,
//! what a click or a right-click changes, drags, the hover panel and the
//! empty states.

use egui::{pos2, Event, Key, Modifiers, PointerButton};

use super::deck::{sample, Deck};
use super::*;
use crate::core::ui_bridge::HostRequest;
use crate::plugins::claude_deck::header::Click;
use crate::plugins::claude_deck::menu;
use crate::plugins::claude_deck::window::Pick;
use crate::plugins::claude_deck::Settings;

fn deck() -> Deck {
    Deck::new(sample(), Settings::default())
}

#[test]
fn the_list_shows_groups_chats_forks_the_cold_line_and_the_header() {
    let mut deck = deck();
    let painted = deck.settle();
    for words in [
        "Alpha",
        "Beta",
        "New",
        "Chat n",
        "Chat a1",
        "Chat a2",
        "Chat f",
        "Chat b1",
        "Which one?",
        "Reading the files",
        "Only open windows",
    ] {
        assert!(painted.has(words), "{words}: {:?}", painted.texts);
    }
    assert!(painted.has(&cold_label(1)), "the cold chat is folded away");
    assert!(!painted.has("Chat old"));
    assert!(painted.has("Chat not found"), "the stand-in");
    assert!(
        deck.state.digit_rows.len() >= 6,
        "{:?}",
        deck.state.digit_rows
    );
    assert!(deck.state.digit_rows.contains(&"n".to_string()));
    assert!(
        !deck.state.digit_rows.contains(&"gone".to_string()),
        "a stand-in has no digit"
    );
}

#[test]
fn a_click_on_the_cold_line_opens_it_and_the_hidden_line_lists_hidden_chats() {
    let mut deck = deck();
    let painted = deck.settle();
    deck.click(painted.at(&cold_label(1)));
    assert!(deck.state.saved.cold_open);
    let painted = deck.settle();
    assert!(painted.has("Chat old"));
    deck.state.saved.hidden.insert("h".to_string());
    let painted = deck.settle();
    assert!(!painted.has("Chat h"));
    assert!(painted.has(&hidden_label(1)), "{:?}", painted.texts);
    deck.click(painted.at(&hidden_label(1)));
    assert!(deck.state.hidden_open);
    assert!(deck.settle().has("Chat h"));
    let painted = deck.settle();
    deck.click(painted.at(&hidden_label(1)));
    assert!(!deck.state.hidden_open);
}

#[test]
fn a_click_on_a_group_folds_it_and_on_a_chat_with_a_window_brings_it_forward() {
    let mut deck = deck();
    let painted = deck.settle();
    let before = deck.state.saved.clone();
    deck.click(painted.at("Beta"));
    assert_ne!(deck.state.saved, before);
    let beta = deck.state.saved.groups.iter().find(|g| g.name == "Beta");
    assert!(beta.is_some_and(|g| g.folded));
    assert!(matches!(&deck.state.selected, Some(Pick::Group(_))));
    let painted = deck.settle();
    assert!(painted.has("chats"), "a folded group says how many chats");
    assert!(!painted.has("Chat b1"));
    deck.click(painted.at("Beta"));
    let painted = deck.settle();
    assert!(painted.has("Chat b1"));
    deck.requests();
    deck.click(painted.at("Chat b1"));
    assert_eq!(deck.state.selected, Some(Pick::Chat("b1".into())));
    let sent = deck.requests();
    assert!(
        sent.iter().any(|r| matches!(
            r,
            HostRequest::Arrange {
                action: crate::core::ui_bridge::ArrangeAction::Activate(1234),
                ..
            }
        )),
        "{}",
        sent.len()
    );
    deck.click(painted.at("Chat a1"));
    assert!(
        deck.requests().is_empty(),
        "a chat with no window sends nothing"
    );
}

#[test]
fn a_click_on_the_fold_arrow_folds_the_forks_and_a_stand_in_folds_by_any_click() {
    let mut deck = deck();
    let painted = deck.settle();
    let mut row = painted.at("Chat a1");
    row.x = 12.0 + 5.0;
    deck.click(row);
    assert!(deck.state.saved.folded_chats.contains("a1"));
    let painted = deck.settle();
    assert!(!painted.has("Chat f"));
    assert!(painted.has(&forks_label(1)), "{:?}", painted.texts);
    deck.click(painted.at("Chat not found"));
    let stand_in = deck.state.saved.folded_chats.contains("gone");
    assert!(stand_in, "{:?}", deck.state.saved.folded_chats);
}

#[test]
fn the_chat_menu_lists_its_items_and_hiding_a_chat_moves_it_to_the_hidden_line() {
    let mut deck = deck();
    let painted = deck.settle();
    deck.right_click(painted.at("Chat a2"));
    let menu_up = deck.settle();
    for item in [
        menu::OPEN,
        menu::PIN,
        menu::HIDE,
        menu::COPY,
        menu::MOVE_TO,
        menu::NEW_WITH,
    ] {
        assert!(menu_up.has(item), "{item}: {:?}", menu_up.texts);
    }
    deck.click(menu_up.at(menu::PIN));
    assert!(deck.state.saved.pinned.contains("a2"));
    deck.settle();
    let painted = deck.settle();
    deck.right_click(painted.at("Chat a2"));
    let again = deck.settle();
    assert!(again.has(menu::UNPIN));
    deck.click(again.at(menu::HIDE));
    assert!(deck.state.saved.hidden.contains("a2"));
    let painted = deck.settle();
    assert!(!painted.has("Chat a2"));
    assert!(painted.has(&hidden_label(1)));
}

#[test]
fn a_menu_pick_can_move_a_chat_to_another_group_or_start_a_new_one() {
    let mut deck = deck();
    let painted = deck.settle();
    deck.right_click(painted.at("Chat n"));
    let up = deck.settle();
    deck.click(up.at(menu::NEW_WITH));
    let renaming = deck.state.renaming.as_ref().map(|r| r.text.clone());
    assert_eq!(
        renaming.as_deref(),
        Some(crate::plugins::claude_deck::edit::NEW_GROUP_NAME)
    );
    deck.frame(vec![Event::Text("Fresh".into())]);
    deck.key(Key::Enter);
    deck.settle();
    assert!(deck.state.renaming.is_none());
    assert!(
        deck.state.saved.groups.iter().any(|g| g.name == "Fresh"),
        "{:?}",
        deck.state
            .saved
            .groups
            .iter()
            .map(|g| &g.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_group_menu_renames_colours_folds_and_moves_groups() {
    let mut deck = deck();
    let painted = deck.settle();
    deck.right_click(painted.at("Alpha"));
    let up = deck.settle();
    for item in [
        menu::RENAME,
        menu::FOLD_GROUP,
        menu::FOLD_ALL,
        menu::UNFOLD_ALL,
        menu::COLOUR,
        menu::NEW_GROUP,
        menu::MOVE_UP,
        menu::MOVE_DOWN,
        menu::UNGROUP,
    ] {
        assert!(up.has(item), "{item}: {:?}", up.texts);
    }
    deck.click(up.at(menu::FOLD_ALL));
    assert!(deck.state.saved.groups.iter().all(|g| g.folded));
    let painted = deck.settle();
    deck.right_click(painted.at("Alpha"));
    let up = deck.settle();
    deck.click(up.at(menu::UNFOLD_ALL));
    assert!(deck.state.saved.groups.iter().all(|g| !g.folded));
    let order: Vec<String> = deck
        .state
        .saved
        .groups
        .iter()
        .map(|g| g.name.clone())
        .collect();
    let painted = deck.settle();
    deck.right_click(painted.at("Beta"));
    let up = deck.settle();
    deck.click(up.at(menu::MOVE_UP));
    let moved: Vec<String> = deck
        .state
        .saved
        .groups
        .iter()
        .map(|g| g.name.clone())
        .collect();
    assert_ne!(moved, order, "Beta moved up");
    let painted = deck.settle();
    deck.right_click(painted.at("Alpha"));
    let up = deck.settle();
    deck.click(up.at(menu::RENAME));
    assert!(deck.state.renaming.is_some());
    deck.settle();
    deck.frame(vec![Event::Text("Omega".into())]);
    deck.key(Key::Enter);
    deck.settle();
    assert!(deck.state.saved.groups.iter().any(|g| g.name == "Omega"));
    let painted = deck.settle();
    assert!(painted.has("Omega") && !painted.has("Alpha"));
}

#[test]
fn escape_while_renaming_keeps_the_old_name() {
    let mut deck = deck();
    let painted = deck.settle();
    let at = painted.at("Alpha");
    deck.click(at);
    deck.key(Key::F2);
    assert!(deck.state.renaming.is_some());
    deck.settle();
    deck.frame(vec![Event::Text("Zed".into())]);
    deck.key(Key::Escape);
    assert!(deck.state.renaming.is_none());
    assert!(deck.state.saved.groups.iter().all(|g| g.name != "Zed"));
}

#[test]
fn dragging_a_chat_to_another_place_moves_it_and_escape_stops_the_drag() {
    let mut deck = deck();
    let painted = deck.settle();
    let from = painted.at("Chat a1");
    let to = pos2(from.x + 20.0, painted.at("Chat b1").y + 4.0);
    let before = deck.state.saved.clone();
    let press = |pressed, at| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    deck.frame(vec![Event::PointerMoved(from)]);
    deck.frame(vec![press(true, from)]);
    deck.frame(vec![Event::PointerMoved(pos2(from.x, from.y + 12.0))]);
    let lifted = deck.frame(vec![Event::PointerMoved(to)]);
    assert!(matches!(deck.state.drag, Some(Pick::Chat(_))));
    assert!(lifted.has("Chat a1"), "the lifted row is painted");
    deck.frame(vec![Event::PointerMoved(to)]);
    deck.frame(vec![press(false, to)]);
    deck.settle();
    assert!(deck.state.drag.is_none());
    assert_ne!(deck.state.saved, before, "the chat has a new place");

    let painted = deck.settle();
    let from = painted.at("Chat a2");
    deck.frame(vec![Event::PointerMoved(from)]);
    deck.frame(vec![press(true, from)]);
    deck.frame(vec![Event::PointerMoved(pos2(from.x, from.y + 30.0))]);
    deck.frame(vec![Event::PointerMoved(pos2(from.x, from.y + 40.0))]);
    assert!(deck.state.drag.is_some());
    let kept = deck.state.saved.clone();
    deck.key(Key::Escape);
    deck.frame(vec![press(false, from)]);
    assert!(deck.state.drag.is_none());
    assert_eq!(deck.state.saved, kept, "Esc drops nothing");
}

#[test]
fn dragging_a_fork_moves_its_whole_family() {
    let mut deck = deck();
    let painted = deck.settle();
    let from = painted.at("Chat f");
    let to = pos2(from.x + 20.0, painted.at("Chat b1").y + 4.0);
    let before = deck.state.saved.clone();
    let press = |pressed, at| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    deck.frame(vec![Event::PointerMoved(from)]);
    deck.frame(vec![press(true, from)]);
    deck.frame(vec![Event::PointerMoved(pos2(from.x, from.y + 12.0))]);
    deck.frame(vec![Event::PointerMoved(to)]);
    let Some(Pick::Chat(lead)) = deck.state.drag.clone() else {
        panic!("a drag from a fork lifts its family");
    };
    assert!(
        before.groups.iter().any(|g| g.chats.contains(&lead)),
        "the lifted chat is the one at the top of the family"
    );
    deck.frame(vec![press(false, to)]);
    deck.settle();
    assert!(deck.state.drag.is_none());
    assert_ne!(deck.state.saved, before, "the family has a new place");
}

#[test]
fn dragging_a_group_header_moves_the_group() {
    let mut deck = deck();
    let painted = deck.settle();
    let from = painted.at("New");
    let to = pos2(from.x, painted.at("Chat b1").y + 6.0);
    let before: Vec<String> = deck
        .state
        .saved
        .groups
        .iter()
        .map(|g| g.name.clone())
        .collect();
    let press = |pressed, at| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    deck.frame(vec![Event::PointerMoved(from)]);
    deck.frame(vec![press(true, from)]);
    deck.frame(vec![Event::PointerMoved(pos2(from.x, from.y + 14.0))]);
    let lifted = deck.frame(vec![Event::PointerMoved(to)]);
    assert!(matches!(deck.state.drag, Some(Pick::Group(_))));
    assert!(lifted.has("New"));
    deck.frame(vec![press(false, to)]);
    deck.settle();
    let after: Vec<String> = deck
        .state
        .saved
        .groups
        .iter()
        .map(|g| g.name.clone())
        .collect();
    assert_eq!(before.len(), after.len());
    assert_ne!(before, after, "the New group moved down");
}

#[test]
fn resting_the_mouse_on_a_row_shows_the_hover_panel_with_the_full_text() {
    let mut deck = deck();
    let painted = deck.settle();
    let on = painted.at("Chat b1");
    let shown = deck.rest(on, 12);
    assert!(
        shown.has("Reading the files, then the tests."),
        "{:?}",
        shown.texts
    );
    assert!(shown.has("Beta"), "the project in the panel");
    let on = painted.at("Chat a1");
    let shown = deck.rest(on, 12);
    assert!(shown.has(NO_WINDOW), "{:?}", shown.texts);
    let stand_in = deck.rest(painted.at("Chat not found"), 12);
    assert!(!stand_in.has(NO_WINDOW), "no panel for a stand-in");
}

#[test]
fn the_header_buttons_report_to_the_window() {
    let mut deck = deck();
    deck.settle();
    let middle = crate::plugins::claude_deck::header::HEIGHT / 2.0;
    let close = pos2(360.0 - 15.0, middle);
    deck.click(close);
    deck.click(pos2(close.x - 25.0, middle));
    assert_eq!(deck.clicked, [Click::Close, Click::Mini]);
}

#[test]
fn the_empty_states_say_why_nothing_is_listed() {
    let mut deck = Deck::new(Vec::new(), Settings::default());
    let none = deck.settle();
    assert!(none.has("No Claude chats yet."), "{:?}", none.texts);
    let mut filtered = Deck::new(
        vec![chat("a", Some(r"E:\a\Alpha"), 1)],
        Settings {
            only_open: true,
            ..Settings::default()
        },
    );
    let words = filtered.settle();
    assert!(words.has("No chat has a window open."), "{:?}", words.texts);
}

#[test]
fn every_setting_combination_paints_without_a_problem() {
    for mark in [
        crate::plugins::claude_deck::MarkStyle::Shapes,
        crate::plugins::claude_deck::MarkStyle::ShapesAndWords,
        crate::plugins::claude_deck::MarkStyle::Off,
    ] {
        for flag in [false, true] {
            let settings = Settings {
                mark,
                time: flag,
                project: flag,
                jobs: flag,
                model: flag,
                text: flag,
                fold_cold: flag,
                ..Settings::default()
            };
            let mut deck = Deck::new(sample(), settings);
            let painted = deck.settle();
            assert!(painted.has("Chat b1"), "{mark:?} {flag}");
            assert_eq!(painted.has("Reading the files"), flag);
            let open = pos2(30.0, 100.0);
            deck.rest(open, 3);
        }
    }
}
