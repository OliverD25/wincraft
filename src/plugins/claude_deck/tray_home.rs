//! "Live in the tray": ClaudeDeck gets a tray icon of its own (through the
//! host's `set_own_tray`) instead of a taskbar button, its × hides the
//! window instead of closing it, and while the window is not shown a timer
//! on the host thread keeps reading the chats and the servers, so the
//! icon's count, the news and the job history stay current. The timer
//! does only that: it never paints.

pub mod rules;

use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, IsWindowVisible, KillTimer, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOW, SW_SHOWNA, WM_TIMER,
};

use crate::core::plugin_tray::OwnTray;
use crate::core::traits::WinCraftPlugin;
use crate::core::tray::MenuItem;
use crate::core::{claude_jobs, claude_sessions, claude_windows, host, windows_list};

use super::reading;
use super::servers::Deck;
use super::taskbar;
use super::window::{self, DeckUi};
use super::{ClaudeDeck, Shared, ACTION_MINI, COMMAND_QUICK, ID};
use rules::*;

/// The host-thread timer that reads while the window is not shown.
const TIMER_ID: usize = 0xC1A0_DEC1;

/// What the window and the plugin share about living in the tray.
#[derive(Default)]
pub struct Home {
    /// The window is hidden in the tray: kept, but not shown.
    pub hidden: bool,
    /// The next `open` comes from a click on the tray icon.
    pub from_tray: bool,
    /// The next `open` hides the window (the menu's Hide ClaudeDeck).
    pub hide_wanted: bool,
    /// The next `open` closes the hidden window for good: the setting
    /// was switched off while it was hidden.
    pub close_wanted: bool,
    /// The chats that wait and the unseen replies, as read last; None
    /// when nothing reads.
    pub counts: Option<(usize, usize)>,
    /// The window and the style it was given last.
    styled: Option<(isize, bool)>,
}

/// The plugin's side, on the host thread.
#[derive(Default)]
pub struct PluginSide {
    /// The window's state, which the timer reads into while it is hidden.
    pub state: Arc<Mutex<DeckUi>>,
    hwnd: isize,
    running: bool,
    timer: bool,
    tip_set: Option<String>,
    own_set: Option<OwnTray>,
}

/// Whether the window shows now.
fn shown(shared: &Shared) -> bool {
    shared.open && !shared.home.hidden
}

impl ClaudeDeck {
    pub(super) fn start_tray(&mut self, hwnd: HWND) {
        self.home.hwnd = hwnd as isize;
        self.home.running = true;
        self.tray_settings_changed();
    }

    pub(super) fn stop_tray(&mut self) {
        self.home.running = false;
        self.set_timer(false);
        if let Ok(mut shared) = self.shared.lock() {
            shared.home.counts = None;
        }
        self.publish_tray();
    }

    /// The settings changed: start or stop the timer, and close a hidden
    /// window for good when the setting went off.
    pub(super) fn tray_settings_changed(&mut self) {
        if !self.home.running {
            return;
        }
        let (live, hidden, open) = match self.shared.lock() {
            Ok(shared) => (shared.settings.tray_home, shared.home.hidden, shared.open),
            Err(_) => return,
        };
        self.set_timer(live);
        if !live && (hidden || !open) {
            // Nothing shows now, so nothing is counted any more.
            if let Ok(mut shared) = self.shared.lock() {
                shared.home.counts = None;
                shared.home.close_wanted = hidden;
            }
        }
        if !live && hidden {
            host::show_window(ID, window::WINDOW_ID);
        }
        if !live && !open {
            self.rest();
        }
        self.publish_tray();
    }

    fn set_timer(&mut self, on: bool) {
        if on == self.home.timer || self.home.hwnd == 0 {
            return;
        }
        self.home.timer = on;
        let every = claude_sessions::READ_EVERY.as_millis() as u32;
        unsafe {
            if on {
                SetTimer(self.home.hwnd as HWND, TIMER_ID, every, None);
            } else {
                KillTimer(self.home.hwnd as HWND, TIMER_ID);
            }
        }
    }

    /// Nothing shows and nothing needs the reading any more: the servers
    /// stop and the lists of open chats are emptied.
    fn rest(&mut self) {
        if let Ok(mut state) = self.home.state.lock() {
            state.servers = Deck::default();
        }
        claude_windows::set_wanted(false);
        claude_jobs::want_for_deck(Vec::new());
        claude_windows::publish(Vec::new());
    }

    pub(super) fn on_tray_timer(&mut self, msg: u32, wparam: usize) {
        if msg == WM_TIMER && wparam == TIMER_ID {
            read_while_hidden(&self.shared, &self.home.state);
            self.publish_tray();
        }
    }

    /// Shows what the window or the timer counted: on ClaudeDeck's own
    /// icon while it lives in the tray, else in WinCraft's tooltip as
    /// before.
    pub(super) fn publish_tray(&mut self) {
        let (live, count_on, counts) = match self.shared.lock() {
            Ok(shared) => (
                shared.settings.tray_home && self.home.running,
                shared.settings.tray_count,
                shared.home.counts,
            ),
            Err(_) => return,
        };
        let wincraft_tip = match (live, counts) {
            (false, Some((waiting, news))) => super::select::tray_tip(count_on, waiting, news),
            _ => None,
        };
        if wincraft_tip != self.home.tip_set {
            self.home.tip_set = wincraft_tip.clone();
            host::set_tray_tip(wincraft_tip);
        }
        let own = live.then(|| OwnTray {
            icon: taskbar::window_icon(),
            tip: tip(count_on, counts),
            badge: badge(count_on, counts),
        });
        if own != self.home.own_set {
            self.home.own_set = own.clone();
            host::set_own_tray(ID, own);
        }
    }

