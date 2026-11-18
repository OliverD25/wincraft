//! The link that makes the Claude app's main window show a chat of the Claude
//! app on this PC: `claude://code/continue?session=<id>`. The id is the
//! session file's `local_` id. The app opens an empty new chat for any other
//! id, so only a `local_` id gets a link.

use std::sync::Mutex;
use std::time::Duration;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::{app_windows, poll, HOME_TITLE};
use crate::core::{instance, wide, window_actions, windows_list};

const PREFIX: &str = "local_";
/// The most characters the app accepts after the prefix.
const MOST_AFTER_PREFIX: usize = 64;
/// How long the home window gets to show before it is brought forward.
const FORWARD_WAIT: Duration = Duration::from_secs(3);
const FORWARD_STEP: Duration = Duration::from_millis(150);

/// The chat a link last showed in the main window, and that window once it
/// was found. The main window's title stays "Claude Home" whatever chat it
/// shows, so this is the only way to know which chat it holds; a chat the
/// user picks inside the app later is not seen.
static HOME_CHAT: Mutex<Option<(isize, String)>> = Mutex::new(None);

/// The main window and the chat a link last showed in it. The window is 0
/// until it was found.
pub fn home_chat() -> Option<(isize, String)> {
    HOME_CHAT.lock().ok().and_then(|home| home.clone())
}

/// Notes the chat a link shows; a window of 0 keeps the one found before,
/// which still is the main window when the app stayed open.
fn note_home(hwnd: isize, id: &str) {
    if let Ok(mut home) = HOME_CHAT.lock() {
        let hwnd = match home.as_ref() {
            Some((before, _)) if hwnd == 0 => *before,
            _ => hwnd,
        };
        *home = Some((hwnd, id.to_string()));
    }
}

/// The link for a desktop chat id, or `None` when the app would not take it
/// (it accepts `local_` and 1 to 64 letters, digits or dashes).
pub fn chat_link(id: &str) -> Option<String> {
    let rest = id.strip_prefix(PREFIX)?;
    let fine = !rest.is_empty()
        && rest.len() <= MOST_AFTER_PREFIX
        && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    fine.then(|| format!("claude://code/continue?session={id}"))
}

/// Makes the app's main window show this chat. Returns false when the id has
/// no link or Windows could not open it. A test copy of WinCraft only reads,
/// so it opens nothing; unit tests start nothing.
pub fn show_chat(id: &str) -> bool {
    show_chat_then(id, || {})
}

/// `show_chat`, and `finished` runs once the home window was brought
/// forward (or its wait is over), or at once when nothing was opened.
pub fn show_chat_then(id: &str, finished: impl FnOnce() + Send + 'static) -> bool {
    let Some(link) = chat_link(id) else {
        log::warn!("ClaudeDeck: \"{id}\" is not an id the Claude app can open");
        finished();
        return false;
    };
    if cfg!(test) {
        finished();
        return true;
    }
    if instance::is_test() {
        log::info!("read-only test instance: opening {link} skipped");
        finished();
        return true;
    }
    log::info!("ClaudeDeck: showing a chat in the Claude app with {link}");
    let verb = wide("open");
    let target = wide(&link);
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as usize <= 32 {
        log::warn!(
            "ClaudeDeck: Windows could not open {link} (code {})",
            result as usize
        );
        finished();
        return false;
    }
    note_home(0, id);
    let id = id.to_string();
    std::thread::spawn(move || {
        bring_home_forward(&id);
        finished();
    });
    true
}

/// Brings the window titled "Claude Home" forward once it shows, and notes
/// it as the window that shows chat `id`.
fn bring_home_forward(id: &str) {
    let home = poll(FORWARD_WAIT, FORWARD_STEP, || {
        app_windows()
            .into_iter()
            .find(|hwnd| windows_list::window_text(*hwnd) == HOME_TITLE)
    });
    if let Some(hwnd) = home {
        note_home(hwnd as isize, id);
        window_actions::activate(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_id_gets_a_link() {
        assert_eq!(
            chat_link("local_a1b2-C3").as_deref(),
            Some("claude://code/continue?session=local_a1b2-C3")
        );
    }

    #[test]
    fn other_ids_get_none() {
        assert_eq!(chat_link(""), None);
        assert_eq!(chat_link("local_"), None);
        assert_eq!(chat_link("3f2a9c1e-7b64-4d0a-9e51-0c8d2b7a6f10"), None);
        for bad in ["local_a&b", "local_a b", "local_a/b", "local_a_b"] {
            assert_eq!(chat_link(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_new_link_keeps_the_main_window_found_before() {
        note_home(0, "local_a");
        note_home(42, "local_a");
        note_home(0, "local_b");
        assert_eq!(home_chat(), Some((42, "local_b".to_string())));
        note_home(43, "local_c");
        assert_eq!(home_chat(), Some((43, "local_c".to_string())));
    }

    #[test]
    fn the_length_limit_is_64_after_the_prefix() {
        assert!(chat_link(&format!("local_{}", "a".repeat(64))).is_some());
        assert!(chat_link(&format!("local_{}", "a".repeat(65))).is_none());
    }
}
