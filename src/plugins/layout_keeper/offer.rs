//! LayoutKeeper's short notices in the shared notice (`core::notice`): the
//! offer to put an app's windows back after it restarted, and the word
//! after Save layout now. One notice at a time; a new one takes the old
//! one's place. Nothing moves until Put back is clicked (Principle 1).
//!
//! The notice reports the mouse to a hidden window of LayoutKeeper's own.
//! That window only queues what it heard and wakes the plugin through
//! `host::post_to_plugins`, so the work runs on the host's turn.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, HWND_MESSAGE, WNDCLASSW,
};

use super::*;
use crate::core::notice::{self as shared, Clock, Glyph, Message, Notice};

/// How long the offer stays, not counting the time the mouse rests on it.
pub(super) const OFFER_MS: u64 = 15_000;
/// Wakes the plugin when the notice heard the mouse.
pub(super) const OFFER_POST_ID: usize = 0x4C4B_0003;
const NAME: &str = "LayoutKeeper";
const LISTEN_CLASS: &str = "WinCraftLayoutKeeperListen";
const NOTICE_CLASS: &str = "WinCraftLayoutKeeperNotice";
const PUT_BACK: &str = "Put back";
const DEFAULT_RESTORE_KEY: &str = "Win+Alt+L";

thread_local! {
    static HEARD: RefCell<Vec<(u32, LPARAM)>> = const { RefCell::new(Vec::new()) };
}

/// The notice on screen and what its button does.
pub(super) struct Shown {
    /// What it says, for the tests.
    #[cfg(test)]
    pub(super) message: Message,
    notice: Option<Notice>,
    clock: Clock,
    /// The group Put back restores; None for a notice without the button.
    pub(super) put_back: Option<String>,
}

#[derive(Default)]
pub(super) struct Offers {
    pub(super) shown: Option<Shown>,
    listener: Option<HWND>,
}

/// What happened to the notice since the last look.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Nothing,
    /// Put back was clicked for this group.
    PutBack(String),
    /// The offer for this group went away unanswered.
    Expired(String),
}

impl Offers {
    /// Shows `message`, in place of any notice shown before. `put_back`
    /// names the group the button restores.
    pub(super) fn show(&mut self, message: Message, put_back: Option<String>, length: u64) {
        self.close();
        // A unit test never puts a window on the screen.
        let notice = if cfg!(test) {
            None
        } else {
            self.listener()
                .and_then(|target| Notice::show(&message, &instance::name(NOTICE_CLASS), target))
        };
        self.shown = Some(Shown {
            #[cfg(test)]
            message,
            notice,
            clock: Clock::lasting(now_ms(), length),
            put_back,
        });
    }

    pub(super) fn close(&mut self) {
        self.shown = None;
    }

    /// The group an offer on screen is for.
    pub(super) fn offered(&self) -> Option<&str> {
        self.shown.as_ref()?.put_back.as_deref()
    }

    /// Handles what the notice heard, then lets an expired notice go.
    pub(super) fn step(&mut self) -> Outcome {
        let now = now_ms();
        for (msg, lparam) in HEARD.with(|heard| std::mem::take(&mut *heard.borrow_mut())) {
            let Some(shown) = self.shown.as_mut() else {
                continue;
            };
            match msg {
                shared::WM_APP_NOTICE_HOVER => shown.clock.hover(now, lparam != 0),
                shared::WM_APP_NOTICE_CLICK => {
                    let point = shared::click_point(lparam);
                    let on_button = shown
                        .notice
                        .as_ref()
                        .is_some_and(|notice| notice.on_button(point.x, point.y));
                    let group = shown.put_back.clone();
                    self.close();
                    if let (true, Some(group)) = (on_button, group) {
                        return Outcome::PutBack(group);
                    }
                }
                _ => {}
            }
        }
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.clock.expired(now))
        {
            let group = self.shown.take().and_then(|shown| shown.put_back);
            if let Some(group) = group {
                return Outcome::Expired(group);
            }
        }
        Outcome::Nothing
    }

    fn listener(&mut self) -> Option<HWND> {
        if self.listener.is_none() {
            match create_listener() {
                Ok(hwnd) => self.listener = Some(hwnd),
                Err(reason) => {
                    log::warn!("the LayoutKeeper notice cannot hear the mouse: {reason}")
                }
            }
        }
        self.listener
    }

    pub(super) fn teardown(&mut self) {
        self.close();
        if let Some(hwnd) = self.listener.take() {
            unsafe { DestroyWindow(hwnd) };
        }
        HEARD.with(|heard| heard.borrow_mut().clear());
    }
}

fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

