//! Where a dragged chat or group lands, Alt+arrows, the menus' items and
//! the keyboard's pick order.

use super::*;
use crate::plugins::claude_deck::drop::*;
use crate::plugins::claude_deck::edit::{GroupSpot, Spot};
use crate::plugins::claude_deck::menu::{chat_items, group_items, Item};

fn group(id: &str, folded: bool, top: f32) -> Placed {
    Placed {
        kind: Kind::Group {
            id: id.into(),
            folded,
        },
        top,
        bottom: top + 24.0,
    }
}

fn row(key: Option<&str>, top: f32) -> Placed {
    Placed {
        kind: Kind::Chat {
            key: key.map(str::to_string),
        },
        top,
        bottom: top + 22.0,
    }
}

/// Group A (0-24) with a (24-46) and its fork (46-68), b (68-90); group B
/// (90-114) with c (114-136); folded group C (136-160); the cold line
/// (160-184).
fn screen() -> Vec<Placed> {
    vec![
        group("A", false, 0.0),
        row(Some("a"), 24.0),
        row(None, 46.0),
        row(Some("b"), 68.0),
        group("B", false, 90.0),
        row(Some("c"), 114.0),
        group("C", true, 136.0),
        Placed {
            kind: Kind::Other,
            top: 160.0,
            bottom: 184.0,
        },
        row(Some("cold"), 184.0),
    ]
}

#[test]
fn a_chat_lands_above_or_below_the_family_under_the_mouse() {
    let lines = screen();
    // The upper half of a's family (24-68) is above a.
    assert_eq!(
        chat_spot(&lines, 30.0),
        Some((Spot::Before("a".into()), 24.0))
    );
    // The lower half, the fork included, is below it: above b.
    assert_eq!(
        chat_spot(&lines, 60.0),
        Some((Spot::Before("b".into()), 68.0))
    );
    // Below the last chat of a group is its end.
    assert_eq!(chat_spot(&lines, 85.0), Some((Spot::End("A".into()), 90.0)));
    // On an open group's header: its top. On a folded one: its end.
    assert_eq!(
        chat_spot(&lines, 100.0),
        Some((Spot::Before("c".into()), 114.0))
    );
    assert_eq!(
        chat_spot(&lines, 150.0),
        Some((Spot::End("C".into()), 160.0))
    );
    // Over the cold line and its rows: nowhere.
    assert_eq!(chat_spot(&lines, 170.0), None);
    assert_eq!(chat_spot(&lines, 190.0), None);
    // Under everything, with no cold line: the end of the last group.
    let short = &lines[..6];
    assert_eq!(
        chat_spot(short, 500.0),
        Some((Spot::End("B".into()), 136.0))
    );
}

#[test]
fn a_group_lands_above_or_below_the_group_under_the_mouse() {
    let lines = screen();
    assert_eq!(
        group_spot(&lines, 10.0),
        Some((GroupSpot::Before("A".into()), 0.0))
    );
    assert_eq!(
        group_spot(&lines, 80.0),
        Some((GroupSpot::Before("B".into()), 90.0))
    );
    assert_eq!(
        group_spot(&lines, 125.0),
        Some((GroupSpot::Before("C".into()), 136.0))
    );
    assert_eq!(group_spot(&lines, 155.0), Some((GroupSpot::End, 160.0)));
    assert_eq!(group_spot(&lines, 400.0), Some((GroupSpot::End, 160.0)));
}

#[test]
fn alt_arrows_move_a_chat_past_its_neighbour_and_across_groups() {
    let lines = screen();
    assert_eq!(step_chat(&lines, "a", false), Some(Spot::End("A".into())));
    assert_eq!(step_chat(&lines, "b", true), Some(Spot::Before("a".into())));
    assert_eq!(step_chat(&lines, "a", true), None);
    assert_eq!(
        step_chat(&lines, "b", false),
        Some(Spot::Before("c".into()))
    );
    assert_eq!(step_chat(&lines, "c", true), Some(Spot::End("A".into())));
    assert_eq!(step_chat(&lines, "c", false), Some(Spot::End("C".into())));
    assert_eq!(step_chat(&lines, "cold", true), None);
}

