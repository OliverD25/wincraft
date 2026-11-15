//! How a pin keeps up with its window while the window is open, so it can
//! find the window again after a close: it learns the Claude chat the
//! window shows, and its remembered title follows the window's title. Also
//! what a jump does when the window is closed. Nothing here calls Windows.

use crate::core::claude_app;
use crate::core::window_identity::{bare_title, exe_key, title_contains};

use super::matching::{candidates, find_how, How, Live};
use super::pins::Pin;

/// Titles a window has before it shows anything; a pin never follows one.
const PLACEHOLDERS: [&str; 3] = ["untitled", "new tab", "new window"];

/// The link that opens the pin's Claude chat, when it remembers one the
/// Claude app takes.
pub fn chat_link(pin: &Pin) -> Option<String> {
    pin.claude_session
        .as_deref()
        .and_then(claude_app::chat_link)
}

/// What a jump does when the pin has no open window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Closed {
    /// Open the chat (its sessionId) in the Claude app by its link.
    OpenChat(String),
    /// Start the program.
    Start,
    /// Say the window is not open.
    Tell,
}

pub fn when_closed(pin: &Pin) -> Closed {
    match (chat_link(pin), &pin.claude_session) {
        (Some(_), Some(id)) => Closed::OpenChat(id.clone()),
        _ if pin.start => Closed::Start,
        _ => Closed::Tell,
    }
}

/// An empty title, a placeholder ("Untitled", "New Tab") or the app's own
/// name, which says nothing about which window it is.
pub fn placeholder(title: &str, app_name: &str) -> bool {
    let title = title.trim().to_lowercase();
    title.is_empty() || PLACEHOLDERS.contains(&title.as_str()) || title == app_name.to_lowercase()
}

/// Ties the pin to its window and keeps it in step with that window: the
/// handle, the chat it shows, and its title. True when something worth
/// saving changed (the chat or the title).
///
/// Only a sure match teaches the pin anything: its chat, the handle it had
/// while that was sure, or a title that only one window holds. The "app's
/// only window" rule never does, as it can be another window.
pub fn follow(pin: &mut Pin, live: &[Live]) -> bool {
    let Some((window, how)) = find_how(pin, live) else {
        pin.hwnd = None;
        pin.sure = false;
        return false;
    };
    let sure = match how {
        How::Chat => true,
        How::Handle => pin.sure,
        How::Title => {
            !pin.title.trim().is_empty()
                && title_contains(&window.title, &pin.title)
                && candidates(pin, live).len() == 1
        }
        How::Only => false,
    };
    pin.hwnd = Some(window.hwnd);
    pin.sure = sure;
    if !sure {
        return false;
    }
    let mut changed = false;
    if pin.claude_session.is_none() {
        // The window still shows what the pin was made for: a Claude main
        // window that moved on to another chat teaches nothing.
        let same = !pin.title.trim().is_empty() && title_contains(&window.title, &pin.title);
        if let Some(chat) = window.chat.as_ref().filter(|_| same) {
            pin.claude_session = Some(chat.clone());
            changed = true;
        }
    }
    // A chat window's title follows only by its chat: the handle of a
    // Claude main window may show another chat by now.
    let by_chat = how == How::Chat || (pin.claude_session.is_none() && window.chat.is_none());
    let title = bare_title(&window.title);
    if pin.title_auto && by_chat && !placeholder(title, &pin.app_name) && title != pin.title {
        pin.title = title.to_string();
        changed = true;
    }
    changed
}

/// The pin after the details panel's Done. `before` is the pin as the panel
/// opened, `old` the pin now (its title may have followed the window since),
/// `draft` what the panel sends. A title the user typed stops following,
/// and leaves the chat behind, so what was typed is what finds the window.
pub fn edited(old: &Pin, before: &Pin, draft: Pin) -> Pin {
    let typed = draft.title != before.title;
    let other_app = exe_key(&draft.exe) != exe_key(&before.exe) || draft.app_id != before.app_id;
    Pin {
        title: if typed {
            draft.title.clone()
        } else {
            old.title.clone()
        },
        title_auto: old.title_auto && !typed && !other_app,
        claude_session: if typed || other_app {
            None
        } else {
            old.claude_session.clone()
        },
        hwnd: None,
        sure: false,
        ..draft
    }
}

#[cfg(test)]
mod tests;
