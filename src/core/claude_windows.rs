//! Which Claude chats have a window open now, and which of those wait for
//! the user, for ClaudeDeck. The waiting marks live on the host thread, so
//! the host writes this list there (on `HostRequest::ClaudeDeckRefresh`)
//! and ClaudeDeck's window reads it on the UI thread.

use std::sync::Mutex;

/// One open window of the Claude desktop app and the chat it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatWindow {
    pub hwnd: isize,
    /// The chat's `sessionId`.
    pub session_id: String,
    /// When a Claude Code hook said the chat waits, in Unix milliseconds.
    pub waiting_since: Option<u64>,
}

static LATEST: Mutex<Vec<ChatWindow>> = Mutex::new(Vec::new());

pub fn publish(windows: Vec<ChatWindow>) {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = windows;
    }
}

/// The list the host wrote last; empty before the first.
pub fn latest() -> Vec<ChatWindow> {
    LATEST
        .lock()
        .map(|latest| latest.clone())
        .unwrap_or_default()
}

/// The window that shows this chat. A chat in two windows gives the one
/// listed first.
pub fn of<'a>(windows: &'a [ChatWindow], session_id: &str) -> Option<&'a ChatWindow> {
    windows
        .iter()
        .find(|window| window.session_id == session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_written_last_is_read_back() {
        let window = ChatWindow {
            hwnd: 7,
            session_id: "local_a".into(),
            waiting_since: Some(5),
        };
        publish(vec![window.clone()]);
        let latest = latest();
        assert_eq!(of(&latest, "local_a"), Some(&window));
        assert_eq!(of(&latest, "local_b"), None);
        publish(Vec::new());
        assert!(super::latest().is_empty());
    }
}
