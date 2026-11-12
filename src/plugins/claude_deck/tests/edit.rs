//! The user's changes to the arrangement. None of them may lose a chat.

use super::*;
use crate::plugins::claude_deck::edit::*;

const WC: &str = r"E:\code\WinCraft";
const NEWS: &str = r"E:\NewsHQ";

fn names(saved: &Saved) -> Vec<String> {
    saved
        .groups
        .iter()
        .map(|g| format!("{}: {}", g.name, g.chats.join(" ")))
        .collect()
}

fn start() -> (Vec<Chat>, Saved) {
    let chats = vec![
        chat("a", Some(WC), 1),
        chat("b", Some(WC), 2),
        chat("c", Some(WC), 3),
        chat("n", Some(NEWS), 4),
    ];
    let saved = arrange(&Saved::default(), &chats, NOW);
    (chats, saved)
}

fn group_id(saved: &Saved, name: &str) -> String {
    saved
        .groups
        .iter()
        .find(|g| g.name == name)
        .map(|g| g.id.clone())
        .expect("the group")
}

#[test]
fn a_chat_moves_before_another_or_to_the_end_of_a_group() {
    let (_, mut saved) = start();
    place_chat(&mut saved, "c", &Spot::Before("a".into()));
    assert_eq!(names(&saved), ["NewsHQ: n", "WinCraft: c a b"]);
    let news = group_id(&saved, "NewsHQ");
    place_chat(&mut saved, "a", &Spot::End(news));
    assert_eq!(names(&saved), ["NewsHQ: n a", "WinCraft: c b"]);
    // Onto itself, or to a place that does not exist: nothing changes.
    let before = saved.clone();
    place_chat(&mut saved, "a", &Spot::Before("a".into()));
    place_chat(&mut saved, "a", &Spot::Before("nope".into()));
    place_chat(&mut saved, "a", &Spot::End("nope".into()));
    place_chat(&mut saved, "nope", &Spot::Before("b".into()));
    assert_eq!(saved, before);
}

#[test]
fn pinning_puts_a_chat_on_top_and_moving_keeps_pinned_ones_on_top() {
    let (_, mut saved) = start();
    pin(&mut saved, "c", true);
    assert_eq!(names(&saved)[1], "WinCraft: c a b");
    pin(&mut saved, "b", true);
    assert_eq!(names(&saved)[1], "WinCraft: b c a");
    // Dropped above a pinned chat, a chat is pinned too.
    place_chat(&mut saved, "a", &Spot::Before("c".into()));
    assert!(saved.pinned.contains("a"));
    // Dropped below them, it is not.
    let wc = group_id(&saved, "WinCraft");
    place_chat(&mut saved, "b", &Spot::End(wc));
    assert!(!saved.pinned.contains("b"));
    assert_eq!(names(&saved)[1], "WinCraft: a c b");
    pin(&mut saved, "a", false);
    assert_eq!(names(&saved)[1], "WinCraft: c a b");
}

#[test]
fn hide_fold_rename_and_colour_change_only_the_list() {
    let (_, mut saved) = start();
    hide(&mut saved, "a", true);
    assert!(saved.hidden.contains("a"));
    hide(&mut saved, "a", false);
    assert!(saved.hidden.is_empty());
    fold_chat(&mut saved, "a");
    assert!(saved.folded_chats.contains("a"));
    fold_chat(&mut saved, "a");
    assert!(saved.folded_chats.is_empty());
    let wc = group_id(&saved, "WinCraft");
    fold_group(&mut saved, &wc);
    assert!(saved.groups[1].folded);
    fold_all(&mut saved, false);
    assert!(saved.groups.iter().all(|g| !g.folded));
    fold_all(&mut saved, true);
    assert!(saved.groups.iter().all(|g| g.folded));
    rename(&mut saved, &wc, "   ");
    assert_eq!(saved.groups[1].name, "WinCraft");
    assert!(!saved.groups[1].keep);
    rename(&mut saved, &wc, "  Rust work ");
    assert_eq!(saved.groups[1].name, "Rust work");
    assert!(saved.groups[1].keep);
    colour(&mut saved, &wc, Some(Colour::Blue));
    assert_eq!(saved.groups[1].colour, Some(Colour::Blue));
}

#[test]
fn a_new_group_sits_below_its_group_and_can_take_a_chat() {
    let (_, mut saved) = start();
    let news = group_id(&saved, "NewsHQ");
    let id = new_group(&mut saved, Some(&news), Some("b"));
    assert_eq!(
        names(&saved),
        ["NewsHQ: n", "New group: b", "WinCraft: a c"]
    );
    assert!(saved.groups[1].keep);
    assert_ne!(id, news);
    let last = new_group(&mut saved, None, None);
    assert_eq!(saved.groups.last().map(|g| g.id.clone()), Some(last));
}

#[test]
fn groups_move_by_drag_and_one_step_at_a_time() {
    let (_, mut saved) = start();
    let news = group_id(&saved, "NewsHQ");
    let wc = group_id(&saved, "WinCraft");
    place_group(&mut saved, &news, &GroupSpot::End);
    assert_eq!(names(&saved), ["WinCraft: a b c", "NewsHQ: n"]);
    place_group(&mut saved, &news, &GroupSpot::Before(wc.clone()));
    assert_eq!(names(&saved), ["NewsHQ: n", "WinCraft: a b c"]);
    step_group(&mut saved, &wc, true);
    assert_eq!(names(&saved), ["WinCraft: a b c", "NewsHQ: n"]);
    step_group(&mut saved, &wc, true);
    assert_eq!(names(&saved), ["WinCraft: a b c", "NewsHQ: n"]);
    step_group(&mut saved, &news, false);
    assert_eq!(names(&saved), ["WinCraft: a b c", "NewsHQ: n"]);
}

#[test]
fn ungroup_sends_each_chat_back_to_its_folder_and_loses_none() {
    let (chats, mut saved) = start();
    let families = families_of(&chats);
    let news = group_id(&saved, "NewsHQ");
    let mixed = new_group(&mut saved, Some(&news), Some("a"));
    place_chat(&mut saved, "n", &Spot::End(mixed.clone()));
    assert_eq!(
        names(&saved),
        ["NewsHQ: ", "New group: a n", "WinCraft: b c"]
    );
    ungroup(&mut saved, &mixed, &families);
    let saved = arrange(&saved, &chats, NOW);
    assert_eq!(names(&saved), ["NewsHQ: n", "WinCraft: b c a"]);

    // A group made for a folder is made again for its chats.
    let mut saved = saved;
    let wc = group_id(&saved, "WinCraft");
    rename(&mut saved, &wc, "Rust");
    ungroup(&mut saved, &wc, &families);
    assert_eq!(names(&saved), ["NewsHQ: n", "WinCraft: b c a"]);

    // Chats with no folder go to "New"; in "New" itself they stay.
    let odd = vec![chat("x", None, 9), chat("n", Some(NEWS), 4)];
    let mut saved = arrange(&Saved::default(), &odd, NOW);
    let families = families_of(&odd);
    let other = new_group(&mut saved, None, Some("x"));
    assert_eq!(names(&saved), ["New: ", "NewsHQ: n", "New group: x"]);
    ungroup(&mut saved, &other, &families);
    assert_eq!(names(&saved), ["New: x", "NewsHQ: n"]);
    ungroup(&mut saved, NEW_ID, &families);
    assert_eq!(names(&saved), ["New: x", "NewsHQ: n"]);
}
