//! The Stats tab and the token bar, drawn from a summary set by hand: the
//! tests never read the user's transcripts.

use egui::Key;

use super::deck::{sample, Deck};
use crate::core::claude_usage::tally::ModelUse;
use crate::core::claude_usage::{ChatUse, Progress, Snapshot, Span, Tokens};
use crate::plugins::claude_deck::news::Tab;
use crate::plugins::claude_deck::stats::{view, Spans};
use crate::plugins::claude_deck::Settings;

fn week() -> Span {
    let tokens = Tokens {
        input: 10,
        output: 2_000,
        read: 90_000,
        write: 1_000,
        write_1h: 0,
    };
    let used = |session: &str, weight: f64| ChatUse {
        session: session.into(),
        folder: "E--b--Beta".into(),
        tokens,
        weight,
        ..ChatUse::default()
    };
    Span {
        chats: vec![used("cli-b1", 2.0), used("cli-a1", 1.0)],
        models: vec![ModelUse {
            model: "claude-opus-5-5".into(),
            tokens,
            weight: 3.0,
            known: true,
        }],
        days: (0..7).map(|d| (20_769 + d, 0.5)).collect(),
        hours: [0.125; 24],
        tokens,
        weight: 3.0,
    }
}

fn deck_with_stats(settings: Settings, counting: Option<Progress>) -> Deck {
    let mut deck = Deck::new(sample(), settings);
    deck.state.stats.chat_of = [("cli-b1", "b1"), ("cli-a1", "a1")]
        .into_iter()
        .map(|(cli, id)| (cli.to_string(), id.to_string()))
        .collect();
    deck.state.stats.set_latest(Snapshot {
        today: 20_775,
        week: week(),
        counting,
        ..Snapshot::default()
    });
    deck
}

#[test]
fn the_stats_tab_shows_the_top_chats_models_days_and_hours() {
    let mut deck = deck_with_stats(Settings::default(), None);
    deck.state.news.tab = Tab::Stats;
    let painted = deck.settle();
    for words in [
        view::TODAY,
        view::WEEK,
        view::NOTE,
        view::CHATS,
        "Chat b1",
        "67%",
        view::GROUPS,
        view::DAYS,
        "Mon",
        view::MODELS,
        "Opus 5.5",
        view::HOURS,
    ] {
        assert!(painted.has(words), "{words}: {:?}", painted.texts);
    }
    deck.click(painted.at(view::TODAY));
    assert_eq!(deck.state.stats.span, Spans::Today);
    let painted = deck.settle();
    assert!(painted.has(view::EMPTY), "{:?}", painted.texts);
    assert!(!painted.has(view::DAYS));
    deck.key(Key::ArrowRight);
    assert_eq!(deck.state.stats.span, Spans::Week, "Right picks 7 days");
}

#[test]
fn the_first_pass_shows_how_far_it_counted() {
    let progress = Progress {
        done: 120,
        total: 1_538,
    };
    let mut deck = deck_with_stats(Settings::default(), Some(progress));
    deck.state.news.tab = Tab::Stats;
    let painted = deck.settle();
    assert!(
        painted.has("Counting\u{2026} 120 of 1,538 files"),
        "{:?}",
        painted.texts
    );
    assert!(!painted.has(view::CHATS));
}

#[test]
fn the_token_bar_draws_under_the_chats_that_used_tokens() {
    let shapes = |token_bar: bool| {
        let settings = Settings {
            token_bar,
            ..Settings::default()
        };
        deck_with_stats(settings, None).settle().shapes
    };
    assert_eq!(
        shapes(true),
        shapes(false) + 2,
        "one bar for each of two chats"
    );
}
