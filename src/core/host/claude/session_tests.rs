//! Finding the windows of a chat named by its Claude Code session id, and
//! the chats that wait with no window, on made-up sessions.

use super::*;
use crate::core::claude_sessions::{Popout, Session, ShownWindow};

fn session(id: &str, cli: &str, title: &str) -> Session {
    Session {
        id: id.to_string(),
        cli_session_id: cli.to_string(),
        title: title.to_string(),
        last_activity: Some(10),
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
fn a_session_id_finds_the_windows_that_show_its_chat() {
    let sessions = vec![
        session("local_a", "cli-a", "Fix the tray"),
        session("local_b", "cli-b", "Write the docs"),
    ];
    let windows = vec![shown(1, "Fix the tray"), shown(2, "Write the docs")];
    let popouts: Vec<Popout> = Vec::new();
    assert_eq!(showing(&sessions, &popouts, &windows, "cli-a"), vec![1]);
    assert_eq!(showing(&sessions, &popouts, &windows, "cli-b"), vec![2]);
    assert!(showing(&sessions, &popouts, &windows, "cli-x").is_empty());
    assert!(showing(&sessions, &popouts, &windows, "").is_empty());
}

#[test]
fn a_waiting_chat_with_no_window_still_gets_a_row_mark() {
    let sessions = vec![
        session("local_a", "cli-a", "Fix the tray"),
        session("local_b", "cli-b", "Write the docs"),
        session("local_c", "", "No CLI id"),
    ];
    let windows = vec![ChatWindow {
        hwnd: 1,
        session_id: "local_a".into(),
        waiting_since: None,
        approval: false,
    }];
    let at = Instant::now();
    let waits = windowless_waits(&sessions, &windows, |cli| match cli {
        "cli-a" | "cli-b" => Some((at, Ask::Approval)),
        _ => None,
    });
    // local_a has a window, which carries its wait; local_c has no id.
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].session_id, "local_b");
    assert_eq!(waits[0].hwnd, 0);
    assert!(waits[0].approval);
    assert!(waits[0].waiting_since.is_some());
}
