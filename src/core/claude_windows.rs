//! Which Claude chats have a window open now, and which of those wait for
//! the user, for ClaudeDeck. The waiting marks live on the host thread, so
//! the host writes this list there (on `HostRequest::ClaudeDeckRefresh`)
//! and ClaudeDeck's window reads it on the UI thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

use super::claude_sessions::{self, Popout, Session, ShownWindow};
use super::windows_list;

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

/// A Claude window as the matching sees it. A renamed window is matched by
/// the title the app gave it.
pub fn shown_window(hwnd: isize) -> ShownWindow {
    let handle = hwnd as HWND;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(handle, &mut rect) };
    ShownWindow {
        hwnd,
        title: windows_list::window_text(handle),
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
        scale: f64::from(unsafe { GetDpiForWindow(handle) }) / 96.0,
    }
}

/// The session a window shows: by its title, else by its size.
pub fn session_for<'a>(
    sessions: &'a [Session],
    popouts: &[Popout],
    by_size: &[(isize, String)],
    seen: &ShownWindow,
) -> Option<&'a Session> {
    claude_sessions::find(sessions, popouts, &seen.title).or_else(|| {
        let (_, id) = by_size.iter().find(|(hwnd, _)| *hwnd == seen.hwnd)?;
        sessions.iter().find(|s| &s.id == id)
    })
}

/// The chat (its `sessionId`) each of these Claude app windows shows,
/// matched as ClaudeDeck matches them, from the session files read last.
/// Starts a new read when the last one is old; never waits, only reads.
/// QuickJump uses it to keep a pinned chat even when its window closes.
pub fn chats_of(hwnds: &[isize]) -> Vec<(isize, String)> {
    if hwnds.is_empty() {
        return Vec::new();
    }
    claude_sessions::read_soon();
    let (sessions, popouts) = claude_sessions::sessions();
    if sessions.is_empty() {
        return Vec::new();
    }
    let shown: Vec<ShownWindow> = hwnds.iter().map(|&hwnd| shown_window(hwnd)).collect();
    let by_size = claude_sessions::match_by_size(&shown, &sessions, &popouts);
    shown
        .iter()
        .filter_map(|seen| {
            let session = session_for(&sessions, &popouts, &by_size, seen)?;
            Some((seen.hwnd, session.id.clone()))
        })
        .collect()
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
