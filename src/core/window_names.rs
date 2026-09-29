//! The names WinCraft gives other apps' windows. A renamed window's title
//! bar shows the name, but everything that keys on titles (LayoutKeeper's
//! saved windows, Explorer's folder lookup, Chrome window names) must keep
//! seeing the title the app itself set, or it would lose track of the window
//! the moment it is renamed. So every title read goes through one of two
//! functions:
//!
//! - `windows_list::window_text`: the app's own title, for keys and matching.
//! - `display_title`: the name when there is one, for what the user reads.
//!
//! WindowNamer is the only writer. It runs on the host thread; the palette
//! reads from the UI thread, hence the lock.

use std::collections::BTreeMap;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::HWND;

use crate::core::windows_list;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    /// The name the user gave the window.
    pub custom: String,
    /// The title the app last set itself.
    pub app_title: String,
}

static NAMES: Mutex<BTreeMap<isize, Names>> = Mutex::new(BTreeMap::new());

pub fn set(hwnd: isize, names: Names) {
    if let Ok(mut map) = NAMES.lock() {
        map.insert(hwnd, names);
    }
}

pub fn remove(hwnd: isize) {
    if let Ok(mut map) = NAMES.lock() {
        map.remove(&hwnd);
    }
}

pub fn get(hwnd: isize) -> Option<Names> {
    NAMES.lock().ok()?.get(&hwnd).cloned()
}

/// The app's own title of a renamed window; None for a window with no name.
pub fn app_title(hwnd: isize) -> Option<String> {
    get(hwnd).map(|names| names.app_title)
}

/// The user's name for a window, if it has one.
pub fn custom(hwnd: isize) -> Option<String> {
    get(hwnd).map(|names| names.custom)
}

/// What the user should read for a window: its name, else its title.
pub fn display_title(hwnd: HWND) -> String {
    custom(hwnd as isize).unwrap_or_else(|| windows_list::window_text(hwnd))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The map is shared by every test in the process, so each test uses
    // handles of its own.

    fn names(custom: &str, app_title: &str) -> Names {
        Names {
            custom: custom.to_string(),
            app_title: app_title.to_string(),
        }
    }

    #[test]
    fn a_renamed_window_shows_its_name_but_keeps_its_app_title_for_keys() {
        let hwnd = 0x5750_0001_isize;
        set(hwnd, names("Work notes", "Untitled - Notepad"));
        assert_eq!(display_title(hwnd as HWND), "Work notes");
        assert_eq!(
            windows_list::window_text(hwnd as HWND),
            "Untitled - Notepad"
        );
        assert_eq!(custom(hwnd).as_deref(), Some("Work notes"));
        assert_eq!(app_title(hwnd).as_deref(), Some("Untitled - Notepad"));
        remove(hwnd);
        assert_eq!(custom(hwnd), None);
        assert_eq!(app_title(hwnd), None);
    }

    #[test]
    fn a_window_without_a_name_reads_the_same_both_ways() {
        // No such window: both read the empty title Windows reports.
        let hwnd = 0x5750_0002_isize;
        assert_eq!(get(hwnd), None);
        assert_eq!(display_title(hwnd as HWND), "");
        assert_eq!(windows_list::window_text(hwnd as HWND), "");
    }

    #[test]
    fn a_new_app_title_replaces_the_old_one_and_the_name_stays() {
        let hwnd = 0x5750_0003_isize;
        set(hwnd, names("Mail", "Inbox - Gmail - Google Chrome"));
        set(hwnd, names("Mail", "Calendar - Google Chrome"));
        assert_eq!(display_title(hwnd as HWND), "Mail");
        assert_eq!(
            windows_list::window_text(hwnd as HWND),
            "Calendar - Google Chrome"
        );
        remove(hwnd);
    }
}
