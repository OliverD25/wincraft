//! Mini mode's column and the fade (stage 4).

use std::time::{Duration, Instant};

use super::*;
use crate::plugins::claude_deck::fade::{self, Fade};
use crate::plugins::claude_deck::mini::{self, Item};

#[test]
fn the_column_has_one_mark_per_chat_and_a_line_between_groups() {
    let chats = vec![
        chat("n", None, 1),
        chat("a1", Some(r"E:\a\Alpha"), 2),
        chat("a2", Some(r"E:\a\Alpha"), 3),
        fork("f", "a1", 4),
        cold("old", Some(r"E:\a\Alpha"), 5),
    ];
    let mut chats = chats;
    chats[3].folder = Some(r"E:\a\Alpha".into());
    let view = View {
        fold_cold: true,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    let items = mini::items(&got);
    let names: Vec<String> = items
        .iter()
        .map(|item| match item {
            Item::Chat(at) => match &got[*at] {
                Line::Chat(c) => c.chat.id.clone(),
                _ => "?".into(),
            },
            Item::Group(_) => "G".into(),
            Item::Gap => "|".into(),
        })
        .collect();
    assert_eq!(
        names,
        ["n", "|", "a1", "f", "a2"],
        "a fork keeps its place; the cold chats stay out"
    );
    assert_eq!(mini::first_nine(&got), ["n", "a1", "f", "a2"]);

    let tops = mini::tops(&items);
    assert_eq!(tops[0], 6.0);
    assert_eq!(
        tops[1],
        6.0 + 14.0 + 6.0,
        "the line sits one gap below a mark"
    );
    assert_eq!(tops[2], tops[1] + 1.0 + 6.0);
    let size = mini::size(&items);
    assert_eq!(size.x, 30.0, "as wide as the design's column");
    assert_eq!(size.y, mini::TOP + 6.0 * 2.0 + 14.0 * 4.0 + 1.0 + 6.0 * 4.0);
}

#[test]
fn a_folded_group_shows_one_mark_in_the_column() {
    let chats = vec![
        chat("a1", Some(r"E:\a\Alpha"), 1),
        chat("b1", Some(r"E:\b\Beta"), 2),
    ];
    let first = lines(&chats, &Archived::new(), &View::default(), NOW);
    let mut saved = arrange(&Saved::default(), &chats, NOW);
    saved.groups[0].folded = true;
    let view = View {
        saved,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    let items = mini::items(&got);
    assert!(matches!(items[0], Item::Group(_)));
    assert_eq!(items[1], Item::Gap);
    assert!(matches!(items[2], Item::Chat(_)));
    assert_eq!(mini::items(&first).len(), 3);
    assert!(mini::items(&[]).is_empty());
}

fn seen(mouse_in: bool) -> fade::Seen {
    fade::Seen {
        on: true,
        mouse_in,
        focused: false,
        new_wait: false,
    }
}

#[test]
fn the_window_fades_after_the_mouse_is_away_and_then_stops_painting() {
    let start = Instant::now();
    let mut fade = Fade::default();
    assert_eq!(
        fade.step(seen(true), start),
        None,
        "solid with the mouse in"
    );
    let wait = fade.step(seen(false), start).expect("waits for the delay");
    assert_eq!(wait, fade::AWAY);
    let later = start + fade::AWAY;
    assert!(fade.step(seen(false), later).is_some(), "the step starts");
    let mut at = later;
    for _ in 0..30 {
        at += Duration::from_millis(16);
        if fade.step(seen(false), at).is_none() {
            break;
        }
    }
    assert_eq!(fade.alpha, fade::FADED, "faded to the design's opacity");
    assert_eq!(
        fade.step(seen(false), at + Duration::from_secs(60)),
        None,
        "faded and still: no frames"
    );
    assert!(fade
        .step(seen(true), at + Duration::from_secs(61))
        .is_some());
    let mut at = at + Duration::from_secs(61);
    while fade.step(seen(true), at).is_some() {
        at += Duration::from_millis(16);
    }
    assert_eq!(fade.alpha, 1.0, "solid again when the mouse is back");
}

#[test]
fn focus_a_new_wait_or_the_setting_off_keep_it_solid() {
    let start = Instant::now();
    let long = start + Duration::from_secs(30);
    let mut fade = Fade::default();
    fade.step(seen(false), start);
    let focused = fade::Seen {
        focused: true,
        ..seen(false)
    };
    assert_eq!(fade.step(focused, long), None);
    assert_eq!(fade.alpha, 1.0, "the keyboard keeps it solid");

    let mut fade = Fade::default();
    let off = fade::Seen {
        on: false,
        ..seen(false)
    };
    fade.step(off, start);
    assert_eq!(fade.step(off, long), None);
    assert_eq!(fade.alpha, 1.0, "the setting is off");

    let mut fade = Fade::default();
    fade.step(seen(false), start);
    let mut at = long;
    while fade.step(seen(false), at).is_some() {
        at += Duration::from_millis(16);
    }
    assert_eq!(fade.alpha, fade::FADED);
    let wait = fade::Seen {
        new_wait: true,
        ..seen(false)
    };
    fade.step(wait, at);
    while fade.step(seen(false), at).is_some() {
        at += Duration::from_millis(16);
    }
    assert_eq!(fade.alpha, 1.0, "a chat started to wait: solid until seen");
    fade.step(seen(true), at);
    fade.step(seen(false), at);
    let mut at = at + fade::AWAY;
    while fade.step(seen(false), at).is_some() {
        at += Duration::from_millis(16);
    }
    assert_eq!(fade.alpha, fade::FADED, "seen once, it fades again later");
}
