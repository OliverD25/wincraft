//! The keys of the list and the mini column, fed as key events to a
//! headless window frame.

use egui::{pos2, Key, Modifiers};

use super::deck::{sample, Deck};
use super::*;
use crate::core::ui_bridge::{ArrangeAction, HostRequest};
use crate::plugins::claude_deck::header::Click;
use crate::plugins::claude_deck::menu;
use crate::plugins::claude_deck::mini::{self, Item};
use crate::plugins::claude_deck::window::Pick;
use crate::plugins::claude_deck::Settings;

fn deck() -> Deck {
    let mut deck = Deck::new(sample(), Settings::default());
    deck.settle();
    deck
}

fn selected(deck: &Deck) -> String {
    match &deck.state.selected {
        Some(Pick::Chat(id)) => format!("chat {id}"),
        Some(Pick::Group(id)) => format!("group {id}"),
        None => "none".into(),
    }
}

fn group_named(deck: &Deck, name: &str) -> String {
    deck.state
        .saved
        .groups
        .iter()
        .find(|group| group.name == name)
        .map(|group| group.id.clone())
        .expect("a group")
}

#[test]
fn the_arrows_walk_the_list_and_home_and_end_jump_to_its_ends() {
    let mut deck = deck();
    assert_eq!(selected(&deck), "none");
    deck.key(Key::ArrowDown);
    let first = selected(&deck);
    assert!(first.starts_with("group"), "the first line: {first}");
    assert!(
        deck.state.keyboard,
        "the digits show while the keys are used"
    );
    deck.key(Key::ArrowDown);
    assert!(selected(&deck).starts_with("chat"), "{}", selected(&deck));
    deck.key(Key::ArrowUp);
    assert_eq!(selected(&deck), first);
    deck.key(Key::End);
    let last = selected(&deck);
    assert_ne!(last, first);
    deck.key(Key::ArrowDown);
    assert_eq!(selected(&deck), last, "the end stays the end");
    deck.key(Key::Home);
    assert_eq!(selected(&deck), first);
    deck.key(Key::ArrowUp);
    assert_eq!(selected(&deck), first, "the top stays the top");
    let painted = deck.settle();
    assert!(painted.has("1"), "the row digits show: {:?}", painted.texts);
}

#[test]
fn a_digit_picks_the_chat_with_that_number() {
    let mut deck = deck();
    deck.key(Key::Num2);
    let second = deck.state.digit_rows[1].clone();
    assert_eq!(selected(&deck), format!("chat {second}"));
    deck.key(Key::Num1);
    assert_eq!(
        selected(&deck),
        format!("chat {}", deck.state.digit_rows[0])
    );
    deck.key(Key::Num9);
    assert_eq!(
        selected(&deck),
        format!("chat {}", deck.state.digit_rows[0]),
        "no ninth chat: nothing changes"
    );
}

#[test]
fn left_and_right_fold_a_group_and_the_forks_of_a_chat() {
    let mut deck = deck();
    let alpha = group_named(&deck, "Alpha");
    deck.state.selected = Some(Pick::Group(alpha.clone()));
    deck.key(Key::ArrowLeft);
    assert!(deck
        .state
        .saved
        .groups
        .iter()
        .any(|g| g.id == alpha && g.folded));
    deck.key(Key::ArrowLeft);
    assert!(
        deck.state
            .saved
            .groups
            .iter()
            .any(|g| g.id == alpha && g.folded),
        "Left on a folded group keeps it"
    );
    deck.key(Key::ArrowRight);
    assert!(deck
        .state
        .saved
        .groups
        .iter()
        .any(|g| g.id == alpha && !g.folded));
    deck.settle();
    deck.state.selected = Some(Pick::Chat("a1".into()));
    deck.key(Key::ArrowLeft);
    assert!(deck.state.saved.folded_chats.contains("a1"));
    deck.key(Key::ArrowRight);
    assert!(!deck.state.saved.folded_chats.contains("a1"));
    deck.state.selected = Some(Pick::Chat("n".into()));
    let before = deck.state.saved.clone();
    deck.key(Key::ArrowLeft);
    assert_eq!(
        deck.state.saved, before,
        "a chat with no forks has nothing to fold"
    );
}

#[test]
fn enter_folds_a_group_brings_a_window_forward_and_f2_renames() {
    let mut deck = deck();
    let beta = group_named(&deck, "Beta");
    deck.state.selected = Some(Pick::Group(beta.clone()));
    deck.key(Key::Enter);
    assert!(deck
        .state
        .saved
        .groups
        .iter()
        .any(|g| g.id == beta && g.folded));
    deck.key(Key::Enter);
    deck.state.selected = Some(Pick::Chat("b1".into()));
    deck.requests();
    deck.key(Key::Enter);
    let sent = deck.requests();
    assert!(sent.iter().any(|r| matches!(
        r,
        HostRequest::Arrange {
            action: ArrangeAction::Activate(1234),
            ..
        }
    )));
    deck.state.selected = Some(Pick::Chat("gone".into()));
    deck.key(Key::Enter);
    assert!(
        deck.state.saved.folded_chats.contains("gone"),
        "Enter on a stand-in folds it"
    );
    deck.state.selected = Some(Pick::Group(beta));
    deck.key(Key::F2);
    let rename = deck.state.renaming.as_ref().expect("renaming");
    assert_eq!(rename.text, "Beta");
}