unsafe extern "system" fn listen_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        shared::WM_APP_NOTICE_HOVER | shared::WM_APP_NOTICE_CLICK => {
            let value = if msg == shared::WM_APP_NOTICE_HOVER {
                wparam as LPARAM
            } else {
                lparam
            };
            HEARD.with(|heard| heard.borrow_mut().push((msg, value)));
            host::post_to_plugins(OFFER_POST_ID);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn create_listener() -> Result<HWND, String> {
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let name = wide(&instance::name(LISTEN_CLASS));
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(listen_proc);
    class.hInstance = hinstance;
    class.lpszClassName = name.as_ptr();
    // Fails harmlessly with "class already exists" after a restart.
    unsafe { RegisterClassW(&class) };
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            name.as_ptr(),
            wide(NAME).as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return Err("could not create its window".to_string());
    }
    Ok(hwnd)
}

impl LayoutKeeper {
    /// A group's restart has settled: offer Put back when it may be.
    pub(super) fn consider_offer(&mut self, group: &str, live: &[windows::LiveWindow]) {
        let mine: Vec<&windows::LiveWindow> =
            live.iter().filter(|window| window.group == group).collect();
        let watched = mine
            .iter()
            .all(|window| self.watch.covers(&window.identity.exe));
        let restoring = self.restore.is_running() || self.finishing.is_some();
        if !may_offer(self.offer_on, restoring, watched, mine.len()) {
            return;
        }
        let key = if cfg!(test) {
            None
        } else {
            stored_restore_key()
        };
        let label = self.label_of(group);
        let message = offer_message(&label, mine.len(), key.as_deref());
        log::info!(
            "offered to put the {} windows of \"{label}\" back after its restart",
            mine.len()
        );
        self.offers.show(message, Some(group.to_string()), OFFER_MS);
    }

    /// The mouse on the notice, Put back clicked, or the notice due to go.
    pub(super) fn offer_step(&mut self) {
        match self.offers.step() {
            Outcome::Nothing => {}
            Outcome::PutBack(group) => self.put_back(&group),
            Outcome::Expired(group) => log::info!(
                "the offer to put \"{}\" back went unanswered",
                self.label_of(&group)
            ),
        }
    }

    /// Put back was clicked: restores that group alone, from the layout
    /// saved before its restart.
    pub(super) fn put_back(&mut self, group: &str) {
        log::info!(
            "Put back clicked: restoring \"{}\" only",
            self.label_of(group)
        );
        self.start_restore_of(false, Some(group));
    }
}

/// Whether a group whose restart just settled gets the offer: the setting
/// is on, no restore is under way (the restore after sign-in included),
/// its app is watched (not on the Never touch list) and it has at least
/// two windows.
pub(super) fn may_offer(on: bool, restoring: bool, watched: bool, windows: usize) -> bool {
    on && !restoring && watched && windows >= 2
}

/// The offer for an app with `windows` windows; `key` is the Restore
/// layout hotkey as stored in the plugin's file, None when nothing is
/// stored (the default).
pub(super) fn offer_message(label: &str, windows: usize, key: Option<&str>) -> Message {
    let text = format!("{label} restarted. Put its {windows} windows back in order?");
    let mut message = Message::new(NAME, Glyph::Monitor, text).with_button(PUT_BACK);
    if let Some(line) = key_line(key) {
        message = message.detail(line);
    }
    message
}

/// "Or press Win+Alt+L", with the key the user set; nothing when Restore
/// layout has no hotkey.
pub(super) fn key_line(stored: Option<&str>) -> Option<String> {
    let text = stored.unwrap_or(DEFAULT_RESTORE_KEY).trim();
    let hotkey = crate::core::hotkeys::parse(text).ok()?;
    Some(format!("Or press {}", crate::core::hotkeys::format(hotkey)))
}

/// The Restore layout hotkey from LayoutKeeper's file, when one is stored.
pub(super) fn stored_restore_key() -> Option<String> {
    let path = crate::core::config::plugin_path("layout_keeper");
    let text = std::fs::read_to_string(path).ok()?;
    let file: serde_json::Value =
        serde_json::from_str(crate::core::config::strip_bom(&text)).ok()?;
    file.get("hotkeys")?
        .get("restore_now")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offer_names_the_app_its_windows_and_the_key_the_user_set() {
        let message = offer_message("Chrome", 5, None);
        assert_eq!(message.app, "LayoutKeeper");
        assert_eq!(
            message.text,
            "Chrome restarted. Put its 5 windows back in order?"
        );
        assert_eq!(message.detail.as_deref(), Some("Or press Win+Alt+L"));
        assert_eq!(message.button, Some("Put back"));
        let rebound = offer_message("Telegram", 2, Some("win+ctrl+r"));
        assert_eq!(rebound.detail.as_deref(), Some("Or press Win+Ctrl+R"));
        assert_eq!(offer_message("Chrome", 2, Some("")).detail, None);
    }

    #[test]
    fn only_a_watched_app_with_two_windows_and_no_restore_running_is_offered() {
        assert!(may_offer(true, false, true, 2));
        assert!(!may_offer(false, false, true, 5), "the setting is off");
        assert!(!may_offer(true, true, true, 5), "a restore is running");
        assert!(!may_offer(true, false, false, 5), "on the Never touch list");
        assert!(!may_offer(true, false, true, 1), "one window");
    }
}
