//! The News tab without drawing: which replies show where, what is
//! unseen, the read marks and their file, the lines and the words.

use std::collections::HashSet;

use crate::core::claude_sessions::Age;
use crate::core::claude_turns::Reply;

use super::super::model::{Chat, Mark, DAY_MS};
use super::collect::{collect, key_of, project_of, Item, News, Source, PER_CHAT};
use super::view::{self, ChatFold, Folds, Head, Line};
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
        job_on: None,
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
        status: mark.unwrap_or(Mark::Idle(Age::Fresh)),
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
            Line::Block {
                item, unseen, open, ..
            } => format!(
                "[{}]{}{}",
                item.chat,
                if *unseen > 0 {
                    format!(" {unseen}")
                } else {
                    String::new()
                },
                if *open { "" } else { " folded" }
            ),
            Line::Item(item) => item.key.clone(),
            Line::Earlier { chat, count } => format!("{chat}: {}", view::earlier(*count)),
            Line::Empty => "empty".into(),
        })
        .collect()
}

#[test]
fn the_lines_are_pinned_then_the_day_then_older_folded_with_its_count() {
    let news = sample_news();
    let ui = NewsUi::default();
    let lines = view::lines(&news, &ui);
    assert_eq!(
        shape(&lines),
        [
            "Waits for you 1",
            "[p]",
            "p",
            "Last 24 hours 2",
            "[r1] 1",
            "r1",
            "[r2]",
            "r2",
            "Older 2 folded dot"
        ]
    );
    let mut ui = NewsUi {
        only_unseen: true,
        ..NewsUi::default()
    };
    ui.fold(Head::Older, false);
    let lines = view::lines(&news, &ui);
    assert_eq!(
        shape(&lines),
        [
            "Waits for you 1",
            "[p]",
            "p",
            "Last 24 hours 1",
            "[r1] 1",
            "r1",
            "Older 1",
            "[o1] 1",
            "o1"
        ],
        "only unseen hides seen replies, never a pinned chat"
    );
    assert_eq!(
        view::picks(&lines)[..3],
        [
            "head:Waits for you".to_string(),
            "chat:Waits for you:p".to_string(),
            "p".to_string()
        ]
    );
}

#[test]
fn an_empty_tab_has_its_words_and_pinned_chats_keep_the_day_line() {
    let nothing = News::default();
    let lines = view::lines(&nothing, &NewsUi::default());
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
    let lines = view::lines(&news, &NewsUi::default());
    assert_eq!(
        shape(&lines),
        ["Waits for you 1", "[p]", "p", "Last 24 hours 0"]
    );
}

/// A reply of `chat`, `ago` minutes ago.
fn said(chat: &str, key: &str, ago: u64, unseen: bool) -> Item {
    Item {
        key: key.to_string(),
        chat: chat.to_string(),
        title: chat.to_string(),
        project: None,
        server: None,
        folder: None,
        window: None,
        at: NOW - ago * MIN,
        text: format!("{key} said"),
        unseen,
        mark: None,
        status: Mark::Idle(Age::Fresh),
    }
}

fn day(recent: Vec<Item>) -> News {
    News {
        recent,
        ..News::default()
    }
}

#[test]
fn replies_group_into_one_block_per_chat_in_the_order_of_their_newest() {
    let news = day(vec![
        said("a", "a1", 1, false),
        said("b", "b1", 2, true),
        said("a", "a2", 3, false),
        said("b", "b2", 10, false),
    ]);
    let blocks = view::by_chat(&news.recent);
    let keys: Vec<Vec<&str>> = blocks
        .iter()
        .map(|block| block.iter().map(|item| item.key.as_str()).collect())
        .collect();
    assert_eq!(keys, [["a1", "a2"], ["b1", "b2"]]);
    let lines = view::lines(&news, &NewsUi::default());
    assert_eq!(
        shape(&lines),
        [
            "Last 24 hours 2",
            "[a]",
            "a1",
            "a: 1 earlier reply",
            "[b] 1",
            "b1",
            "b: 1 earlier reply"
        ]
    );
}

