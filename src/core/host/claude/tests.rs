//! The matching of windows to chats, on made-up sessions. Nothing is read
//! from the user's own Claude files and no real window is looked at.

use super::*;
use crate::core::claude_sessions::{Popout, Session, ShownWindow};
use crate::core::ui_bridge::{ArrangeGroup, ArrangeWindow};

fn session(id: &str, title: &str, active: u64) -> Session {
    Session {
        id: id.to_string(),
        title: title.to_string(),
        last_activity: Some(active),
        ..Session::default()
    }
}

fn shown(hwnd: isize, title: &str) -> ShownWindow {
    ShownWindow {
        hwnd,
        title: title.to_string(),
        width: 1200,
        height: 800,
        scale: 1.0,
    }
}

#[test]
fn a_window_is_matched_by_its_title_then_by_its_size() {
    let sessions = vec![
        session("s1", "Fix the tray", 10),
        session("s2", "Write docs", 20),
    ];
    let by_size = vec![(7, "s2".to_string())];
    let by_title = session_for(&sessions, &[], &by_size, &shown(5, "Fix the tray"));
    assert_eq!(by_title.map(|s| s.id.as_str()), Some("s1"));
    let renamed = session_for(&sessions, &[], &by_size, &shown(7, "A title nobody knows"));
    assert_eq!(
        renamed.map(|s| s.id.as_str()),
        Some("s2"),
        "found by its size"
    );
    assert!(session_for(&sessions, &[], &by_size, &shown(8, "A title nobody knows")).is_none());
    let dangling = vec![(9, "gone".to_string())];
    assert!(session_for(&sessions, &[], &dangling, &shown(9, "")).is_none());
}

#[test]
fn a_popout_title_finds_its_session() {
    let sessions = vec![session("s1", "Fix the tray", 10)];
    let popouts = vec![Popout {
        session_id: "s1".into(),
        title: "Popped out".into(),
        width: 0,
        height: 0,
    }];
    let found = session_for(&sessions, &popouts, &[], &shown(1, "Popped out"));
    assert_eq!(found.map(|s| s.id.as_str()), Some("s1"));
}

fn window(hwnd: isize) -> ArrangeWindow {
    ArrangeWindow {
        hwnd,
        label: "one".to_string(),
        desktop: String::new(),
        monitor: None,
        on_monitor: None,
        saved_monitor: None,
        renamed: false,
        same_title: false,
        attention: false,
        claude: None,
    }
}

#[test]
fn only_groups_of_the_claude_app_get_a_card_and_others_are_left_alone() {
    let mut groups = vec![ArrangeGroup {
        key: "Chrome.UserData.Profile3".to_string(),
        label: "Chrome".to_string(),
        windows: vec![window(0x7fff_fff0)],
    }];
    fill_claude_cards(&mut groups);
    assert!(groups[0].windows[0].claude.is_none());
    fill_claude_cards(&mut []);
}

#[test]
fn publishing_chat_windows_waits_until_the_deck_is_open() {
    claude_windows::set_wanted(false);
    publish_chat_windows();
    assert!(claude_windows::latest().is_empty());
}

#[test]
fn a_window_that_does_not_exist_has_an_empty_title_and_size() {
    let seen = shown_window(0x7fff_fff0);
    assert_eq!(seen.hwnd, 0x7fff_fff0);
    assert!(seen.title.is_empty());
    assert_eq!(seen.width, 0);
}
