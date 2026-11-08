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
    /// The groups Put back restores; empty for a notice without the button.
    pub(super) put_back: Vec<Offered>,
}

/// One restarted group in the offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Offered {
    pub(super) group: String,
    pub(super) exe: String,
    /// What the restore panel calls it, for an offer of this group alone.
    pub(super) label: String,
    pub(super) windows: usize,
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
    /// Put back was clicked for these groups.
    PutBack(Vec<String>),
    /// The offer for these groups went away unanswered.
    Expired(Vec<String>),
}

impl Offers {
    /// Offers Put back for `entry`. An offer already on screen takes it in,
    /// so groups that restart together share one notice, and its clock
    /// starts again. Returns the groups the notice now holds.
    pub(super) fn offer(&mut self, entry: Offered, key: Option<&str>, now: u64) -> &[Offered] {
        let mut groups = self
            .shown
            .as_ref()
            .map(|shown| shown.put_back.clone())
            .unwrap_or_default();
        groups.retain(|offered| offered.group != entry.group);
        groups.push(entry);
        let message = offer_message(&groups, key);
        self.show_at(message, groups, OFFER_MS, now);
        self.shown
            .as_ref()
            .map_or(&[], |shown| shown.put_back.as_slice())
    }

    /// Shows `message`, in place of any notice shown before. `put_back`
    /// names the groups the button restores.
    pub(super) fn show(&mut self, message: Message, put_back: Vec<Offered>, length: u64) {
        self.show_at(message, put_back, length, now_ms());
    }