#[test]
fn a_folded_chat_lists_its_unseen_replies_or_its_newest() {
    let items = [
        said("a", "u1", 1, true),
        said("a", "s1", 2, false),
        said("a", "u2", 3, true),
        said("a", "s2", 4, false),
    ];
    let all: Vec<&Item> = items.iter().collect();
    let keys = |shown: &[&Item]| {
        shown
            .iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let (shown, rest) = view::fold(&all, false, |_| false);
    assert_eq!((keys(&shown), rest), ("u1 u2".to_string(), 2));
    let (shown, rest) = view::fold(&all, false, |key| key == "s2");
    assert_eq!(
        (keys(&shown), rest),
        ("u1 u2 s2".to_string(), 1),
        "the open or picked reply stays"
    );
    let (shown, rest) = view::fold(&all, true, |_| false);
    assert_eq!((shown.len(), rest), (4, 0), "an unfolded chat lists all");
    let seen = [said("a", "s1", 1, false), said("a", "s2", 2, false)];
    let seen: Vec<&Item> = seen.iter().collect();
    let (shown, rest) = view::fold(&seen, false, |_| false);
    assert_eq!((keys(&shown), rest), ("s1".to_string(), 1));
    assert_eq!(view::earlier(1), "1 earlier reply");
    assert_eq!(view::earlier(3), "3 earlier replies");
}

#[test]
fn a_chat_goes_back_to_the_default_when_it_gets_a_new_reply() {
    let mut ui = NewsUi::default();
    ui.take(day(vec![
        said("a", "a1", 5, false),
        said("a", "a2", 9, false),
    ]));
    ui.fold_chat("a", ChatFold::AllOpen);
    let news = Arc::clone(&ui.news);
    assert_eq!(
        shape(&view::lines(&news, &ui)),
        ["Last 24 hours 1", "[a]", "a1", "a2"]
    );
    ui.take(day(vec![
        said("a", "a1", 5, false),
        said("a", "a2", 9, false),
    ]));
    assert_eq!(
        ui.chat_fold("a"),
        ChatFold::AllOpen,
        "the same replies keep it"
    );
    ui.fold_chat("a", ChatFold::Collapsed);
    ui.take(day(vec![
        said("a", "a0", 1, true),
        said("a", "a1", 5, false),
        said("a", "a2", 9, false),
    ]));
    assert_eq!(
        ui.chat_fold("a"),
        ChatFold::Default,
        "a new reply is never hidden, a collapsed chat opens again"
    );
    let news = Arc::clone(&ui.news);
    assert_eq!(
        shape(&view::lines(&news, &ui)),
        ["Last 24 hours 1", "[a] 1", "a0", "a: 2 earlier replies"]
    );
}

#[test]
fn the_arrow_collapses_any_chat_and_brings_back_the_default() {
    use ChatFold::*;
    assert_eq!(Default.arrow(), Collapsed);
    assert_eq!(AllOpen.arrow(), Collapsed);
    assert_eq!(Collapsed.arrow(), Default);
    assert_eq!(Collapsed.right(), Default);
    assert_eq!(Default.right(), AllOpen);
    assert_eq!(AllOpen.right(), AllOpen);
    assert_eq!(Default.left(), Collapsed);
    assert_eq!(AllOpen.left(), Collapsed);
}

#[test]
fn a_collapsed_chat_shows_only_its_block_line_with_its_count() {
    let mut ui = NewsUi::default();
    ui.take(day(vec![
        said("a", "a1", 1, true),
        said("a", "a2", 2, true),
        said("b", "b1", 3, false),
    ]));
    let unread = shape(&view::lines(&Arc::clone(&ui.news), &ui));
    assert_eq!(
        unread[..4],
        ["Last 24 hours 2", "[a] 2", "a1", "a2"],
        "an unread chat lists its replies by default"
    );
    ui.fold_chat("a", ChatFold::Collapsed);
    let news = Arc::clone(&ui.news);
    let lines = view::lines(&news, &ui);
    assert_eq!(
        shape(&lines),
        ["Last 24 hours 2", "[a] 2 folded", "[b]", "b1"],
        "the count stays and the arrow points right"
    );
    assert_eq!(ui.count, 2, "collapsing marks nothing seen");
    assert_eq!(
        view::picks(&lines),
        [
            "head:Last 24 hours",
            "chat:Last 24 hours:a",
            "chat:Last 24 hours:b",
            "b1"
        ]
    );
}

#[test]
fn collapsing_closes_the_open_reply_and_moves_the_keyboard_to_the_block_line() {
    let mut question = said("w", "wait:w", 1, true);
    question.mark = Some(Mark::AsksYou);
    let mut ui = NewsUi::default();
    ui.take(News {
        pinned: vec![question],
        recent: vec![said("w", "w1", 2, true), said("a", "a1", 3, true)],
        older: Vec::new(),
    });
    ui.open = Some("w1".into());
    ui.selected = Some("w1".into());
    ui.fold_chat("w", ChatFold::Collapsed);
    assert_eq!(ui.open, None);
    assert_eq!(
        ui.selected.as_deref(),
        Some("chat:Waits for you:w"),
        "a waiting chat's reply sits in its pinned block"
    );
    ui.open = Some("a1".into());
    ui.selected = Some("a1".into());
    ui.fold_chat("w", ChatFold::Default);
    ui.fold_chat("w", ChatFold::Collapsed);
    assert_eq!(
        (ui.open.as_deref(), ui.selected.as_deref()),
        (Some("a1"), Some("a1")),
        "another chat's reply stays open"
    );
    ui.fold_chat("a", ChatFold::Collapsed);
    assert_eq!(ui.selected.as_deref(), Some("chat:Last 24 hours:a"));
}

#[test]
fn only_unseen_hides_a_block_with_no_unseen_reply_but_never_a_waiting_chat() {
    let mut question = said("w", "wait:w", 1, true);
    question.mark = Some(Mark::AsksYou);
    let news = News {
        pinned: vec![question],
        recent: vec![
            said("w", "w1", 2, false),
            said("a", "a1", 3, false),
            said("b", "b1", 4, true),
        ],
        older: Vec::new(),
    };
    assert_eq!(
        shape(&view::lines(&news, &NewsUi::default())),
        [
            "Waits for you 1",
            "[w]",
            "wait:w",
            "w: 1 earlier reply",
            "Last 24 hours 2",
            "[a]",
            "a1",
            "[b] 1",
            "b1"
        ],
        "a waiting chat's replies sit in its pinned block, not twice"
    );
    let ui = NewsUi {
        only_unseen: true,
        ..NewsUi::default()
    };
    assert_eq!(
        shape(&view::lines(&news, &ui)),
        [
            "Waits for you 1",
            "[w]",
            "wait:w",
            "Last 24 hours 1",
            "[b] 1",
            "b1"
        ]
    );
}

#[test]
fn the_keys_go_over_groups_blocks_and_replies_and_skip_the_earlier_line() {
    let news = day(vec![
        said("a", "a1", 1, true),
        said("a", "a2", 2, false),
        said("b", "b1", 3, false),
    ]);
    let lines = view::lines(&news, &NewsUi::default());
    assert_eq!(
        view::picks(&lines),
        [
            "head:Last 24 hours",
            "chat:Last 24 hours:a",
            "a1",
            "chat:Last 24 hours:b",
            "b1"
        ]
    );
    assert_eq!(
        view::of_block_key("chat:Last 24 hours:a"),
        Some((Head::Recent, "a"))
    );
    assert_eq!(view::of_block_key("a1"), None);
    assert_eq!(view::of_block_key("head:Older"), None);
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
