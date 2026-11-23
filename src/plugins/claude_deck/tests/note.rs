//! A note on a chat (design section 17, option A): saved with the list,
//! shown as a third line, added with N or the menu, removed with the menu
//! or the × on hover.

use std::collections::BTreeMap;

use egui::{Event, Key};

use super::deck::{sample, Deck};
use super::*;
use crate::plugins::claude_deck::menu;
use crate::plugins::claude_deck::note::{self, KEYS_HINT, LIMIT};
use crate::plugins::claude_deck::rows;
use crate::plugins::claude_deck::store::{load, save, Loaded};
use crate::plugins::claude_deck::window::Pick;
use crate::plugins::claude_deck::Settings;

fn noted(id: &str, words: &str) -> Deck {
    let mut deck = Deck::new(sample(), Settings::default());
    deck.state.saved.notes.insert(id.into(), words.into());
    deck.build();
    deck
}

fn pick(deck: &mut Deck, id: &str) {
    deck.state.selected = Some(Pick::Chat(id.into()));
}

#[test]
fn notes_are_saved_with_the_list_and_an_older_file_has_none() {
    let folder =
        std::env::temp_dir().join(format!("wincraft-claude-deck-notes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    let path = folder.join("claude_deck.list.json");
    let saved = Saved {
        notes: BTreeMap::from([("a".to_string(), "Test the tray".to_string())]),
        ..Saved::default()
    };
    save(&path, &saved);
    let Loaded::Read(read) = load(&path) else {
        panic!("read back");
    };
    assert_eq!(read.notes, saved.notes);
    let _ = std::fs::remove_dir_all(&folder);
    let old: Saved = serde_json::from_str(r#"{"version": 1, "groups": []}"#).expect("read");
    assert!(old.notes.is_empty());
}

#[test]
fn the_note_goes_on_its_own_chat_and_makes_that_row_taller() {
    let chats = vec![
        chat("a", Some(r"E:\a\Alpha"), 1),
        chat("b", Some(r"E:\a\Alpha"), 2),
        fork("f", "a", 3),
    ];
    let mut saved = Saved::default();
    saved
        .notes
        .insert("b".into(), "Ask about the colours".into());
    saved.notes.insert("f".into(), "Throwaway".into());
    let view = View {
        saved,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(chat_line(&got, "a").note, None);
    assert_eq!(
        chat_line(&got, "b").note.as_deref(),
        Some("Ask about the colours")
    );
    assert_eq!(chat_line(&got, "f").note.as_deref(), Some("Throwaway"));
    let height = |id: &str, two| {
        let line = got
            .iter()
            .find(|line| matches!(line, Line::Chat(c) if c.chat.id == id))
            .expect("listed");
        rows::height(line, false, two)
    };
    assert_eq!(height("a", true), rows::ROW_TWO);
    assert_eq!(height("b", true), rows::ROW_TWO + rows::NOTE);
    assert_eq!(height("b", false), rows::ROW + rows::NOTE);
}

#[test]
fn a_note_is_one_line_of_at_most_120_characters_and_an_empty_one_changes_nothing() {
    let mut saved = Saved::default();
    note::set(&mut saved, "a", "  first\nline  ");
    assert_eq!(saved.notes["a"], "first line");
    note::set(&mut saved, "a", "   ");
    assert_eq!(saved.notes["a"], "first line", "empty keeps the old note");
    note::set(&mut saved, "a", &"x".repeat(200));
    assert_eq!(saved.notes["a"].chars().count(), LIMIT);
    note::remove(&mut saved, "a");
    assert!(saved.notes.is_empty());
    assert_eq!(note::count("abc"), "3 / 120");
}

#[test]
fn a_note_stays_while_its_chat_is_gone_and_goes_after_30_days() {
    let chats = vec![chat("a", Some(r"E:\a\Alpha"), 1), chat("b", None, 2)];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.notes.insert("a".into(), "Keep me".into());
    let only_b = vec![chat("b", None, 2)];
    let later = arrange(&saved, &only_b, NOW);
    let day29 = arrange(&later, &only_b, NOW + 29 * DAY_MS);
    assert_eq!(day29.notes.get("a").map(String::as_str), Some("Keep me"));
    let dropped = arrange(&later, &only_b, NOW + 30 * DAY_MS);
    assert!(dropped.notes.is_empty());
    assert!(dropped.gone.is_empty());
}

#[test]
fn n_opens_the_box_and_typing_and_enter_save_the_note() {
    let mut deck = Deck::new(sample(), Settings::default());
    deck.settle();
    pick(&mut deck, "a2");
    deck.key(Key::N);
    assert!(deck.state.noting.is_some(), "N opens the box");
    let open = deck.settle();
    assert!(
        open.has(note::HINT) && open.has(KEYS_HINT),
        "{:?}",
        open.texts
    );
    deck.frame(vec![Event::Text("Test the tray".into())]);
    let typing = deck.settle();
    assert!(typing.has("13 / 120"), "{:?}", typing.texts);
    deck.key(Key::Enter);
    assert!(deck.state.noting.is_none());
    assert_eq!(deck.state.saved.notes["a2"], "Test the tray");
    assert!(deck.settle().has("Test the tray"));
}

#[test]
fn esc_keeps_the_old_note_and_enter_on_an_empty_box_changes_nothing() {
    let mut deck = noted("a2", "Old words");
    deck.settle();
    pick(&mut deck, "a2");
    deck.key(Key::N);
    deck.settle();
    deck.frame(vec![Event::Text("New words".into())]);
    deck.key(Key::Escape);
    assert!(deck.state.noting.is_none());
    assert_eq!(deck.state.saved.notes["a2"], "Old words");
    assert!(
        deck.asked.iter().all(|asked| !asked.give_back),
        "Esc is the box's"
    );

    let mut empty = Deck::new(sample(), Settings::default());
    empty.settle();
    pick(&mut empty, "a1");
    empty.key(Key::N);
    empty.settle();
    empty.key(Key::Enter);
    assert!(empty.state.noting.is_none());
    assert!(empty.state.saved.notes.is_empty());
}

#[test]
fn typing_stops_at_120_characters() {
    let mut deck = Deck::new(sample(), Settings::default());
    deck.settle();
    pick(&mut deck, "a1");
    deck.key(Key::N);
    deck.settle();
    deck.frame(vec![Event::Text("y".repeat(130))]);
    deck.key(Key::Enter);
    assert_eq!(deck.state.saved.notes["a1"].chars().count(), LIMIT);
}

#[test]
fn the_menu_adds_edits_and_removes_a_note() {
    let mut deck = Deck::new(sample(), Settings::default());
    let painted = deck.settle();
    deck.right_click(painted.at("Chat a2"));
    let up = deck.settle();
    assert!(up.has(menu::ADD_NOTE) && !up.has(menu::REMOVE_NOTE));
    deck.click(up.at(menu::ADD_NOTE));
    assert!(deck.state.noting.is_some());
    deck.settle();
    deck.frame(vec![Event::Text("From the menu".into())]);
    deck.key(Key::Enter);
    assert_eq!(deck.state.saved.notes["a2"], "From the menu");
    let painted = deck.settle();
    deck.right_click(painted.at("Chat a2"));
    let up = deck.settle();
    assert!(up.has(menu::EDIT_NOTE) && up.has(menu::REMOVE_NOTE));
    deck.click(up.at(menu::REMOVE_NOTE));
    assert!(deck.state.saved.notes.is_empty());
}

#[test]
fn the_cross_on_hover_removes_the_note_and_a_double_click_edits_it() {
    let mut deck = noted("a2", "Remove me");
    let painted = deck.settle();
    let words = painted
        .rects
        .iter()
        .find(|(text, _)| text == "Remove me")
        .map(|(_, rect)| *rect)
        .expect("the note is painted");
    assert!(words.top() > painted.at("Chat a2").y, "under the row");
    // A click on the note itself only picks the row.
    deck.click(words.center());
    assert!(
        deck.requests().is_empty(),
        "the chat is not brought forward"
    );
    assert_eq!(deck.state.saved.notes["a2"], "Remove me");
    // A double-click on it opens the box with the note.
    let press = |pressed| Event::PointerButton {
        pos: words.center(),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    deck.frame(vec![press(true), press(false), press(true), press(false)]);
    deck.frame(Vec::new());
    let editing = deck.state.noting.as_ref().map(|n| n.text.clone());
    assert_eq!(editing.as_deref(), Some("Remove me"));
    deck.key(Key::Escape);
    deck.settle();
    // The × sits just after the text: the gap and half of its 12 px.
    let cross = egui::pos2(words.right() + 12.0, words.center().y);
    deck.click(cross);
    assert!(deck.state.saved.notes.is_empty(), "the × removed it");
}

#[test]
fn the_hover_panel_shows_the_whole_note() {
    let long = "Ask about the Stats tab colours, then check the light theme screenshots again before the merge";
    let mut deck = noted("b1", long);
    let painted = deck.settle();
    let before = painted.texts.iter().filter(|t| t.as_str() == long).count();
    let shown = deck.rest(painted.at("Chat b1"), 12);
    let after = shown.texts.iter().filter(|t| t.as_str() == long).count();
    assert_eq!((before, after), (1, 2), "{:?}", shown.texts);
}