    fn show_at(&mut self, message: Message, put_back: Vec<Offered>, length: u64, now: u64) {
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
            clock: Clock::lasting(now, length),
            put_back,
        });
    }

    pub(super) fn close(&mut self) {
        self.shown = None;
    }

    /// The groups an offer on screen is for.
    pub(super) fn offered(&self) -> Vec<String> {
        self.shown
            .iter()
            .flat_map(|shown| &shown.put_back)
            .map(|offered| offered.group.clone())
            .collect()
    }

    /// Handles what the notice heard, then lets an expired notice go.
    pub(super) fn step(&mut self) -> Outcome {
        self.step_at(now_ms())
    }

    fn step_at(&mut self, now: u64) -> Outcome {
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
                    let groups = self.offered();
                    self.close();
                    if on_button && !groups.is_empty() {
                        return Outcome::PutBack(groups);
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
            let groups = self.offered();
            self.close();
            if !groups.is_empty() {
                return Outcome::Expired(groups);
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
        let entry = Offered {
            group: group.to_string(),
            exe: mine[0].identity.exe.clone(),
            label: label.clone(),
            windows: mine.len(),
        };
        let groups = self.offers.offer(entry, key.as_deref(), now_ms());
        if groups.len() == 1 {
            log::info!(
                "offered to put the {} windows of \"{label}\" back after its restart",
                mine.len()
            );
        } else {
            log::info!(
                "\"{label}\" ({} windows) joined the offer on screen, which now holds {} groups with {} windows",
                mine.len(),
                groups.len(),
                groups.iter().map(|offered| offered.windows).sum::<usize>()
            );
        }
    }

    /// The mouse on the notice, Put back clicked, or the notice due to go.
    pub(super) fn offer_step(&mut self) {
        match self.offers.step() {
            Outcome::Nothing => {}
            Outcome::PutBack(groups) => self.put_back(&groups),
            Outcome::Expired(groups) => log::info!(
                "the offer to put {} back went unanswered",
                self.quoted(&groups)
            ),
        }
    }

    /// Put back was clicked: restores those groups and no other, from the
    /// layout saved before their restart. Each group gets its order once
    /// its own windows hold still, as in any restore.
    pub(super) fn put_back(&mut self, groups: &[String]) {
        log::info!("Put back clicked: restoring {} only", self.quoted(groups));
        self.start_restore_of(false, Some(groups));
    }

    fn quoted(&self, groups: &[String]) -> String {
        groups
            .iter()
            .map(|group| format!("\"{}\"", self.label_of(group)))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The word after Save layout now; it takes the offer's place.
    pub(super) fn show_save_notice(&mut self, message: Message) {
        let groups = self.offers.offered();
        if !groups.is_empty() {
            log::info!(
                "the offer to put {} back gave way to the save notice",
                self.quoted(&groups)
            );
        }
        self.offers.show(message, Vec::new(), shared::PLAIN_MS);
    }
}

/// How many apps the saved layout holds: its taskbar groups, or the
/// program for a window saved without one.
pub(super) fn saved_apps(saved: Option<&Programs>) -> usize {
    let mut apps = BTreeSet::new();
    for (exe, program) in saved.into_iter().flatten() {
        for window in &program.windows {
            apps.insert(if window.group.is_empty() {
                exe.clone()
            } else {
                window.group.clone()
            });
        }
    }
    apps.len()
}

/// Whether a group whose restart just settled gets the offer: the setting
/// is on, no restore is under way (the restore after sign-in included),
/// its app is watched (not on the Never touch list) and it has at least
/// two windows.
pub(super) fn may_offer(on: bool, restoring: bool, watched: bool, windows: usize) -> bool {
    on && !restoring && watched && windows >= 2
}

/// The offer for these groups; `key` is the Restore layout hotkey as
/// stored in the plugin's file, None when nothing is stored (the default).
/// One group goes by its own label; groups of one program by the program's
/// name; groups of several programs by up to two names and a count.
pub(super) fn offer_message(groups: &[Offered], key: Option<&str>) -> Message {
    let windows: usize = groups.iter().map(|offered| offered.windows).sum();
    let mut apps: Vec<String> = Vec::new();
    for offered in groups {
        let name = identity::program_name(&offered.exe);
        if !apps.contains(&name) {
            apps.push(name);
        }
    }
    let text = match (groups, apps.as_slice()) {
        ([one], _) => format!(
            "{} restarted. Put its {windows} windows back in order?",
            one.label
        ),
        (_, [app]) => format!("{app} restarted. Put its {windows} windows back in order?"),
        (_, [first, second]) => format!(
            "{first} and {second} restarted. Put their {windows} windows back in order?"
        ),
        (_, [first, second, rest @ ..]) => format!(
            "{first}, {second} and {} more {} restarted. Put their {windows} windows back in order?",
            rest.len(),
            if rest.len() == 1 { "app" } else { "apps" }
        ),
        _ => format!("Apps restarted. Put their {windows} windows back in order?"),
    };
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

/// After Save layout now: how many windows of how many apps were saved.
pub(super) fn saved_message(windows: usize, apps: usize) -> Message {
    let plural = |count: usize, one: &str, many: &str| {
        format!("{count} {}", if count == 1 { one } else { many })
    };
    let text = format!(
        "Layout saved: {} in {}.",
        plural(windows, "window", "windows"),
        plural(apps, "app", "apps")
    );
    Message::new(NAME, Glyph::Check, text)
}

/// After Save layout now failed.
pub(super) fn save_failed_message(reason: &str) -> Message {
    Message::new(NAME, Glyph::Info, "Could not save the layout.".to_string())
        .detail(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offer_names_the_app_its_windows_and_the_key_the_user_set() {
        let message = offer_message(&[entry("Chrome", "chrome.exe", 5)], None);
        assert_eq!(message.app, "LayoutKeeper");
        assert_eq!(
            message.text,
            "Chrome restarted. Put its 5 windows back in order?"
        );
        assert_eq!(message.detail.as_deref(), Some("Or press Win+Alt+L"));
        assert_eq!(message.button, Some("Put back"));
        let telegram = [entry("Telegram", "telegram.exe", 2)];
        let rebound = offer_message(&telegram, Some("win+ctrl+r"));
        assert_eq!(rebound.detail.as_deref(), Some("Or press Win+Ctrl+R"));
        assert_eq!(offer_message(&telegram, Some("")).detail, None);
    }

    fn entry(group: &str, exe: &str, windows: usize) -> Offered {
        Offered {
            group: group.to_string(),
            exe: exe.to_string(),
            label: group.to_string(),
            windows,
        }
    }

    #[test]
    fn groups_of_one_program_go_by_its_name_and_several_programs_by_and() {
        let main = entry("Chrome · Dmytro (Oliver)", "chrome.exe", 24);
        let gemini = entry("Chrome · Google Gemini WebAPP", "chrome.exe", 4);
        let telegram = entry("Telegram", "telegram.exe", 5);
        let notepad = entry("Notepad", "notepad.exe", 2);
        let text = |groups: &[Offered]| offer_message(groups, None).text;
        assert_eq!(
            text(&[main.clone(), gemini.clone()]),
            "Chrome restarted. Put its 28 windows back in order?"
        );
        assert_eq!(
            text(&[gemini.clone(), telegram.clone()]),
            "Chrome and Telegram restarted. Put their 9 windows back in order?"
        );
        assert_eq!(
            text(&[main.clone(), telegram.clone(), notepad.clone(), gemini]),
            "Chrome, Telegram and 1 more app restarted. Put their 35 windows back in order?"
        );
        let paint = entry("Paint", "mspaint.exe", 2);
        assert!(text(&[main, telegram, notepad, paint]).contains("and 2 more apps restarted"));
    }

    #[test]
    fn an_offer_that_comes_while_one_shows_joins_it_and_starts_its_clock_again() {
        let mut offers = Offers::default();
        let main = entry("Chrome · Dmytro (Oliver)", "chrome.exe", 24);
        let gemini = entry("Chrome · Google Gemini WebAPP", "chrome.exe", 4);
        offers.offer(main.clone(), None, 0);
        assert_eq!(offers.offer(gemini.clone(), None, 10_000).len(), 2);
        let shown = offers.shown.as_ref().expect("one notice");
        assert_eq!(
            shown.message.text,
            "Chrome restarted. Put its 28 windows back in order?"
        );
        // The same group again takes the place of its own entry.
        assert_eq!(offers.offer(gemini.clone(), None, 10_000).len(), 2);
        assert_eq!(
            offers.step_at(OFFER_MS),
            Outcome::Nothing,
            "the join started the clock again"
        );
        assert_eq!(
            offers.step_at(10_000 + OFFER_MS),
            Outcome::Expired(vec![main.group, gemini.group])
        );
        assert!(offers.offered().is_empty(), "expiry lets every group go");
    }

    #[test]
    fn only_a_watched_app_with_two_windows_and_no_restore_running_is_offered() {
        assert!(may_offer(true, false, true, 2));
        assert!(!may_offer(false, false, true, 5), "the setting is off");
        assert!(!may_offer(true, true, true, 5), "a restore is running");
        assert!(!may_offer(true, false, false, 5), "on the Never touch list");
        assert!(!may_offer(true, false, true, 1), "one window");
    }

    #[test]
    fn the_save_notice_counts_windows_and_apps() {
        assert_eq!(
            saved_message(23, 7).text,
            "Layout saved: 23 windows in 7 apps."
        );
        assert_eq!(saved_message(1, 1).text, "Layout saved: 1 window in 1 app.");
        let saved = saved_message(2, 1);
        assert_eq!(saved.text, "Layout saved: 2 windows in 1 app.");
        assert!(!saved.has_button() && saved.detail.is_none());
        let failed = save_failed_message("access denied");
        assert_eq!(failed.text, "Could not save the layout.");
        assert_eq!(failed.detail.as_deref(), Some("access denied"));
        assert_eq!(failed.glyph, Glyph::Info);
        assert_eq!(failed.app, "LayoutKeeper");
    }
}