    pub(super) fn tray_menu(&self) -> Vec<MenuItem> {
        match self.shared.lock() {
            Ok(shared) => menu(shown(&shared), shared.settings.mini),
            Err(_) => Vec::new(),
        }
    }

    /// A left click: the same toggle as the hotkey.
    pub(super) fn tray_click(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.home.from_tray = true;
        }
        self.open();
    }

    pub(super) fn tray_pick(&mut self, id: u32) {
        match id {
            MENU_SHOW => {
                if let Ok(mut shared) = self.shared.lock() {
                    shared.keep_open = true;
                }
                self.open();
            }
            MENU_HIDE => {
                if let Ok(mut shared) = self.shared.lock() {
                    shared.home.hide_wanted = true;
                }
                self.open();
            }
            MENU_MINI => self.on_hotkey(ACTION_MINI),
            MENU_QUICK => self.on_palette_command(COMMAND_QUICK),
            MENU_SETTINGS => host::show_plugin_page(ID),
            _ => {}
        }
    }
}

/// The timer's work: while ClaudeDeck lives in the tray and its window is
/// not shown, read the chats and the servers as the window would, and
/// count what waits. The window, when it shows, does this itself.
fn read_while_hidden(shared: &Arc<Mutex<Shared>>, state: &Arc<Mutex<DeckUi>>) {
    let settings = match shared.lock() {
        Ok(shared) if shared.settings.tray_home && !shown(&shared) => shared.settings.clone(),
        _ => return,
    };
    let counts = {
        let Ok(mut state) = state.lock() else {
            return;
        };
        claude_windows::set_wanted(true);
        host::read_claude_chats();
        state.servers.tick(&settings.servers, Arc::new(|| {}));
        let now = claude_sessions::now_ms();
        reading::read(&mut state);
        window::build(&mut state, &settings, now);
        (state.cache.waiting, state.news.count)
    };
    if let Ok(mut shared) = shared.lock() {
        shared.home.counts = Some(counts);
    }
}

/// What `open` does first, on the UI thread: a hide or a close the plugin
/// asked for. True when that was all.
pub(super) fn open_asked(shared: &mut Shared, state: &mut DeckUi) -> bool {
    if std::mem::take(&mut shared.home.close_wanted) {
        if shared.home.hidden {
            shared.home.hidden = false;
            shared.open = false;
            window::forget(state);
        }
        return true;
    }
    if std::mem::take(&mut shared.home.hide_wanted) {
        if shown(shared) {
            hide(shared, state);
        }
        return true;
    }
    false
}

/// Whether the window counts as in front for the toggle.
pub(super) fn in_front_now(shared: &mut Shared, state: &DeckUi) -> bool {
    let from_tray = std::mem::take(&mut shared.home.from_tray);
    if !shown(shared) {
        return false;
    }
    let front = claude_windows::foreground();
    let taskbar = matches!(
        windows_list::class_name(front as HWND).as_str(),
        "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
            | "TopLevelWindowForOverflowXamlIsland"
    );
    in_front(state.window, front, taskbar, state.front_seen, from_tray)
}

/// Hides the window to the tray: its place, tab and selection stay.
pub(super) fn hide(shared: &mut Shared, state: &mut DeckUi) {
    state.closed = false;
    if state.window == 0 {
        return;
    }
    window::keep_place(state);
    window::commit(state);
    unsafe { ShowWindow(state.window as HWND, SW_HIDE) };
    shared.home.hidden = true;
    log::info!("ClaudeDeck: hidden in the tray");
}

/// Shows the hidden window again where it was. True when it was hidden.
pub(super) fn unhide(shared: &mut Shared, state: &mut DeckUi) -> bool {
    if !std::mem::take(&mut shared.home.hidden) {
        return false;
    }
    if state.window != 0 {
        unsafe { ShowWindow(state.window as HWND, SW_SHOW) };
    }
    true
}

/// The × or the minimise button with the setting on: hide instead.
pub(super) fn hides_instead(shared: &mut Shared, state: &mut DeckUi) -> bool {
    if !shared.settings.tray_home || state.window == 0 {
        return false;
    }
    hide(shared, state);
    true
}

/// Gives the window the style the setting asks for, once per window and
/// setting: a tool window with no taskbar button while it lives in the
/// tray, else its own taskbar button as before. A visible window is
/// hidden and shown once so the taskbar sees the change.
pub(super) fn keep_style(shared: &mut Shared, hwnd: isize) {
    let live = shared.settings.tray_home;
    if hwnd == 0 || shared.home.styled == Some((hwnd, live)) {
        return;
    }
    let first = shared.home.styled.is_none_or(|(old, _)| old != hwnd);
    shared.home.styled = Some((hwnd, live));
    if first && !live {
        taskbar::own_button(hwnd);
        return;
    }
    let handle = hwnd as HWND;
    let visible = unsafe { IsWindowVisible(handle) } != 0;
    let style = unsafe { GetWindowLongPtrW(handle, GWL_EXSTYLE) } as u32;
    unsafe {
        if visible {
            ShowWindow(handle, SW_HIDE);
        }
        SetWindowLongPtrW(handle, GWL_EXSTYLE, ex_style(style, live) as isize);
        SetWindowPos(
            handle,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    if live {
        taskbar::release(hwnd);
    } else {
        taskbar::own_button(hwnd);
    }
    if visible {
        unsafe { ShowWindow(handle, SW_SHOWNA) };
    }
}

#[cfg(test)]
mod tests;
