//! "Press it for real": the one way to learn which app holds a key when
//! nothing else can tell. After the user's click WinCraft sends the
//! combination once and watches for 3 s which window comes to the front or
//! appears. The other app really acts (Principle 1), so this runs only on
//! that click, never in a test instance, and never for a key Windows
//! reserves.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::core::traits::Hotkey;
use crate::core::{hotkeys, instance, mask_key, windows_list};

use super::known;
use super::owners::{SOURCE_LINK, SOURCE_PRESS};
use super::probe::{ScanResult, Status};

pub const WATCH: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(100);
pub const NOTHING: &str = "Nothing visible happened in 3 s.";
pub const TEST_COPY: &str = "A test copy of WinCraft never presses keys on your desktop.";
const RESERVED: &str = "Windows keeps this key. WinCraft never sends it.";

/// Why the combination may not be sent, if it may not.
pub fn refusal(hotkey: Hotkey, test_instance: bool) -> Option<&'static str> {
    if known::is_reserved(hotkey) {
        Some(RESERVED)
    } else if test_instance {
        Some(TEST_COPY)
    } else {
        None
    }
}

/// The keys to send, in order: the modifiers down, the key down and up,
/// the mask key while Win or Alt is still held (so Windows does not take the
/// release for a lone Win or Alt press and open Start or a menu), then the
/// modifiers up.
pub fn keys(hotkey: Hotkey) -> Vec<(u32, bool)> {
    let held: Vec<u32> = [
        (MOD_CONTROL, VK_CONTROL),
        (MOD_ALT, VK_MENU),
        (MOD_SHIFT, VK_SHIFT),
        (MOD_WIN, VK_LWIN),
    ]
    .into_iter()
    .filter(|(bit, _)| hotkey.modifiers & bit != 0)
    .map(|(_, vk)| u32::from(vk))
    .collect();
    let mut keys: Vec<(u32, bool)> = held.iter().map(|vk| (*vk, false)).collect();
    keys.push((hotkey.vk, false));
    keys.push((hotkey.vk, true));
    if hotkey.modifiers & (MOD_WIN | MOD_ALT) != 0 {
        keys.push((mask_key::VK, false));
        keys.push((mask_key::VK, true));
    }
    keys.extend(held.iter().rev().map(|vk| (*vk, true)));
    keys
}

/// What the watch saw: a window's title and its program's file name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub title: String,
    pub app: String,
}

/// The line under the Check a shortcut box after a press.
pub fn result_text(seen: Option<&Seen>) -> String {
    match seen {
        None => NOTHING.to_string(),
        Some(seen) if seen.title.is_empty() => {
            format!("Pressing it brought {} to the front.", seen.app)
        }
        Some(seen) => format!(
            "Pressing it opened \u{201c}{}\u{201d} from {}.",
            seen.title, seen.app
        ),
    }
}

/// The program name to put in the Owner column: "WindowsTerminal.exe"
/// becomes "WindowsTerminal".
pub fn app_name(seen: &Seen) -> String {
    let name = seen.app.as_str();
    match name.len().checked_sub(4) {
        Some(at) if name[at..].eq_ignore_ascii_case(".exe") => name[..at].to_string(),
        _ => name.to_string(),
    }
}

/// The scan with the pressed app named as the holder of `hotkey`, or None
/// when that row needs no change: it is not an app's, or a shortcut file
/// already names its owner for certain.
pub fn marked(scan: &ScanResult, hotkey: Hotkey, app: &str) -> Option<ScanResult> {
    let at = scan.entries.iter().position(|entry| {
        entry.hotkey == hotkey && entry.status == Status::TakenByApp && entry.source != SOURCE_LINK
    })?;
    let mut copy = scan.clone();
    copy.entries[at].holder = app.to_string();
    copy.entries[at].source = SOURCE_PRESS;
    Some(copy)
}

/// Of the windows on screen after the press, the one the press brought:
/// the window now in front if it changed and is not WinCraft's own, else
/// the first visible window that was not there before.
pub fn pick<T: PartialEq + Copy>(
    front_before: T,
    front_now: T,
    own: T,
    before: &[T],
    now: &[T],
) -> Option<T> {
    if front_now != front_before && front_now != own {
        return Some(front_now);
    }
    now.iter()
        .find(|window| **window != own && !before.contains(window))
        .copied()
}

/// A press test running on its own thread.
pub struct Run {
    pub hotkey: Hotkey,
    done: Arc<Mutex<Option<Option<Seen>>>>,
}

impl Run {
    /// None while it watches; then what it saw, if anything.
    pub fn finished(&self) -> Option<Option<Seen>> {
        self.done.lock().ok().and_then(|done| done.clone())
    }
}

/// Sends the combination once and watches. The caller has checked
/// `refusal` and stopped its own keyboard hook first; this checks again.
pub fn start(hotkey: Hotkey, own_window: isize) -> Result<Run, &'static str> {
    if let Some(why) = refusal(hotkey, instance::is_test()) {
        return Err(why);
    }
    let done = Arc::new(Mutex::new(None));
    let writer = Arc::clone(&done);
    std::thread::spawn(move || {
        let seen = watch(hotkey, own_window);
        if let Ok(mut done) = writer.lock() {
            *done = Some(seen);
        }
    });
    Ok(Run { hotkey, done })
}

