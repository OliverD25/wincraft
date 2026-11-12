//! The lines of the list: groups, the fork tree, folds and the cold line.

use super::*;

#[test]
fn groups_go_new_first_then_by_name_and_chats_by_creation() {
    let chats = vec![
        chat("z", Some(r"E:\b\zeta"), 1),
        chat("a2", Some(r"E:\a\Alpha"), 5),
        chat("a1", Some(r"E:\a\alpha"), 9),
        chat("n", None, 3),
        chat("a0", Some(r"E:\a\Alpha"), 2),
    ];
    let got = lines(&chats, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        shape(&got),
        [
            "G New 1",
            "C n 0",
            "G Alpha 3",
            "C a0 0",
            "C a2 0",
            "C a1 0",
            "G zeta 1",
            "C z 0"
        ]
    );
}

#[test]
fn forks_hang_under_their_chat_with_tree_lines() {
    let chats = vec![
        chat("rust", Some(r"E:\code\WinCraft"), 1),
        fork("deck", "rust", 30),
        fork("design", "rust", 20),
        fork("coder", "design", 25),
    ];
    let got = lines(&chats, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        shape(&got),
        [
            "G WinCraft 4",
            "C rust 0",
            "C design 1",
            "C coder 2",
            "C deck 1"
        ]
    );
    let design = chat_line(&got, "design");
    assert_eq!(design.fold, Fold::Open);
    assert!(!design.last);
    assert!(design.rails.is_empty());
    let coder = chat_line(&got, "coder");
    assert!(coder.last);
    assert_eq!(coder.rails, vec![true], "design's line goes on to deck");
    let deck = chat_line(&got, "deck");
    assert!(deck.last);
    assert_eq!(deck.fold, Fold::None);
    assert_eq!(chat_line(&got, "rust").fold, Fold::Open);
}

