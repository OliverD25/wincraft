//! How the saved arrangement and the chats listed now come together.

use super::*;

const WC: &str = r"E:\code\WinCraft";
const NEWS: &str = r"E:\NewsHQ";

fn names(saved: &Saved) -> Vec<String> {
    saved
        .groups
        .iter()
        .map(|g| format!("{}: {}", g.name, g.chats.join(" ")))
        .collect()
}

#[test]
fn the_first_run_groups_by_project_folder_with_new_on_top() {
    let chats = vec![
        chat("w1", Some(WC), 1),
        chat("n1", Some(NEWS), 2),
        chat("x", None, 3),
        fork("w2", "w1", 4),
        chat("w3", Some(WC), 5),
    ];
    let saved = arrange(&Saved::default(), &chats, NOW);
    assert_eq!(names(&saved), ["New: x", "NewsHQ: n1", "WinCraft: w1 w3"]);
    assert_eq!(saved.groups[0].id, NEW_ID);
    assert_eq!(saved.groups[2].folder.as_deref(), Some(r"e:\code\wincraft"));
    assert_eq!(saved.version, VERSION);
    // Arranging again changes nothing.
    assert_eq!(arrange(&saved, &chats, NOW), saved);
}

#[test]
fn a_new_chat_goes_to_the_end_of_the_group_that_holds_its_project() {
    let mut chats = vec![chat("w1", Some(WC), 1), chat("n1", Some(NEWS), 2)];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    // The user moved the WinCraft chat into the NewsHQ group, before n1.
    crate::plugins::claude_deck::edit::place_chat(
        &mut saved,
        "w1",
        &crate::plugins::claude_deck::edit::Spot::Before("n1".into()),
    );
    chats.push(chat("w2", Some(WC), 3));
    chats.push(fork("w1f", "w1", 4));
    let saved = arrange(&saved, &chats, NOW);
    assert_eq!(names(&saved), ["NewsHQ: w1 n1 w2"]);
}

#[test]
fn a_chat_of_a_project_no_group_holds_goes_to_new_at_the_top() {
    let chats = vec![chat("w1", Some(WC), 1)];
    let saved = arrange(&Saved::default(), &chats, NOW);
    let more = vec![
        chat("w1", Some(WC), 1),
        chat("p1", Some(r"E:\Playwise"), 2),
        chat("q", None, 3),
    ];
    let saved = arrange(&saved, &more, NOW);
    assert_eq!(names(&saved), ["New: p1 q", "WinCraft: w1"]);
}

#[test]
fn a_group_made_for_a_folder_takes_its_chats_back_when_it_is_empty() {
    let chats = vec![chat("w1", Some(WC), 1)];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.groups[0].keep = true;
    let saved = arrange(&saved, &[], NOW);
    assert_eq!(names(&saved), ["WinCraft: w1"]);
    let mut empty = saved.clone();
    empty.groups[0].chats.clear();
    let back = arrange(&empty, &[chat("w2", Some(WC), 2)], NOW);
    assert_eq!(names(&back), ["WinCraft: w2"]);
}

#[test]
fn a_chat_that_is_gone_keeps_its_place_for_30_days() {
    let chats = vec![chat("a", Some(WC), 1), chat("b", Some(WC), 2)];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.pinned.insert("a".into());
    saved.hidden.insert("a".into());
    let only_b = vec![chat("b", Some(WC), 2)];
    let later = arrange(&saved, &only_b, NOW);
    assert_eq!(names(&later), ["WinCraft: a b"]);
    assert_eq!(later.gone.get("a"), Some(&NOW));
    // Back after 29 days: the same place, and no longer counted as gone.
    let day29 = NOW + 29 * DAY_MS;
    let still = arrange(&later, &only_b, day29);
    assert_eq!(still.gone.get("a"), Some(&NOW));
    let back = arrange(&still, &chats, day29);
    assert_eq!(names(&back), ["WinCraft: a b"]);
    assert!(back.gone.is_empty());
    // Gone for 30 days: dropped everywhere.
    let dropped = arrange(&later, &only_b, NOW + 30 * DAY_MS);
    assert_eq!(names(&dropped), ["WinCraft: b"]);
    assert!(dropped.gone.is_empty() && dropped.pinned.is_empty() && dropped.hidden.is_empty());
}

#[test]
fn an_empty_group_goes_unless_the_user_made_or_changed_it() {
    let chats = vec![chat("a", Some(WC), 1), chat("n", Some(NEWS), 2)];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.groups[0].chats.clear();
    let only_a = &chats[..1];
    assert_eq!(names(&arrange(&saved, only_a, NOW)), ["WinCraft: a"]);
    saved.groups[0].keep = true;
    assert_eq!(
        names(&arrange(&saved, only_a, NOW)),
        ["NewsHQ: ", "WinCraft: a"]
    );
}

#[test]
fn pinned_chats_stay_on_top_of_their_group() {
    let chats = vec![
        chat("a", Some(WC), 1),
        chat("b", Some(WC), 2),
        chat("c", Some(WC), 3),
    ];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.pinned.insert("c".into());
    let saved = arrange(&saved, &chats, NOW);
    assert_eq!(names(&saved), ["WinCraft: c a b"]);
    let lines = lines(
        &chats,
        &Archived::new(),
        &View {
            saved,
            ..View::default()
        },
        NOW,
    );
    assert!(chat_line(&lines, "c").pinned);
    assert!(!chat_line(&lines, "a").pinned);
}

#[test]
fn a_hidden_chat_and_its_forks_leave_the_list_for_the_hidden_line() {
    let chats = vec![
        chat("a", Some(WC), 1),
        fork("af", "a", 2),
        chat("b", Some(WC), 3),
    ];
    let hidden = BTreeSet::from(["a".to_string()]);
    let gone: Vec<usize> = {
        let mut set: Vec<usize> = hidden_chats(&chats, &hidden).into_iter().collect();
        set.sort();
        set
    };
    assert_eq!(gone, [0, 1]);
    assert_eq!(hidden_roots(&chats, &hidden), ["a"]);
    let mut view = View {
        saved: Saved {
            hidden,
            ..Saved::default()
        },
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G WinCraft 1", "C b 0", "H 1"]);
    view.hidden_open = true;
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G WinCraft 1", "C b 0", "H 1", "C a 0"]);
    assert!(chat_line(&got, "a").hidden);
    // A hidden chat that is no longer there does not count.
    assert!(hidden_roots(&chats[2..], &BTreeSet::from(["a".to_string()])).is_empty());
}

#[test]
fn groups_and_chats_show_in_the_saved_order_with_colour_and_fold() {
    let chats = vec![
        chat("a", Some(WC), 1),
        chat("b", Some(WC), 2),
        chat("n", Some(NEWS), 3),
    ];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.groups.reverse();
    saved.groups[0].chats.reverse();
    saved.groups[0].colour = Some(Colour::Red);
    let view = View {
        saved,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(
        shape(&got),
        ["G WinCraft 2", "C b 0", "C a 0", "G NewsHQ 1", "C n 0"]
    );
    let Line::Group(group) = &got[0] else {
        panic!("a group");
    };
    assert_eq!(group.colour, Some(Colour::Red));
}

#[test]
fn an_empty_group_the_user_made_shows_unless_only_open_windows_is_on() {
    let chats = vec![chat("a", Some(WC), 1)];
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    crate::plugins::claude_deck::edit::new_group(&mut saved, None, None);
    let mut view = View {
        saved,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G WinCraft 1", "C a 0", "G New group 0"]);
    view.only_open = true;
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert!(shape(&got).is_empty());
}
