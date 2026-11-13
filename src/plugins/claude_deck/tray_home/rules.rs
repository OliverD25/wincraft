//! The pure rules of "Live in the tray": the menu of ClaudeDeck's tray
//! icon, its tooltip and badge, when a click hides the window, and the
//! window style that keeps it out of the taskbar and Alt+Tab.

use windows_sys::Win32::UI::WindowsAndMessaging::{WS_EX_APPWINDOW, WS_EX_TOOLWINDOW};

use crate::core::tray::MenuItem;

/// The ids of the tray icon's menu entries, apart from the hotkey and
/// tray action ids (1 and 2) that `on_tray_action` also hears.
pub const MENU_SHOW: u32 = 11;
pub const MENU_HIDE: u32 = 12;
pub const MENU_MINI: u32 = 13;
pub const MENU_QUICK: u32 = 14;
pub const MENU_SETTINGS: u32 = 15;

pub const SHOW_LABEL: &str = "Show ClaudeDeck";
pub const HIDE_LABEL: &str = "Hide ClaudeDeck";
pub const MINI_LABEL: &str = "Mini mode";
pub const QUICK_LABEL: &str = "Quick settings\u{2026}";
pub const SETTINGS_LABEL: &str = "All settings\u{2026}";

/// The right-click menu: Show or Hide, Mini mode with its tick, a line,
/// Quick settings and All settings. Quit stays on WinCraft's own icon.
pub fn menu(shown: bool, mini: bool) -> Vec<MenuItem> {
    let entry = |id: u32, label: &str, checked: bool| MenuItem::Entry {
        id,
        label: label.to_string(),
        checked,
    };
    vec![
        if shown {
            entry(MENU_HIDE, HIDE_LABEL, false)
        } else {
            entry(MENU_SHOW, SHOW_LABEL, false)
        },
        entry(MENU_MINI, MINI_LABEL, mini),
        MenuItem::Separator,
        entry(MENU_QUICK, QUICK_LABEL, false),
        entry(MENU_SETTINGS, SETTINGS_LABEL, false),
    ]
}

/// The tooltip of ClaudeDeck's icon: "ClaudeDeck — 2 chats wait for you,
/// 4 new replies" while the count is on and there is something to say,
/// else the plain name.
pub fn tip(count_on: bool, counts: Option<(usize, usize)>) -> String {
    let (waiting, news) = counts.unwrap_or_default();
    super::super::select::tray_tip(count_on, waiting, news)
        .map(|text| text.replacen("WinCraft", "ClaudeDeck", 1))
        .unwrap_or_else(|| "ClaudeDeck".to_string())
}

/// The number on the badge: the chats that wait for you, while the count
/// is on.
pub fn badge(count_on: bool, counts: Option<(usize, usize)>) -> usize {
    match (count_on, counts) {
        (true, Some((waiting, _))) => waiting,
        _ => 0,
    }
}

/// Whether ClaudeDeck counts as in front for the show-or-hide toggle. A
/// click on the tray icon puts the taskbar in front first, so there the
/// window the list saw in front last counts.
pub fn in_front(
    deck: isize,
    front: isize,
    front_is_taskbar: bool,
    seen_last: Option<isize>,
    from_tray: bool,
) -> bool {
    deck != 0 && (front == deck || (from_tray && front_is_taskbar && seen_last == Some(deck)))
}

/// The same rule as the hotkey: a shown window in front hides (or closes
/// with "Live in the tray" off); anything else brings it forward. A
/// command that only shows it never hides it.
pub fn hides(shown: bool, in_front: bool, keep_open: bool) -> bool {
    shown && in_front && !keep_open
}

/// The window's extended style: a tool window has no taskbar button and no
/// Alt+Tab entry, and `WS_EX_APPWINDOW` would put both back.
pub fn ex_style(current: u32, live_in_tray: bool) -> u32 {
    if live_in_tray {
        (current | WS_EX_TOOLWINDOW) & !WS_EX_APPWINDOW
    } else {
        (current & !WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW
    }
}
