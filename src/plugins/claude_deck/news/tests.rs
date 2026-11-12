//! The News tab without drawing: which replies show where, what is
//! unseen, the read marks and their file, the lines and the words.

use std::collections::HashSet;

use crate::core::claude_sessions::Age;
use crate::core::claude_turns::Reply;

use super::super::model::{Chat, Mark, DAY_MS};
use super::collect::{collect, key_of, project_of, Item, News, Source, PER_CHAT};
use super::view::{self, Head, Line};
use super::*;

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const NOW: u64 = 100 * DAY_MS;

fn chat(id: &str) -> Chat {
    Chat {
        id: id.to_string(),
        title: format!("Chat {id}"),
        folder: Some(format!(r"E:\code\{id}-project")),
        branch: Some("main".into()),
        model: None,
        parent: None,
        created: 1,
        mark: Mark::Idle(Age::Fresh),
        since: Some(NOW - MIN),
        jobs: 0,
        window: Some(7),
        text: None,
        full: None,
        fork: None,
        terminal: None,
        news: false,
    }
}

fn reply(ago: u64, text: &str) -> Reply {
    Reply {
        at: NOW - ago,
        text: text.to_string(),
    }
}

fn source<'a>(chat: &'a Chat, replies: &'a [Reply], focused: Option<u64>) -> Source<'a> {
    Source {
        chat,
        replies,
        focused,
        server: None,
    }
}

fn texts(items: &[Item]) -> Vec<&str> {
    items.iter().map(|item| item.text.as_str()).collect()
}

#[test]
fn every_reply_of_the_last_day_shows_newest_first_and_each_chats_latest_older_one() {
    let a = chat("a");
    let b = chat("b");
    let a_replies = [
        reply(3 * DAY_MS, "a, three days ago"),
        reply(2 * DAY_MS, "a, two days ago"),
        reply(5 * HOUR, "a, five hours ago"),
        reply(MIN, "a, a minute ago"),
    ];
    let b_replies = [
        reply(30 * HOUR, "b, yesterday"),
        reply(2 * HOUR, "b, two hours ago"),
    ];
    let news = collect(
        &[source(&a, &a_replies, None), source(&b, &b_replies, None)],
        &Marks::default(),
        NOW,
    );
    assert_eq!(
        texts(&news.recent),
        ["a, a minute ago", "b, two hours ago", "a, five hours ago"]
    );
    assert_eq!(
        texts(&news.older),
        ["b, yesterday", "a, two days ago"],
        "one per chat, its latest older reply"
    );
    assert!(news.pinned.is_empty());
    assert_eq!(news.recent[0].project.as_deref(), Some("a-project"));
    assert_eq!(news.recent[0].key, key_of("a", NOW - MIN));
}

#[test]
fn a_chat_shows_at_most_twenty_replies_of_the_day() {
    let a = chat("a");
    let replies: Vec<Reply> = (1..=25).map(|n| reply(n * MIN, &format!("{n}"))).collect();
    let mut sorted = replies.clone();
    sorted.reverse();
    let news = collect(&[source(&a, &sorted, None)], &Marks::default(), NOW);
    assert_eq!(news.recent.len(), PER_CHAT);
    assert_eq!(news.recent[0].text, "1", "the newest first");
}

#[test]
fn chats_that_wait_are_pinned_with_their_mark_and_question() {
    let mut asks = chat("q");
    asks.mark = Mark::AsksYou;
    asks.text = Some("Asks you: which list".into());
    asks.full = Some("Which list should the search use first?".into());
    let mut approve = chat("p");
    approve.mark = Mark::NeedsApproval;
    approve.since = Some(NOW - 2 * MIN);
    approve.text = Some("Waits for your OK to run a command.".into());
    asks.since = Some(NOW - 6 * MIN);
    let news = collect(
        &[source(&asks, &[], None), source(&approve, &[], None)],
        &Marks::default(),
        NOW,
    );
    assert_eq!(
        texts(&news.pinned),
        [
            "Waits for your OK to run a command.",
            "Which list should the search use first?"
        ]
    );
    assert_eq!(news.pinned[0].mark, Some(Mark::NeedsApproval));
    assert_eq!(news.replies().count(), 0, "a pinned chat is not a reply");
}

