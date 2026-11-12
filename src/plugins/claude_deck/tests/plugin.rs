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
            "Fold chats idle for over 1 day",
            "Show a count on the tray icon",
            "Status mark",
            "A line of text (the question, or what it does)",
            "Time in the status",
            "Project and branch",
            "Job count",
            "Model name",
            "Servers (ssh names)",
        ]
    );
    let defaults = deck.default_settings();
    for field in deck.settings_fields() {
        assert!(defaults.get(field.key).is_some(), "{}", field.key);
    }
}

#[test]
fn the_row_fields_sit_under_on_each_row() {
    use crate::core::traits::WinCraftPlugin;
    let deck = crate::plugins::claude_deck::ClaudeDeck::new();
    for field in deck.settings_fields() {
        let row = matches!(
            field.key,
            "status_mark" | "show_text" | "show_time" | "show_project" | "show_jobs" | "show_model"
        );
        let group = match field.key {
            "servers" => Some("Homelab"),
            _ => row.then_some("On each row"),
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