fn line(depth: usize, fold: Fold) -> ChatLine {
    ChatLine {
        chat: chat("a", Some(r"E:\code\WinCraft"), 1),
        stand_in: false,
        depth,
        rails: Vec::new(),
        last: false,
        fold,
        pinned: false,
        hidden: false,
    }
}

#[test]
fn the_chat_menu_offers_what_fits_the_row() {
    use Item::*;
    assert_eq!(
        chat_items(&line(0, Fold::Open), true),
        [Open, Pin, Hide, Copy, RenameChat, Archive, Separator, FoldForks, MoveTo, NewWith]
    );
    // A fork stays under its chat: no pin, no move.
    assert_eq!(
        chat_items(&line(1, Fold::None), true),
        [Open, Hide, Copy, RenameChat, Archive]
    );
    let pinned = ChatLine {
        pinned: true,
        ..line(0, Fold::Folded(2))
    };
    assert_eq!(
        chat_items(&pinned, true),
        [
            Open,
            Unpin,
            Hide,
            Copy,
            RenameChat,
            Archive,
            Separator,
            UnfoldForks,
            MoveTo,
            NewWith
        ]
    );
    let hidden = ChatLine {
        hidden: true,
        ..line(0, Fold::None)
    };
    assert_eq!(chat_items(&hidden, false), [Open, Show, Copy, RenameChat]);
    let stand_in = ChatLine {
        stand_in: true,
        ..line(0, Fold::Open)
    };
    assert_eq!(
        chat_items(&stand_in, true),
        [Pin, Hide, Separator, FoldForks, MoveTo, NewWith]
    );
    // Under the cold line: no pin and no move.
    assert_eq!(
        chat_items(&line(0, Fold::None), false),
        [Open, Hide, Copy, RenameChat, Archive]
    );
}

#[test]
fn the_group_menu_matches_the_design_with_fold_all_added() {
    use Item::*;
    assert_eq!(
        group_items(false),
        [
            Rename, FoldGroup, FoldAll, UnfoldAll, Colour, Separator, NewGroup, MoveUp, MoveDown,
            Separator, Ungroup
        ]
    );
    assert_eq!(group_items(true)[1], UnfoldGroup);
}

#[test]
fn up_and_down_walk_groups_and_chats() {
    use crate::plugins::claude_deck::keys::{picks, step};
    use crate::plugins::claude_deck::window::Pick;
    let chats = vec![chat("a", Some(r"E:\code\WinCraft"), 1), chat("b", None, 2)];
    let got = lines(&chats, &Archived::new(), &View::default(), NOW);
    let all = picks(&got);
    let g = |id: &str| Pick::Group(id.into());
    let c = |id: &str| Pick::Chat(id.into());
    assert_eq!(all[1], c("b"));
    assert_eq!(all.len(), 4);
    assert_eq!(step(&all, None, false), Some(g("new")));
    assert_eq!(step(&all, None, true), Some(c("a")));
    assert_eq!(step(&all, Some(&c("b")), false), Some(all[2].clone()));
    assert_eq!(step(&all, Some(&c("a")), false), Some(c("a")));
    assert_eq!(step(&all, Some(&g("new")), true), Some(g("new")));
    assert!(step(&[], None, false).is_none());
}

#[test]
fn only_a_chat_that_can_be_forked_gets_the_fork_item() {
    use crate::plugins::claude_deck::fork::Fork;
    use Item::*;
    let id = "0f3a9c1e-1b2d-4c5e-8f60-123456789abc";
    let mut forkable = line(0, Fold::None);
    forkable.chat.fork = Fork::new(id, Some(r"E:\code\WinCraft"));
    assert_eq!(
        chat_items(&forkable, true),
        [Open, Pin, Hide, Copy, RenameChat, Fork, Archive, Separator, MoveTo, NewWith]
    );
    assert!(!chat_items(&line(0, Fold::None), true).contains(&Fork));
    let stand_in = ChatLine {
        stand_in: true,
        ..forkable.clone()
    };
    assert!(!chat_items(&stand_in, true).contains(&Fork));
    let hidden = ChatLine {
        hidden: true,
        ..forkable
    };
    assert_eq!(
        chat_items(&hidden, false),
        [Open, Show, Copy, RenameChat, Fork]
    );
}