fn visible_windows() -> Vec<isize> {
    windows_list::top_windows()
        .into_iter()
        .filter(|hwnd| unsafe { IsWindowVisible(*hwnd) } != 0)
        .map(|hwnd| hwnd as isize)
        .collect()
}

fn seen(hwnd: isize) -> Seen {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd as HWND, &mut pid) };
    let path = windows_list::exe_path(pid);
    Seen {
        title: windows_list::window_text(hwnd as HWND),
        app: path.rsplit('\\').next().unwrap_or("").to_string(),
    }
}

fn watch(hotkey: Hotkey, own_window: isize) -> Option<Seen> {
    let before = visible_windows();
    let front_before = unsafe { GetForegroundWindow() } as isize;
    log::info!(
        "shortcut manager: pressing {} for real, as the user asked",
        hotkeys::format(hotkey)
    );
    if !mask_key::send_keys(&keys(hotkey)) {
        log::warn!("shortcut manager: Windows did not take all the keys of the press");
    }
    let start = Instant::now();
    while start.elapsed() < WATCH {
        std::thread::sleep(POLL);
        let now = visible_windows();
        let front_now = unsafe { GetForegroundWindow() } as isize;
        if let Some(found) = pick(front_before, front_now, own_window, &before, &now) {
            let seen = seen(found);
            if !seen.app.is_empty() || !seen.title.is_empty() {
                log::info!("shortcut manager: the press brought {}", seen.app);
                return Some(seen);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Hotkey {
        hotkeys::parse(text).expect(text)
    }

    #[test]
    fn nothing_is_sent_in_a_test_copy_or_for_a_reserved_key() {
        assert_eq!(refusal(key("Win+L"), false), Some(RESERVED));
        assert_eq!(refusal(key("Ctrl+Alt+Delete"), false), Some(RESERVED));
        assert_eq!(refusal(key("Win+`"), true), Some(TEST_COPY));
        assert_eq!(refusal(key("Win+`"), false), None);
        // The tests themselves count as a test copy only when the suffix is
        // set, so start() is checked through refusal() above and never
        // called here: a test never presses a key on the real desktop.
    }

    #[test]
    fn the_keys_go_down_in_order_and_up_the_other_way_round() {
        let ctrl = u32::from(VK_CONTROL);
        let alt = u32::from(VK_MENU);
        let win = u32::from(VK_LWIN);
        let f9 = 0x78;
        assert_eq!(
            keys(key("Ctrl+F9")),
            [(ctrl, false), (f9, false), (f9, true), (ctrl, true)]
        );
        assert_eq!(
            keys(key("Win+Alt+F9")),
            [
                (alt, false),
                (win, false),
                (f9, false),
                (f9, true),
                (mask_key::VK, false),
                (mask_key::VK, true),
                (win, true),
                (alt, true)
            ]
        );
    }

    #[test]
    fn the_result_line_names_the_window_and_its_app() {
        let terminal = Seen {
            title: "Windows PowerShell".to_string(),
            app: "WindowsTerminal.exe".to_string(),
        };
        assert_eq!(
            result_text(Some(&terminal)),
            "Pressing it opened \u{201c}Windows PowerShell\u{201d} from WindowsTerminal.exe."
        );
        assert_eq!(app_name(&terminal), "WindowsTerminal");
        let untitled = Seen {
            title: String::new(),
            app: "ShareX.exe".to_string(),
        };
        assert_eq!(
            result_text(Some(&untitled)),
            "Pressing it brought ShareX.exe to the front."
        );
        assert_eq!(result_text(None), NOTHING);
        let odd = Seen {
            title: String::new(),
            app: "tool".to_string(),
        };
        assert_eq!(app_name(&odd), "tool");
    }

    #[test]
    fn the_window_in_front_wins_unless_it_is_wincraft() {
        let own = 9;
        assert_eq!(pick(1, 5, own, &[1, 2], &[1, 2, 5]), Some(5));
        assert_eq!(pick(1, own, own, &[1, 2], &[1, 2, own]), None);
        assert_eq!(
            pick(1, 1, own, &[1, 2], &[1, 2, 7]),
            Some(7),
            "a new window"
        );
        assert_eq!(pick(1, 1, own, &[1, 2], &[1, 2]), None);
    }

    #[test]
    fn a_press_names_the_app_in_the_list_unless_a_shortcut_file_already_does() {
        use super::super::probe::classify;
        let mut link = classify(key("Ctrl+Alt+N"), true, None);
        link.holder = "Explorer shortcut: Notes".to_string();
        link.source = SOURCE_LINK;
        let scan = ScanResult {
            entries: vec![
                classify(key("Ctrl+Alt+F9"), true, None),
                classify(key("Win+E"), true, None),
                link,
            ],
            probed: 3,
            elapsed_ms: 1,
            owners: Default::default(),
        };
        let marked = marked(&scan, key("Ctrl+Alt+F9"), "ShareX").expect("an app row");
        assert_eq!(marked.entries[0].owner(), "ShareX");
        assert_eq!(marked.entries[0].source, SOURCE_PRESS);
        assert_eq!(scan.entries[0].owner(), "unknown app", "the old scan stays");
        assert!(super::marked(&scan, key("Win+E"), "x").is_none());
        assert!(super::marked(&scan, key("Ctrl+Alt+N"), "x").is_none());
    }
}
