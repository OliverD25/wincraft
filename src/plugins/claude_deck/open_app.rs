//! Opening a chat from the list. A chat with a window of its own is brought
//! forward. A chat that is only in the Claude app's main window is pressed
//! in the app's sidebar (see `core::claude_app`); when that fails, WinCraft
//! says so in a notice that names ClaudeDeck.

use std::sync::Mutex;

use crate::core::{claude_app, host};

use super::window::bring_forward;
use crate::core::ui_bridge::HostChannel;

/// The `host::post_to_plugins` id a worker thread sends when a notice is
/// waiting. The notice itself is shown on the host thread.
pub(crate) const NOTICE_POST_ID: usize = 0xC1A0_DEC1;

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Opens the chat: its window when it has one, else its button in the app.
pub(super) fn open_chat(window: Option<isize>, title: &str, to_host: &HostChannel) {
    if window.is_some() {
        bring_forward(window, to_host);
    } else {
        claude_app::open_chat(title, say);
    }
}

fn say(text: String) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.push(text);
    }
    host::post_to_plugins(NOTICE_POST_ID);
}

/// Shows the waiting notices. Runs on the host thread.
pub(crate) fn show_pending() {
    let texts = PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default();
    for text in texts {
        host::notify("ClaudeDeck", &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_notice_is_handed_over_once() {
        PENDING.lock().unwrap().push("one".to_string());
        let first = PENDING.lock().map(|mut p| std::mem::take(&mut *p)).unwrap();
        assert_eq!(first, ["one"]);
        assert!(PENDING.lock().unwrap().is_empty());
    }
}