#[test]
fn a_folded_chat_shows_its_forks_most_urgent_status() {
    let mut chats = vec![
        Chat {
            mark: Mark::Working,
            ..chat("rust", Some(r"E:\code\WinCraft"), 1)
        },
        fork("design", "rust", 20),
        fork("coder", "design", 25),
    ];
    chats[2].mark = Mark::AsksYou;
    chats[2].since = Some(NOW - 7 * MIN);
    let view = View {
        saved: folded_chat("rust"),
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G WinCraft 3", "C rust 0"]);
    let rust = chat_line(&got, "rust");
    assert_eq!(rust.fold, Fold::Folded(2));
    assert_eq!(rust.chat.mark, Mark::AsksYou);
    assert_eq!(rust.chat.since, Some(NOW - 7 * MIN));
}

#[test]
fn a_folded_group_shows_its_count_and_most_urgent_status() {
    let mut chats = vec![
        chat("a", Some(r"E:\NewsHQ"), 1),
        chat("b", Some(r"E:\NewsHQ"), 2),
    ];
    chats[1].mark = Mark::Background;
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.groups[0].folded = true;
    let view = View {
        saved,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G NewsHQ 2"]);
    let Line::Group(group) = &got[0] else {
        panic!("a group");
    };
    assert!(group.folded);
    assert_eq!(group.roll, Mark::Background);
}

#[test]
fn a_fork_of_an_archived_or_missing_chat_keeps_a_stand_in() {
    let chats = vec![
        fork("deck", "gone", 30),
        fork("other", "gone", 10),
        fork("lost", "never", 40),
    ];
    let archived = Archived::from([("gone".to_string(), "WinCraft Rust".to_string())]);
    let got = lines(&chats, &archived, &View::default(), NOW);
    assert_eq!(
        shape(&got),
        [
            "G WinCraft 3",
            "S WinCraft Rust \u{b7} archived",
            "C other 1",
            "C deck 1",
            "S Chat not found",
            "C lost 1",
        ]
    );
    assert!(chat_line(&got, "deck").last);
    assert!(!chat_line(&got, "other").last);

    let view = View {
        saved: folded_chat("gone"),
        ..View::default()
    };
    let mut urgent = chats.clone();
    urgent[0].mark = Mark::Working;
    let got = lines(&urgent, &archived, &view, NOW);
    let stand_in = chat_line(&got, "gone");
    assert!(stand_in.stand_in);
    assert_eq!(stand_in.fold, Fold::Folded(2));
    assert_eq!(stand_in.chat.mark, Mark::Working);
}

#[test]
fn a_chat_that_names_itself_or_a_loop_still_shows_once() {
    let mut own = chat("self", Some(r"E:\x"), 1);
    own.parent = Some("self".into());
    let mut a = chat("a", Some(r"E:\x"), 2);
    a.parent = Some("b".into());
    let mut b = chat("b", Some(r"E:\x"), 3);
    b.parent = Some("a".into());
    let got = lines(&[own, a, b], &Archived::new(), &View::default(), NOW);
    let ids: Vec<String> = shape(&got);
    assert_eq!(ids.iter().filter(|line| line.starts_with("C ")).count(), 3);
    assert_eq!(ids[0], "G x 3");
}

#[test]
fn only_open_windows_keeps_the_chats_above_an_open_one() {
    let mut chats = vec![
        chat("rust", Some(r"E:\code\WinCraft"), 1),
        fork("design", "rust", 20),
        fork("deck", "rust", 30),
        chat("other", Some(r"E:\code\Other"), 2),
    ];
    chats[1].window = Some(5);
    let view = View {
        only_open: true,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G WinCraft 2", "C rust 0", "C design 1"]);
    assert!(chat_line(&got, "design").last);
    assert_eq!(counts(&chats, true), vec![]);
    chats[1].mark = Mark::Working;
    chats[3].mark = Mark::Working;
    assert_eq!(counts(&chats, true), vec![(Mark::Working, 1)]);
    assert_eq!(counts(&chats, false), vec![(Mark::Working, 2)]);
}

#[test]
fn chats_idle_over_a_day_fold_into_one_line_at_the_end() {
    let mut chats = vec![
        chat("live", Some(r"E:\a"), 1),
        cold("old1", Some(r"E:\a"), 1),
        cold("old9", Some(r"E:\b"), 9),
        cold("parent", Some(r"E:\a"), 4),
        fork("kid", "parent", 50),
    ];
    chats[4].folder = Some(r"E:\a".into());
    let view = View {
        fold_cold: true,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(
        shape(&got),
        ["G a 3", "C live 0", "C parent 0", "C kid 1", "~ 2"],
        "a cold chat with a fresh fork stays"
    );
    let Some(Line::Cold(line)) = got.last() else {
        panic!("the cold line");
    };
    assert_eq!(line.days, (1, 9));
    assert!(!line.open);

    let mut open = view.clone();
    open.saved.cold_open = true;
    let got = lines(&chats, &Archived::new(), &open, NOW);
    assert_eq!(&shape(&got)[4..], ["~ 2", "C old1 0", "C old9 0"]);

    let off = View::default();
    let got = lines(&chats, &Archived::new(), &off, NOW);
    assert!(!shape(&got).iter().any(|line| line.starts_with('~')));
}

#[test]
fn the_header_counts_only_the_marks_it_shows_in_its_order() {
    let mut chats: Vec<Chat> = (0..5).map(|n| chat(&n.to_string(), None, n)).collect();
    chats[0].mark = Mark::DoneNotSeen;
    chats[1].mark = Mark::Working;
    chats[2].mark = Mark::NeedsApproval;
    chats[3].mark = Mark::Working;
    chats[4].mark = Mark::Failed;
    assert_eq!(
        counts(&chats, false),
        vec![
            (Mark::NeedsApproval, 1),
            (Mark::Working, 2),
            (Mark::DoneNotSeen, 1)
        ]
    );
}

#[test]
fn a_folded_chat_says_when_a_fork_is_the_most_urgent() {
    let mut chats = vec![
        chat("rust", Some(r"E:\code\WinCraft"), 1),
        fork("design", "rust", 20),
    ];
    chats[0].text = Some("Last: Done.".into());
    chats[1].mark = Mark::NeedsApproval;
    chats[1].text = Some("Waits for your OK to run a command".into());
    let view = View {
        saved: folded_chat("rust"),
        ..View::default()
    };
    let lines = lines(&chats, &Archived::new(), &view, NOW);
    let rust = chat_line(&lines, "rust");
    assert_eq!(rust.chat.mark, Mark::NeedsApproval);
    assert_eq!(
        rust.chat.text.as_deref(),
        Some("A fork needs approval: Waits for your OK to run a command")
    );
    // Open, each keeps its own line.
    let lines =
        crate::plugins::claude_deck::model::lines(&chats, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        chat_line(&lines, "rust").chat.text.as_deref(),
        Some("Last: Done.")
    );
}
