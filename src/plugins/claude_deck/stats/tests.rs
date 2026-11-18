use std::collections::HashMap;

use egui::{pos2, Rect};

use crate::core::claude_usage::tally::ModelUse;
use crate::core::claude_usage::{ChatUse, Snapshot, Span, Tokens};
use crate::plugins::claude_deck::model::{Chat, Mark};
use crate::plugins::claude_deck::store::{Group, Saved};

use super::view::{self, Bar};
use super::{bar, StatsUi};

fn tokens(output: u64) -> Tokens {
    Tokens {
        input: 10,
        output,
        read: 1_000,
        write: 100,
        write_1h: 0,
    }
}

fn chat_use(session: &str, weight: f64, sub: f64) -> ChatUse {
    ChatUse {
        session: session.to_string(),
        folder: "E--code--wincraft".to_string(),
        tokens: tokens(500),
        weight,
        subagent_tokens: tokens(100),
        subagent_weight: sub,
        context: Some(120_000),
        other_model: false,
    }
}

fn week() -> Span {
    Span {
        chats: vec![
            chat_use("cli-a", 3.0, 1.5),
            chat_use("cli-b", 1.0, 0.0),
            chat_use("cli-c", 0.5, 0.0),
        ],
        models: vec![
            ModelUse {
                model: "claude-opus-5-5".into(),
                tokens: tokens(900),
                weight: 4.0,
                known: true,
            },
            ModelUse {
                model: "claude-new-9".into(),
                tokens: tokens(10),
                weight: 0.5,
                known: false,
            },
        ],
        days: (100..=106).map(|d| (d, 0.0)).collect(),
        hours: [0.0; 24],
        tokens: tokens(1500),
        weight: 4.5,
    }
}

fn chat(id: &str, title: &str, parent: Option<&str>) -> Chat {
    Chat {
        id: id.to_string(),
        title: title.to_string(),
        folder: None,
        branch: None,
        model: None,
        parent: parent.map(str::to_string),
        created: 0,
        mark: Mark::Unknown,
        since: None,
        jobs: 0,
        window: None,
        text: None,
        full: None,
        fork: None,
        terminal: None,
        news: false,
        job_on: None,
        context: None,
    }
}

fn chat_of() -> HashMap<String, String> {
    [("cli-a", "a"), ("cli-b", "b")]
        .into_iter()
        .map(|(cli, id)| (cli.to_string(), id.to_string()))
        .collect()
}

#[test]
fn the_biggest_chat_fills_its_row_and_others_are_shares_of_it() {
    let shares = view::shares(&week(), &chat_of());
    assert_eq!(shares.get("a"), Some(&1.0));
    assert!((shares["b"] - 1.0 / 3.0).abs() < 1e-6);
    assert_eq!(shares.get("c"), None, "not listed in ClaudeDeck");
    let row = Rect::from_min_max(pos2(0.0, 0.0), pos2(224.0, 30.0));
    assert_eq!(bar::rect(row, 1.0).width(), 200.0);
    assert_eq!(bar::rect(row, 0.0).width(), 2.0, "a tiny share still shows");
    assert_eq!(bar::rect(row, 1.0).height(), bar::HEIGHT);
}

#[test]
fn chat_bars_take_listed_titles_and_the_folder_for_others() {
    let chats = vec![chat("a", "Stats tab", None), chat("b", "Fix", None)];
    let bars = view::chat_bars(&week(), &chats, &chat_of());
    assert_eq!(bars[0].name, "Stats tab");
    assert_eq!(bars[0].share, "67%");
    assert_eq!(bars[0].chat.as_deref(), Some("a"));
    assert_eq!(bars[2].name, "wincraft (not listed)");
    assert_eq!(bars[2].chat, None);
    let tip = &bars[0].tip;
    assert!(tip.contains("Output 500"), "{tip}");
    assert!(tip.contains("Subagents: 50%"), "{tip}");
    assert!(tip.contains("Cache read: 62% of the tokens"), "{tip}");
    assert!(tip.contains("Context now: 120k"), "{tip}");
}

#[test]
fn groups_sum_their_chats_and_forks_count_for_their_parents_group() {
    let chats = vec![chat("a", "A", None), chat("b", "B", Some("a"))];
    let saved = Saved {
        groups: vec![Group {
            id: "g1".into(),
            name: "WinCraft".into(),
            chats: vec!["a".into()],
            ..Group::default()
        }],
        ..Saved::default()
    };
    let bars: Vec<Bar> = view::group_bars(&week(), &chats, &chat_of(), &saved);
    assert_eq!(bars[0].name, "WinCraft");
    assert_eq!(bars[0].share, "89%");
    assert_eq!(bars[1].name, view::NO_GROUP);
    assert_eq!(bars[1].length, 0.5 / 4.0);
}

#[test]
fn models_names_counts_and_labels_read_plainly() {
    assert_eq!(view::model_name("claude-opus-5-5"), "Opus 5.5");
    assert_eq!(view::model_name("claude-sonnet-5-5[1m]"), "Sonnet 5.5");
    assert_eq!(view::model_name("claude-haiku-4-5-20251001"), "Haiku 4.5");
    let models = view::model_bars(&week());
    assert_eq!(models[0].share, "89%");
    assert!(models[1].tip.contains("Other model"));
    assert_eq!(view::count(950), "950");
    assert_eq!(view::count(34_400), "34k");
    assert_eq!(view::count(1_240_000), "1.2M");
    assert_eq!(view::percent(0.001, 10.0), "<1%");
    assert_eq!(
        view::counting(120, 1538),
        "Counting\u{2026} 120 of 1,538 files"
    );
    assert_eq!(
        view::folder_name("E--codespace--rde-wincraft"),
        "rde-wincraft"
    );
    assert_eq!(view::day_label(20_775, 20_775), "Today");
    assert_eq!(view::day_label(20_774, 20_775), "Tue");
    assert_eq!(view::heights(&[0.0, 2.0, 1.0]), vec![0.0, 1.0, 0.5]);
}

#[test]
fn the_reader_never_starts_in_tests_and_the_bars_come_from_the_summary() {
    let mut stats = StatsUi {
        bar_on: true,
        ..StatsUi::default()
    };
    stats.tick(&[], false);
    assert!(!stats.is_reading(), "tests never read the user's chats");
    stats.set_latest(Snapshot {
        week: week(),
        ..Snapshot::default()
    });
    stats.chat_of = chat_of();
    stats.set_latest(Snapshot {
        week: week(),
        ..Snapshot::default()
    });
    assert_eq!(stats.share("a"), Some(1.0));
}
