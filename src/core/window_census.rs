//! The app windows the strip can show, as the plugin that keeps the window
//! order (LayoutKeeper) last saw them: each window's taskbar group and
//! rectangle. WindowDeck's hover strip counts an app's windows from it
//! without calling into that plugin. The plugin publishes a fresh list
//! whenever it looks at the windows, and clears it when it is switched off,
//! so with no such plugin every app counts no windows and the hover strip
//! never opens. Host thread only.

use std::cell::RefCell;

use crate::core::monitors::{self, Rect};
use crate::core::{taskbar, taskbar_scope};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CensusWindow {
    /// The taskbar group key: an AppUserModelID, or a program path.
    pub group: String,
    pub rect: Rect,
}

thread_local! {
    static CENSUS: RefCell<Vec<CensusWindow>> = const { RefCell::new(Vec::new()) };
}

pub fn publish(windows: Vec<CensusWindow>) {
    CENSUS.with(|cell| *cell.borrow_mut() = windows);
}

pub fn clear() {
    publish(Vec::new());
}

/// The windows of the app with taskbar ID `app`, on every desktop.
pub fn count_of(windows: &[CensusWindow], app: &str) -> usize {
    windows
        .iter()
        .filter(|window| taskbar::same_app(app, &window.group))
        .count()
}

/// How many windows the taskbar button at `button` lists for the app. A
/// secondary taskbar may list only the windows on its own monitor (see
/// `taskbar_scope`); the main one lists them all, and so does a taskbar
/// whose monitor cannot be found.
pub fn on_taskbar(app: &str, button: Rect) -> usize {
    CENSUS.with(|cell| {
        let windows = cell.borrow();
        let screens = monitors::screens();
        let Some(hovered) = monitors::screen_at(button, &screens) else {
            return count_of(&windows, app);
        };
        let own_only = taskbar_scope::MultiMonitor::read().shows_only_own(screens[hovered].primary);
        if !own_only {
            return count_of(&windows, app);
        }
        let primary = screens.iter().position(|s| s.primary).unwrap_or(0);
        let on: Vec<usize> = windows
            .iter()
            .filter(|window| taskbar::same_app(app, &window.group))
            .map(|window| monitors::screen_at(window.rect, &screens).unwrap_or(primary))
            .collect();
        taskbar_scope::count_shown(&on, hovered, true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(group: &str) -> CensusWindow {
        CensusWindow {
            group: group.to_string(),
            rect: [0, 0, 800, 600],
        }
    }

    #[test]
    fn an_app_counts_its_windows_on_every_desktop() {
        let windows = [
            window("Chrome.UserData.Profile3"),
            window("Chrome.UserData.Profile3"),
            window("Telegram.TelegramDesktop"),
        ];
        assert_eq!(count_of(&windows, "Chrome.UserData.Profile3"), 2);
        assert_eq!(count_of(&windows, "Appid: chrome.userdata.profile3"), 2);
        assert_eq!(count_of(&windows, "Telegram.TelegramDesktop"), 1);
        assert_eq!(count_of(&windows, "md.obsidian"), 0);
    }

    #[test]
    fn a_store_app_counts_all_its_windows() {
        let claude = r"d:\windowsapps\claude_3.6556.2.0_x64__pzs8sxrjxfjjc\app\claude.exe";
        let windows: Vec<CensusWindow> = (0..10).map(|_| window(claude)).collect();
        assert_eq!(count_of(&windows, "Claude_pzs8sxrjxfjjc!Claude"), 10);
        assert_eq!(count_of(&windows, "Appid: Claude_pzs8sxrjxfjjc!Claude"), 10);
    }

    #[test]
    fn with_nothing_published_no_app_has_windows() {
        clear();
        assert_eq!(
            on_taskbar("Chrome.UserData.Profile3", [0, 1040, 48, 1088]),
            0
        );
        publish(vec![window("Chrome.UserData.Profile3")]);
        assert_eq!(CENSUS.with(|cell| cell.borrow().len()), 1);
        clear();
        assert_eq!(CENSUS.with(|cell| cell.borrow().len()), 0);
    }
}