#[test]
fn alt_arrows_move_the_picked_chat_or_group() {
    let mut deck = deck();
    deck.state.selected = Some(Pick::Chat("a2".into()));
    let before = deck.state.saved.clone();
    let alt = Modifiers::ALT;
    deck.key_with(Key::ArrowUp, alt);
    assert_ne!(deck.state.saved, before, "a2 moved above a1");
    let order: Vec<String> = deck
        .state
        .saved
        .groups
        .iter()
        .flat_map(|g| g.chats.clone())
        .collect();
    let a1 = order.iter().position(|id| id == "a1");
    let a2 = order.iter().position(|id| id == "a2");
    assert!(a2 < a1, "{order:?}");
    let alpha = group_named(&deck, "Alpha");
    deck.state.selected = Some(Pick::Group(alpha.clone()));
    let names = |deck: &Deck| -> Vec<String> {
        deck.state
            .saved
            .groups
            .iter()
            .map(|g| g.name.clone())
            .collect()
    };
    let before = names(&deck);
    deck.key_with(Key::ArrowDown, alt);
    assert_ne!(names(&deck), before);
    deck.key_with(Key::ArrowUp, alt);
    assert_eq!(names(&deck), before);
}

#[test]
fn escape_asks_to_give_the_keyboard_back_and_ctrl_m_asks_for_mini_mode() {
    let mut deck = deck();
    deck.key(Key::Escape);
    deck.key_with(Key::M, Modifiers::COMMAND);
    assert!(deck.asked.iter().any(|a| a.give_back));
    assert!(deck.asked.iter().any(|a| a.mini));
}

#[test]
fn shift_f10_opens_the_menu_of_the_picked_line() {
    let mut deck = deck();
    deck.state.selected = Some(Pick::Chat("a1".into()));
    deck.key_with(Key::F10, Modifiers::SHIFT);
    let painted = deck.settle();
    assert!(painted.has(menu::HIDE), "{:?}", painted.texts);
    assert!(painted.has(menu::FOLD_FORKS), "a1 has a fork");
    deck.key(Key::Escape);
    let after = deck.settle();
    assert!(!after.has(menu::HIDE), "Esc closes the menu");
}

fn mini_deck() -> Deck {
    let settings = Settings {
        mini: true,
        ..Settings::default()
    };
    let mut deck = Deck::new(sample(), settings);
    deck.settle();
    deck
}

fn mark_at(deck: &Deck, id: &str) -> egui::Pos2 {
    let lines = deck.lines();
    let items = mini::items(&lines);
    let tops = mini::tops(&items);
    let at = items
        .iter()
        .position(|item| match item {
            Item::Chat(at) => matches!(&lines[*at], Line::Chat(c) if c.chat.id == id),
            _ => false,
        })
        .expect("a mark");
    pos2(15.0, mini::TOP + tops[at] + 7.0)
}

#[test]
fn the_mini_column_clicks_bring_windows_forward_and_the_button_goes_back_to_the_list() {
    let mut deck = mini_deck();
    deck.requests();
    let on = mark_at(&deck, "b1");
    deck.click(on);
    assert_eq!(deck.state.selected, Some(Pick::Chat("b1".into())));
    let sent = deck.requests();
    assert!(sent.iter().any(|r| matches!(
        r,
        HostRequest::Arrange {
            action: ArrangeAction::Activate(1234),
            ..
        }
    )));
    deck.click(pos2(180.0, mini::TOP / 2.0));
    assert_eq!(deck.clicked, [Click::Mini]);
    assert!(!deck.state.digit_rows.is_empty());
    assert_eq!(deck.state.digit_rows, mini::first_nine(&deck.lines()));
}

#[test]
fn mini_mode_keys_pick_marks_and_a_group_mark_has_a_tip() {
    let mut deck = mini_deck();
    deck.key(Key::Num1);
    assert_eq!(
        selected(&deck),
        format!("chat {}", deck.state.digit_rows[0])
    );
    deck.key(Key::ArrowDown);
    assert!(selected(&deck).starts_with("chat") || selected(&deck).starts_with("group"));
    let painted = deck.settle();
    assert!(
        painted.shapes > 10,
        "a mark for each chat and a line between groups"
    );
    let first = deck.state.digit_rows[0].clone();
    let at = mark_at(&deck, &first);
    deck.rest(at, 12);
    deck.state.selected = None;
    deck.key(Key::End);
    assert!(deck.state.selected.is_some());
}

#[test]
fn mini_mode_paints_a_folded_group_as_one_mark() {
    let mut deck = mini_deck();
    let alpha = group_named(&deck, "Alpha");
    deck.state
        .saved
        .groups
        .iter_mut()
        .for_each(|g| g.folded = g.id == alpha);
    deck.settle();
    let lines = deck.lines();
    let items = mini::items(&lines);
    assert!(items.iter().any(|item| matches!(item, Item::Group(_))));
    let size = mini::size(&items);
    assert_eq!(size.x, mini::WIDTH);
    let tops = mini::tops(&items);
    let group_at = items
        .iter()
        .position(|i| matches!(i, Item::Group(_)))
        .expect("group");
    deck.rest(pos2(15.0, mini::TOP + tops[group_at] + 7.0), 4);
}
