use egui::{Event, Key, Modifiers};
use serde_json::json;

use super::*;
use crate::core::config::PluginConfig;
use crate::core::traits::WinCraftPlugin;
use crate::plugins::claude_deck::ClaudeDeck;

fn key(modifiers: Modifiers) -> Vec<Event> {
    vec![Event::Key {
        key: Key::B,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }]
}

/// What the host does at start: fill in the defaults, then migrate.
fn started(saved: serde_json::Value) -> Settings {
    let deck = ClaudeDeck::new();
    let mut plugin: PluginConfig =
        serde_json::from_value(json!({ "settings": saved })).expect("a plugin file");
    plugin.merge_defaults(&[], &deck.default_settings());
    deck.migrate_settings(&mut plugin.settings);
    assert!(plugin.settings.get(OLD_KEY).is_none(), "the old key goes");
    Settings::read(&plugin.settings)
}

#[test]
fn an_old_token_bar_reads_as_line_or_off() {
    assert_eq!(started(json!({ OLD_KEY: true })).token_mark, Style::Line);
    assert_eq!(started(json!({ OLD_KEY: false })).token_mark, Style::Off);
    assert_eq!(started(json!({ OLD_KEY: false })).token_last, Style::Line);
    let fresh = started(json!({}));
    assert_eq!(fresh.token_mark, Style::Line, "Line is the default");
    assert_eq!(
        fresh.token_counts,
        Counts::Week,
        "the last 7 days by default"
    );
    let kept = started(json!({ KEY: FILL, COUNTS_KEY: TODAY }));
    assert_eq!(
        (kept.token_mark, kept.token_counts),
        (Style::Fill, Counts::Today)
    );
    assert_eq!(
        read(&json!({ OLD_KEY: false })).0,
        Style::Off,
        "without the host's defaults too"
    );
}

#[test]
fn ctrl_b_turns_the_mark_off_and_back_to_its_last_style() {
    let mut settings = Settings::default();
    set_style(&mut settings, Style::Signal);
    let saves = flip(&mut settings);
    assert_eq!(settings.token_mark, Style::Off);
    assert_eq!(
        saves,
        vec![(KEY, json!(OFF)), (LAST_KEY, json!(SIGNAL))],
        "the style it had is saved for the next start"
    );
    let after_restart = Settings::read(&json!({ KEY: OFF, LAST_KEY: SIGNAL }));
    assert_eq!(after_restart.token_last, Style::Signal);
    flip(&mut settings);
    assert_eq!(settings.token_mark, Style::Signal);
}

#[test]
fn ctrl_shift_b_switches_today_and_the_last_7_days() {
    let mut settings = Settings::default();
    assert_eq!(
        switch_counts(&mut settings),
        vec![(COUNTS_KEY, json!(TODAY))]
    );
    assert_eq!(settings.token_counts, Counts::Today);
    switch_counts(&mut settings);
    assert_eq!(settings.token_counts, Counts::Week);
}

#[test]
fn the_settings_page_turning_it_off_keeps_the_style() {
    let before = Settings {
        token_mark: Style::Fill,
        token_last: Style::Fill,
        ..Settings::default()
    };
    let mut after = Settings::read(&json!({ KEY: OFF }));
    assert_eq!(keep_last(&before, &mut after), Some(Style::Fill));
    assert_eq!(after.token_last, Style::Fill);
    let mut again = after.clone();
    assert_eq!(keep_last(&after, &mut again), None, "already off");
}

#[test]
fn the_keys_need_ctrl_and_never_act_while_typing() {
    let ctrl = Modifiers::COMMAND;
    assert_eq!(asked(&key(ctrl), false), Some(Asked::Flip));
    assert_eq!(
        asked(&key(ctrl | Modifiers::SHIFT), false),
        Some(Asked::Counts)
    );
    assert_eq!(asked(&key(ctrl), true), None, "a rename has the keyboard");
    assert_eq!(asked(&key(Modifiers::NONE), false), None);
    assert_eq!(asked(&key(ctrl | Modifiers::ALT), false), None);
}

#[test]
fn the_tip_says_which_tokens_and_the_bars_follow_the_share() {
    assert_eq!(tip("12%", Counts::Today), "12% of today");
    assert_eq!(tip("12%", Counts::Week), "12% of the last 7 days");
    assert_eq!(
        [3.0, 6.0, 11.9, 12.0, 24.9, 25.0, 80.0].map(level),
        [1, 2, 2, 3, 3, 4, 4]
    );
}

#[test]
fn the_palette_commands_carry_out_the_keys() {
    let deck = ClaudeDeck::new();
    let labels: Vec<&str> = deck.palette_commands().iter().map(|c| c.label).collect();
    assert!(labels.contains(&ON_OFF_COMMAND) && labels.contains(&COUNTS_COMMAND));
    let ids: Vec<u32> = deck.palette_commands().iter().map(|c| c.id).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "every command has its own id");
    assert_eq!(asked_by(command(Asked::Flip).id), Some(Asked::Flip));
    assert_eq!(asked_by(command(Asked::Counts).id), Some(Asked::Counts));
    assert_eq!(asked_by(1), None);
}
