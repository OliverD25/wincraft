//! The link that makes the Claude app's main window show a chat of the Claude
//! app on this PC: `claude://code/continue?session=<id>`. The id is the
//! session file's `local_` id. The app opens an empty new chat for any other
//! id, so only a `local_` id gets a link.

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
    let Some(link) = chat_link(id) else {
        log::warn!("ClaudeDeck: \"{id}\" is not an id the Claude app can open");
        return false;
    };
    if cfg!(test) {
        return true;
    }
    if instance::is_test() {
        log::info!("read-only test instance: opening {link} skipped");
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
        return false;
    }
    std::thread::spawn(bring_home_forward);
    true
}

/// Brings the window titled "Claude Home" forward once it shows.
fn bring_home_forward() {
    let home = poll(FORWARD_WAIT, FORWARD_STEP, || {
        app_windows()
            .into_iter()
            .find(|hwnd| windows_list::window_text(*hwnd) == HOME_TITLE)
    });
    if let Some(hwnd) = home {
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
    fn the_length_limit_is_64_after_the_prefix() {
        assert!(chat_link(&format!("local_{}", "a".repeat(64))).is_some());
        assert!(chat_link(&format!("local_{}", "a".repeat(65))).is_none());
    }
}
