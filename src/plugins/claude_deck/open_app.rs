//! What ClaudeDeck asks of the Claude app for a chat: open it, archive it,
//! rename it. A chat with a window of its own is brought forward; the rest
//! goes through the app's main window (see `core::claude_app`). When a step
//! fails, or an archive worked, WinCraft says so in a notice that names
//! ClaudeDeck. Rename asks for the new name in WinCraft's rename box, then
//! changes it in the app, so there is one name for the chat.

use std::sync::Mutex;

use crate::core::ui_bridge::{ArrangeAction, HostChannel, HostRequest, Prompt};
use crate::core::{claude_app, host, window_names};

use super::window::bring_forward;

/// The `host::post_to_plugins` ids a worker or the list sends: a notice
/// is waiting, or the rename box should open. Both are handled on the host
/// thread.
pub(crate) const NOTICE_POST_ID: usize = 0xC1A0_DEC1;
pub(crate) const RENAME_POST_ID: usize = 0xC1A0_DEC2;
/// The number of the rename box's question.
pub(crate) const PROMPT_RENAME_CHAT: u32 = 1;

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The chat being renamed: its title now, and its window if it has one.
static RENAMING: Mutex<Option<(String, Option<isize>)>> = Mutex::new(None);
/// Windows whose WindowNamer name is to be cleared by the list.
static CLEARS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Opens the chat: its window when it has one, else its button in the app.
pub(super) fn open_chat(window: Option<isize>, title: &str, to_host: &HostChannel) {
    if window.is_some() {
        bring_forward(window, to_host);
    } else {
        claude_app::open_chat(title, say);
    }
}

/// Archives the chat in the Claude app.
pub(super) fn archive(title: &str) {
    claude_app::archive_chat(title, say);
}

/// Starts a rename: the host thread opens the rename box.
pub(super) fn ask_rename(title: &str, window: Option<isize>) {
    if let Ok(mut renaming) = RENAMING.lock() {
        *renaming = Some((title.to_string(), window));
    }
    host::post_to_plugins(RENAME_POST_ID);
}

/// Opens the rename box, started with the chat's name now. Host thread.
pub(crate) fn ask_pending_rename(plugin: &str) {
    let Some((title, window)) = RENAMING.lock().ok().and_then(|renaming| renaming.clone()) else {
        return;
    };
    host::ask(Prompt {
        plugin: plugin.to_string(),
        id: PROMPT_RENAME_CHAT,
        target: window.unwrap_or(0),
        chip: "Rename".to_string(),
        placeholder: format!("Name for \u{201c}{title}\u{201d}"),
        text: title,
        action: "Rename to".to_string(),
        subtitle: "Claude chat".to_string(),
        ..Default::default()
    });
}

/// The new name was typed. Renames the chat in the app, and drops the
/// WindowNamer name of its window: the app sets the window title itself.
pub(crate) fn rename_answered(window: isize, name: &str) {
    let Some((title, _)) = RENAMING
        .lock()
        .ok()
        .and_then(|mut renaming| renaming.take())
    else {
        return;
    };
    let name = name.trim();
    if name.is_empty() || name == title {
        return;
    }
    claude_app::rename_chat(&title, name, say);
    if window != 0 && window_names::custom(window).is_some() {
        if let Ok(mut clears) = CLEARS.lock() {
            clears.push(window);
        }
    }
}

/// Clears the WindowNamer names that are waiting. Runs in the list's
/// refresh, which can reach WindowNamer through the host.
pub(super) fn send_pending_clears(to_host: &HostChannel) {
    let windows = CLEARS
        .lock()
        .map(|mut clears| std::mem::take(&mut *clears))
        .unwrap_or_default();
    for hwnd in windows {
        to_host.send(HostRequest::Arrange {
            plugin: super::ID.to_string(),
            action: ArrangeAction::Rename { hwnd, name: None },
        });
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

/// For tests: the rename that waits for the host thread.
#[cfg(test)]
pub(super) fn pending_rename() -> Option<(String, Option<isize>)> {
    RENAMING.lock().ok().and_then(|renaming| renaming.clone())
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
