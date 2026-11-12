//! The keyboard (stage 4): the row the hotkey selects, the digits, and the
//! tray count's words.

use super::*;
use crate::plugins::claude_deck::select::*;
use crate::plugins::claude_deck::window::Pick;

fn waiting(id: &str, mark: Mark, since_min: u64) -> Chat {
    Chat {
        mark,
        since: Some(NOW - since_min * MIN),
        ..chat(id, Some(r"E:\code\WinCraft"), since_min)
    }
}

#[test]
fn the_hotkey_selects_the_chat_that_waits_longest() {
    let chats = vec![
        chat("idle", Some(r"E:\code\WinCraft"), 1),
        waiting("asks", Mark::AsksYou, 9),
        waiting("approve", Mark::NeedsApproval, 4),
    ];
    let got = lines(&chats, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        open_pick(&got, None),
        Some(Pick::Chat("asks".into())),
        "the one waiting 9 minutes beats the one waiting 4"
    );

    let same = vec![
        waiting("asks", Mark::AsksYou, 5),
        waiting("approve", Mark::NeedsApproval, 5),
    ];
    let got = lines(&same, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        open_pick(&got, None),
        Some(Pick::Chat("approve".into())),
        "at the same time, approval comes first"
    );
}

#[test]
fn with_nothing_waiting_the_row_used_last_stays_selected() {
    let chats = vec![
        chat("a", Some(r"E:\code\WinCraft"), 1),
        chat("b", Some(r"E:\code\WinCraft"), 2),
    ];
    let got = lines(&chats, &Archived::new(), &View::default(), NOW);
    let last = Pick::Chat("b".into());
    assert_eq!(open_pick(&got, Some(&last)), Some(last));
    let gone = Pick::Chat("gone".into());
    assert_eq!(
        open_pick(&got, Some(&gone)),
        Some(Pick::Chat("a".into())),
        "a row no longer shown gives way to the first chat"
    );
    assert_eq!(open_pick(&[], None), None);
}

#[test]
fn the_digits_go_to_the_first_nine_chat_rows_on_screen() {
    let mut seen = 0;
    let rows: Vec<Option<u8>> = (0..12)
        .map(|at| {
            let on_screen = at >= 1;
            let stand_in = at == 3;
            digit_for(on_screen, stand_in, &mut seen)
        })
        .collect();
    assert_eq!(rows[0], None, "off screen");
    assert_eq!(rows[1], Some(1));
    assert_eq!(rows[2], Some(2));
    assert_eq!(rows[3], None, "a stand-in has no digit");
    assert_eq!(rows[4], Some(3));
    assert_eq!(rows[10], Some(9));
    assert_eq!(rows[11], None, "only nine digits");
    assert_eq!(DIGITS.len(), 9);
}

#[test]
fn the_tray_count_names_wincraft_and_counts_only_waiting_chats() {
    let mut open = waiting("approve", Mark::NeedsApproval, 3);
    open.window = Some(7);
    let chats = vec![
        open,
        waiting("asks", Mark::AsksYou, 2),
        waiting("works", Mark::Working, 1),
        waiting("done", Mark::DoneNotSeen, 1),
    ];
    assert_eq!(waiting_ids(&chats), ["approve", "asks"]);
    assert_eq!(super::super::select::waiting(&chats, false), 2);
    assert_eq!(super::super::select::waiting(&chats, true), 1);
    assert_eq!(
        tray_tip(true, 2).as_deref(),
        Some("WinCraft \u{2014} 2 chats wait for you")
    );
    assert_eq!(
        tray_tip(true, 1).as_deref(),
        Some("WinCraft \u{2014} 1 chat waits for you")
    );
    assert_eq!(tray_tip(true, 0), None, "no count, the plain tooltip");
    assert_eq!(tray_tip(false, 3), None, "the setting is off");
}

#[test]
fn a_chat_that_starts_waiting_is_new_once() {
    let before = vec!["a".to_string()];
    assert!(!new_wait(&before, &before));
    assert!(
        !new_wait(&before, &[]),
        "a chat that stopped waiting is not new"
    );
    assert!(new_wait(&before, &["a".to_string(), "b".to_string()]));
}

#[test]
fn a_waiting_chat_out_of_sight_is_picked_by_the_line_that_holds_it() {
    let mut chats = vec![
        chat("rust", Some(r"E:\code\WinCraft"), 1),
        fork("design", "rust", 20),
        waiting("seen", Mark::AsksYou, 2),
    ];
    chats[1].mark = Mark::NeedsApproval;
    chats[1].since = Some(NOW - 9 * MIN);
    let view = View {
        saved: folded_chat("rust"),
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(
        open_pick(&got, None),
        Some(Pick::Chat("rust".into())),
        "the folded chat holds the fork that waits longest"
    );

    let mut gone_waits = vec![fork("deck", "gone", 30), waiting("seen", Mark::AsksYou, 2)];
    gone_waits[0].mark = Mark::AsksYou;
    gone_waits[0].since = Some(NOW - 9 * MIN);
    let view = View {
        saved: folded_chat("gone"),
        ..View::default()
    };
    let got = lines(&gone_waits, &Archived::new(), &view, NOW);
    assert_eq!(open_pick(&got, None), Some(Pick::Chat("gone".into())));

    let mut chats = vec![
        waiting("far", Mark::NeedsApproval, 9),
        waiting("near", Mark::AsksYou, 2),
    ];
    chats[0].folder = Some(r"E:\NewsHQ".into());
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    for group in &mut saved.groups {
        group.folded = group.name == "NewsHQ";
    }
    let view = View {
        saved,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    let Some(Pick::Group(key)) = open_pick(&got, None) else {
        panic!("the folded group that holds the longest wait");
    };
    assert!(got
        .iter()
        .any(|line| matches!(line, Line::Group(g) if g.key == key && g.name == "NewsHQ")));
}
