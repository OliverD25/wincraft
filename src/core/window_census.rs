//! The app windows the strip can show, as the plugin that keeps the window
//! order (LayoutKeeper) last saw them: each window's taskbar group and
//! rectangle. WindowDeck's hover strip counts an app's windows from it
//! without calling into that plugin. The plugin publishes a fresh list
//! whenever it looks at the windows, and clears it when it is switched off.
//! With no such plugin the windows open now are counted instead, every app's
//! but WinCraft's and Windows' helpers. Host thread only.

use std::cell::RefCell;

use crate::core::live_windows;
use crate::core::monitors::{self, Rect};
use crate::core::virtual_desktops::Reader;
use crate::core::{taskbar, taskbar_scope};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CensusWindow {
    /// The taskbar group key: an AppUserModelID, or a program path.
    pub group: String,
    pub rect: Rect,
}

thread_local! {
    /// None while no plugin keeps a window order.
    static CENSUS: RefCell<Option<Vec<CensusWindow>>> = const { RefCell::new(None) };
}

pub fn publish(windows: Vec<CensusWindow>) {
    CENSUS.with(|cell| *cell.borrow_mut() = Some(windows));
}

/// The plugin that published is off; from now on the open windows count.
pub fn clear() {
    CENSUS.with(|cell| *cell.borrow_mut() = None);
}

/// The published list, or the windows open now when nobody publishes.
fn windows() -> Vec<CensusWindow> {
    CENSUS
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(open_now)
}

fn open_now() -> Vec<CensusWindow> {
    let reader = Reader::new().ok();
    of_live(&live_windows::on_desktops(
        &live_windows::is_app,
        reader.as_ref(),
    ))
}

/// The census of windows listed by `live_windows`.
pub fn of_live(live: &[live_windows::LiveWindow]) -> Vec<CensusWindow> {
    live.iter()
        .map(|window| CensusWindow {
            group: window.group.clone(),
            rect: window.identity.rect,
        })
        .collect()
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
    let windows = windows();
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
    fn a_published_list_is_used_until_it_is_cleared() {
        let app = "Chrome.UserData.Profile3.CensusTest";
        publish(vec![window(app), window(app)]);
        assert_eq!(windows().len(), 2);
        assert_eq!(on_taskbar(app, [0, 1040, 48, 1088]), 2);
        publish(Vec::new());
        assert!(
            windows().is_empty(),
            "an empty list is a list, not 'nobody'"
        );
        assert_eq!(on_taskbar(app, [0, 1040, 48, 1088]), 0);
        clear();
        assert!(CENSUS.with(|cell| cell.borrow().is_none()));
        // Nobody publishes: the open windows count, and no real app has
        // this made-up ID.
        assert_eq!(on_taskbar(app, [0, 1040, 48, 1088]), 0);
    }
}
