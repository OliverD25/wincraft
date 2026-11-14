//! Which Claude chats have a window open now, and which of those wait for
//! the user, for ClaudeDeck. The waiting marks live on the host thread, so
//! the host writes this list there (on `HostRequest::ClaudeDeckRefresh`)
//! and ClaudeDeck's window reads it on the UI thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// One open window of the Claude desktop app and the chat it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatWindow {
    pub hwnd: isize,
    /// The chat's `sessionId`.
    pub session_id: String,
    /// When a Claude Code hook said the chat waits, in Unix milliseconds.
    pub waiting_since: Option<u64>,
    /// The hook said the wait is a permission prompt, not a question.
    pub approval: bool,
}

static LATEST: Mutex<Vec<ChatWindow>> = Mutex::new(Vec::new());
/// Chats that wait by WinCraft's own hook and have no window of their own;
/// each has `hwnd` 0.
static WAITS: Mutex<Vec<ChatWindow>> = Mutex::new(Vec::new());
static WANTED: AtomicBool = AtomicBool::new(false);

/// ClaudeDeck's window opens (true) or closes (false). A refresh the host
/// takes after the close does nothing, so a closed window leaves no chats
/// to read behind.
pub fn set_wanted(on: bool) {
    WANTED.store(on, Ordering::Release);
}

pub fn wanted() -> bool {
    WANTED.load(Ordering::Acquire)
}

pub fn publish(windows: Vec<ChatWindow>) {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = windows;
    }
}

pub fn publish_waits(waits: Vec<ChatWindow>) {
    if let Ok(mut latest) = WAITS.lock() {
        *latest = waits;
    }
}

/// The windowless waits the host wrote last.
pub fn waits() -> Vec<ChatWindow> {
    WAITS.lock().map(|waits| waits.clone()).unwrap_or_default()
}

/// The list the host wrote last; empty before the first.
pub fn latest() -> Vec<ChatWindow> {
    LATEST
        .lock()
        .map(|latest| latest.clone())
        .unwrap_or_default()
}

/// The window in front now, to tell whether the user looked at a chat.
pub fn foreground() -> isize {
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as isize }
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
    fn a_closed_deck_wants_nothing() {
        set_wanted(true);
        assert!(wanted());
        set_wanted(false);
        assert!(!wanted(), "a refresh after the close does nothing");
    }

    #[test]
    fn the_list_written_last_is_read_back() {
        let window = ChatWindow {
            hwnd: 7,
            session_id: "local_a".into(),
            waiting_since: Some(5),
            approval: false,
        };
        publish(vec![window.clone()]);
        let latest = latest();
        assert_eq!(of(&latest, "local_a"), Some(&window));
        assert_eq!(of(&latest, "local_b"), None);
        publish(Vec::new());
        assert!(super::latest().is_empty());
    }
}