#[test]
fn a_reply_is_unseen_until_the_app_shows_the_chat_or_it_is_read_here() {
    let a = chat("a");
    let replies = [reply(2 * HOUR, "older"), reply(10 * MIN, "newer")];
    let opened_between = Some(NOW - HOUR);
    let news = collect(
        &[source(&a, &replies, opened_between)],
        &Marks::default(),
        NOW,
    );
    let unseen: Vec<(&str, bool)> = news
        .recent
        .iter()
        .map(|item| (item.text.as_str(), item.unseen))
        .collect();
    assert_eq!(unseen, [("newer", true), ("older", false)]);

    let mut marks = Marks::default();
    marks.mark(&key_of("a", NOW - 10 * MIN), NOW - 10 * MIN);
    let news = collect(&[source(&a, &replies, opened_between)], &marks, NOW);
    assert!(news.recent.iter().all(|item| !item.unseen), "read here");

    let news = collect(&[source(&a, &replies, None)], &Marks::default(), NOW);
    assert!(news.recent.iter().all(|item| item.unseen), "never opened");
}

#[test]
fn a_server_reply_counts_only_the_marks_and_a_week_old_reply_is_seen() {
    let a = chat("rdehome2/-home-rde-app");
    let replies = [reply(8 * DAY_MS, "last week"), reply(MIN, "now")];
    let server = Source {
        server: Some("rdehome2"),
        focused: Some(NOW),
        ..source(&a, &replies, None)
    };
    let news = collect(&[server], &Marks::default(), NOW);
    assert!(news.recent[0].unseen, "no app focus time for a server chat");
    assert_eq!(news.recent[0].server.as_deref(), Some("rdehome2"));
    assert!(!news.older[0].unseen, "older than the marks are kept");
}

#[test]
fn the_folder_gives_the_project_name() {
    assert_eq!(
        project_of(Some(r"E:\code\WinCraft\")).as_deref(),
        Some("WinCraft")
    );
    assert_eq!(
        project_of(Some("/home/rde/projects/verdict-ai")).as_deref(),
        Some("verdict-ai")
    );
    assert_eq!(project_of(None), None);
}

fn sample_news() -> News {
    let item = |key: &str, unseen: bool, mark: Option<Mark>| Item {
        key: key.to_string(),
        chat: key.to_string(),
        title: key.to_string(),
        project: None,
        server: None,
        folder: None,
        window: None,
        at: NOW - MIN,
        text: format!("{key} said"),
        unseen,
        mark,
    };
    News {
        pinned: vec![item("p", true, Some(Mark::AsksYou))],
        recent: vec![item("r1", true, None), item("r2", false, None)],
        older: vec![item("o1", true, None), item("o2", false, None)],
    }
}

fn shape(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            Line::Head {
                head,
                count,
                folded,
                dot,
            } => format!(
                "{} {count}{}{}",
                head.name(),
                if *folded { " folded" } else { "" },
                if *dot { " dot" } else { "" }
            ),
            Line::Item(item) => item.key.clone(),
            Line::Empty => "empty".into(),
        })
        .collect()
}

#[test]
fn the_lines_are_pinned_then_the_day_then_older_folded_with_its_count() {
    let news = sample_news();
    let ui = NewsUi::default();
    let lines = view::lines(&news, false, |head| ui.is_folded(head));
    assert_eq!(
        shape(&lines),
        [
            "Waits for you 1",
            "p",
            "Last 24 hours 2",
            "r1",
            "r2",
            "Older 2 folded dot"
        ]
    );
    let mut ui = NewsUi::default();
    ui.fold(Head::Older, false);
    let lines = view::lines(&news, true, |head| ui.is_folded(head));
    assert_eq!(
        shape(&lines),
        [
            "Waits for you 1",
            "p",
            "Last 24 hours 1",
            "r1",
            "Older 1",
            "o1"
        ],
        "only unseen hides seen replies, never a pinned chat"
    );
    assert_eq!(
        view::picks(&lines)[..2],
        ["head:Waits for you".to_string(), "p".to_string()]
    );
}

