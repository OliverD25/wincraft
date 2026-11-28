//! The plugin around the list: settings, bringing a window forward, the
//! kept place.

use super::*;

#[test]
fn settings_read_with_defaults_for_what_is_missing() {
    use crate::plugins::claude_deck::{MarkStyle, Settings};
    let defaults = Settings::read(&serde_json::json!({}));
    assert_eq!(defaults, Settings::default());
    assert!(defaults.stay_on_top && defaults.fold_cold && defaults.time);
    assert!(!defaults.only_open && !defaults.model);
    assert_eq!(defaults.mark, MarkStyle::Shapes);
    let set = Settings::read(&serde_json::json!({
        "stay_on_top": false,
        "only_open_windows": true,
        "status_mark": "Shapes and words",
        "show_model": true,
        "show_jobs": "yes",
    }));
    assert!(!set.stay_on_top && set.only_open && set.model && set.jobs);
    assert_eq!(set.mark, MarkStyle::ShapesAndWords);
    let off = Settings::read(&serde_json::json!({ "status_mark": "Off" }));
    assert_eq!(off.mark, MarkStyle::Off);
    assert!(defaults.fade && defaults.tray_count && !defaults.mini);
    let later = Settings::read(&serde_json::json!({
        "fade_when_away": false,
        "tray_count": false,
        "mini_mode": true,
    }));
    assert!(!later.fade && !later.tray_count && later.mini);
    assert!(
        defaults.servers.is_empty(),
        "off unless the user lists servers"
    );
    let listed = Settings::read(&serde_json::json!({ "servers": "rdehome1, rdehome2" }));
    assert_eq!(listed.servers, vec!["rdehome1", "rdehome2"]);
}

#[test]
fn the_cold_time_is_48_hours_unless_saved_and_stays_in_range() {
    use crate::core::traits::WinCraftPlugin;
    use crate::plugins::claude_deck::Settings;
    let missing = Settings::read(&serde_json::json!({}));
    assert_eq!(missing.cold_hours, 48);
    assert_eq!(missing.cold_ms(), 48 * 60 * 60 * 1000);
    let saved = Settings::read(&serde_json::json!({ "cold_after_hours": 36 }));
    assert_eq!(saved.cold_hours, 36);
    let low = Settings::read(&serde_json::json!({ "cold_after_hours": 0 }));
    let high = Settings::read(&serde_json::json!({ "cold_after_hours": 9999 }));
    assert_eq!((low.cold_hours, high.cold_hours), (1, 720));
    let deck = crate::plugins::claude_deck::ClaudeDeck::new();
    let defaults = Settings::read(&deck.default_settings());
    assert_eq!(defaults.cold_hours, 48, "the defaults file says 48 too");
}

/// The heading "On each row" became "View" (2026-11-18); the keys in the
/// settings file stayed, so a file saved before reads the same.
#[test]
fn a_settings_file_from_before_the_view_heading_reads_unchanged() {
    use crate::plugins::claude_deck::token_mark::Style as TokenStyle;
    use crate::plugins::claude_deck::{MarkStyle, Settings};
    let old = Settings::read(&serde_json::json!({
        "status_mark": "Off",
        "show_text": false,
        "show_time": false,
        "show_project": false,
        "show_jobs": false,
        "show_model": true,
        "show_token_bar": false,
    }));
    assert_eq!(old.mark, MarkStyle::Off);
    assert!(!old.text && !old.time && !old.project && !old.jobs && old.model);
    assert_eq!(
        old.token_mark,
        TokenStyle::Off,
        "a Token bar off reads as Off"
    );
    assert!(old.quick_numbers, "the new setting starts on");
    let hidden = Settings::read(&serde_json::json!({ "show_quick_numbers": false }));
    assert!(!hidden.quick_numbers);
}

#[test]
fn every_setting_of_the_design_page_is_there_and_mini_mode_is_not_one() {
    use crate::core::traits::WinCraftPlugin;
    let deck = crate::plugins::claude_deck::ClaudeDeck::new();
    let labels: Vec<&str> = deck.settings_fields().iter().map(|f| f.label).collect();
    assert_eq!(
        labels,
        [
            "Stay on top",
            "Only open windows",
            "Fade when the mouse is away",
            "Fold idle chats",
            "Cold after (hours)",
            "Live in the tray (no taskbar button)",
            "Show a count on the tray icon",
            "Status mark",
            "A line of text (the question, or what it does)",
            "Time in the status",
            "Project and branch",
            "Job count",
            "Model name",
            "Quick access numbers",
            "Token mark",
            "Token mark counts",
            "Servers (ssh names)",
        ]
    );
    let defaults = deck.default_settings();
    for field in deck.settings_fields() {
        assert!(defaults.get(field.key).is_some(), "{}", field.key);
    }
}

#[test]
fn the_row_fields_sit_under_view() {
    use crate::core::traits::WinCraftPlugin;
    let deck = crate::plugins::claude_deck::ClaudeDeck::new();
    for field in deck.settings_fields() {
        let row = matches!(
            field.key,
            "status_mark"
                | "show_text"
                | "show_time"
                | "show_project"
                | "show_jobs"
                | "show_model"
                | "show_quick_numbers"
                | "token_mark"
                | "token_mark_counts"
        );
        let group = match field.key {
            "servers" => Some("Homelab"),
            _ => row.then_some("View"),
        };
        assert_eq!(deck.settings_group(field.key), group, "{}", field.key);
    }
}

#[test]
fn only_a_listed_claude_window_is_brought_forward_and_never_by_a_test_copy() {
    let listed = [ChatWindow {
        hwnd: 42,
        session_id: "local_a".into(),
        waiting_since: None,
        approval: false,
    }];
    assert!(crate::plugins::claude_deck::may_activate(
        42, &listed, false
    ));
    assert!(!crate::plugins::claude_deck::may_activate(
        42, &listed, true
    ));
    assert!(!crate::plugins::claude_deck::may_activate(
        43, &listed, false
    ));
    assert!(!crate::plugins::claude_deck::may_activate(
        0, &listed, false
    ));
}

#[test]
fn the_place_is_kept_only_when_it_is_worth_it_and_reads_back() {
    use crate::plugins::claude_deck::place::{load, save, worth_keeping, Place};
    let place = Place {
        x: 100,
        y: -20,
        width: 360,
        height: 480,
        dpi: 144,
    };
    assert!(worth_keeping(false, true, place));
    assert!(!worth_keeping(true, true, place));
    assert!(!worth_keeping(false, false, place));
    let minimised = Place {
        x: -32000,
        y: -32000,
        ..place
    };
    assert!(!worth_keeping(false, true, minimised));
    let tiny = Place { width: 10, ..place };
    assert!(!worth_keeping(false, true, tiny));

    let folder = std::env::temp_dir().join(format!("wincraft-claude-deck-{}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    let path = folder.join("place.json");
    assert_eq!(load(&path), None);
    save(&path, place);
    assert_eq!(load(&path), Some(place));
    std::fs::write(&path, r#"{"x":1,"y":2,"width":0,"height":5}"#).expect("written");
    assert_eq!(load(&path), None);
    let _ = std::fs::remove_dir_all(&folder);
}