#[test]
fn an_empty_tab_has_its_words_and_pinned_chats_keep_the_day_line() {
    let nothing = News::default();
    let lines = view::lines(&nothing, false, |_| false);
    assert_eq!(shape(&lines), ["empty"]);
    assert_eq!(
        view::EMPTY,
        "No replies in the last 24 hours. Each reply a chat finishes shows here."
    );
    let news = News {
        recent: Vec::new(),
        older: Vec::new(),
        ..sample_news()
    };
    let lines = view::lines(&news, false, |_| false);
    assert_eq!(shape(&lines), ["Waits for you 1", "p", "Last 24 hours 0"]);
}

#[test]
fn a_long_open_reply_is_cut_after_eleven_lines_with_show_all() {
    assert_eq!(view::cut(5, false), (5, None));
    assert_eq!(view::cut(11, false), (11, None));
    assert_eq!(
        view::cut(24, false),
        (11, Some("Show all \u{b7} 24 lines".to_string()))
    );
    assert_eq!(view::cut(24, true), (24, None));
    assert_eq!(view::short_text("One.\n\nTwo  three."), "One. Two three.");
}

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wincraft-news-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(FILE)
}

#[test]
fn the_tab_counts_unseen_replies_and_reading_marks_them_seen() {
    let mut ui = NewsUi {
        file: Some(temp_file("count")),
        ..NewsUi::default()
    };
    ui.take(sample_news());
    assert_eq!(ui.count, 2, "the pinned chat is not counted");
    assert_eq!(
        ui.unseen_chats,
        HashSet::from(["r1".to_string(), "o1".to_string()])
    );
    ui.toggle("r1", NOW);
    assert_eq!(ui.open.as_deref(), Some("r1"));
    assert_eq!(ui.count, 1, "opening an item reads it");
    ui.toggle("r1", NOW);
    assert_eq!(ui.open, None, "a second click closes it");
    ui.mark_all(NOW);
    assert_eq!(ui.count, 0);
    assert!(ui.unseen_chats.is_empty());
    let saved = load(&ui.file(), NOW);
    assert!(saved.has("r1") && saved.has("o1") && saved.has("r2"));
    assert!(!saved.has("p"), "a pinned chat has no mark");
}

#[test]
fn the_marks_file_keeps_a_week_and_a_broken_file_starts_fresh() {
    let file = temp_file("file");
    let mut marks = Marks::default();
    marks.mark("new", NOW - DAY_MS);
    marks.mark("old", NOW - 8 * DAY_MS);
    save(&file, &marks, NOW);
    let back = load(&file, NOW);
    assert!(back.has("new"));
    assert!(!back.has("old"), "marks older than 7 days are dropped");
    std::fs::write(&file, "{ not json").expect("write");
    assert_eq!(load(&file, NOW), Marks::default());
    let _ = std::fs::remove_file(&file);
    assert_eq!(load(&file, NOW), Marks::default(), "no file yet");
}

#[test]
fn the_tray_tooltip_adds_the_new_replies() {
    assert_eq!(
        view::tray_tip(true, 2, 4).as_deref(),
        Some("WinCraft \u{2014} 2 chats wait for you, 4 new replies")
    );
    assert_eq!(
        view::tray_tip(true, 0, 1).as_deref(),
        Some("WinCraft \u{2014} 1 new reply")
    );
    assert_eq!(
        view::tray_tip(true, 1, 0).as_deref(),
        Some("WinCraft \u{2014} 1 chat waits for you")
    );
    assert_eq!(view::tray_tip(true, 0, 0), None);
    assert_eq!(view::tray_tip(false, 2, 4), None);
}

#[test]
fn a_closed_window_keeps_the_marks_and_the_switch_only() {
    let mut ui = NewsUi {
        tab: Tab::News,
        only_unseen: true,
        marks: Some(Marks::default()),
        open: Some("x".into()),
        ..NewsUi::default()
    };
    let kept = ui.carry();
    assert_eq!(kept.tab, Tab::Chats);
    assert!(kept.only_unseen && kept.marks.is_some() && kept.open.is_none());
}
