//! Windows that want attention: the ones whose taskbar button Windows shows
//! in orange, because the app flashed it (a chat message, a finished
//! download). The host hears of them through the shell hook messages, so no
//! code runs inside Explorer. A window stays marked until it is activated,
//! as the taskbar's orange does, not merely until it stops flashing.
//! Windows that already flashed before WinCraft started cannot be known.
//!
//! A second kind of mark comes from outside: a Claude Code hook says "the
//! session with this title waits for you" (`wincraft.exe --waiting`). Such a
//! mark stays through activation, until the hook says the session was
//! answered, the window closes, or the user marks all as seen.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::core::claude_waits::Ask;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    FlashWindowEx, FLASHWINFO, FLASHW_STOP, HSHELL_HIGHBIT, HSHELL_REDRAW, HSHELL_WINDOWACTIVATED,
    HSHELL_WINDOWDESTROYED,
};

/// A window flashes its taskbar button.
const HSHELL_FLASH: u32 = HSHELL_REDRAW | HSHELL_HIGHBIT;
/// A window was activated while a full-screen app was in front, or on
/// Windows 10 and 11 for most activations.
const HSHELL_RUDEAPPACTIVATED: u32 = HSHELL_WINDOWACTIVATED | HSHELL_HIGHBIT;

#[derive(Debug, Default)]
pub struct Attention {
    windows: HashSet<isize>,
    /// Windows a hook said wait for an answer, with the session title and
    /// when the mark was set.
    waiting: HashMap<isize, (String, Instant)>,
    /// The waiting marks the last `clear_all` took, so its undo can bring
    /// them back as waiting marks.
    cleared_waiting: HashMap<isize, (String, Instant)>,
    /// Waiting windows whose hook said a permission prompt blocks them.
    /// Counts only while the window also waits.
    approvals: HashSet<isize>,
    /// Chats WinCraft's own hook said wait (`session:<id>`), by Claude Code
    /// session id, with the time and the kind. Kept by chat, so a window
    /// that shows the chat later, or ClaudeDeck's row of a chat with no
    /// window of its own, still shows the wait.
    sessions: HashMap<String, (Instant, Ask)>,
}

impl Attention {
    /// Takes one shell hook message: `code` is its wParam, `hwnd` its
    /// lParam, and `foreground` the window in front, which a flash does not
    /// mark since the user is already in it. Returns whether the set
    /// changed.
    pub fn shell_message(&mut self, code: usize, hwnd: isize, foreground: isize) -> bool {
        match (code & 0xFFFF) as u32 {
            HSHELL_FLASH => hwnd != 0 && hwnd != foreground && self.windows.insert(hwnd),
            HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED => self.windows.remove(&hwnd),
            HSHELL_WINDOWDESTROYED => {
                let flashed = self.windows.remove(&hwnd);
                self.waiting.remove(&hwnd).is_some() || flashed
            }
            _ => false,
        }
    }

    /// Marks a window as if it had flashed, for a test scene.
    pub fn mark(&mut self, hwnd: isize) {
        self.windows.insert(hwnd);
    }

    /// Marks these windows as waiting for an answer in the session `title`.
    /// Returns how many were not marked as waiting before. A window that
    /// already waited keeps the time of its first mark.
    pub fn wait(&mut self, windows: &[isize], title: &str) -> usize {
        let now = Instant::now();
        windows
            .iter()
            .filter(|hwnd| {
                let since = self.waiting.get(*hwnd).map_or(now, |(_, since)| *since);
                self.waiting
                    .insert(**hwnd, (title.to_string(), since))
                    .is_none()
            })
            .count()
    }

    /// The same as `wait`, saying what the session waits for. The last
    /// hook's word wins: a question after a permission prompt is a question.
    pub fn wait_as(&mut self, windows: &[isize], title: &str, ask: Ask) -> usize {
        let added = self.wait(windows, title);
        for hwnd in windows {
            match ask {
                Ask::Approval => self.approvals.insert(*hwnd),
                Ask::Question => self.approvals.remove(hwnd),
            };
        }
        let waiting = &self.waiting;
        self.approvals.retain(|hwnd| waiting.contains_key(hwnd));
        added
    }

    /// Whether this window waits for a permission prompt.
    pub fn waits_for_approval(&self, hwnd: isize) -> bool {
        self.waiting.contains_key(&hwnd) && self.approvals.contains(&hwnd)
    }

    /// Since when a hook has said this window waits for an answer.
    pub fn waiting_since(&self, hwnd: isize) -> Option<Instant> {
        self.waiting.get(&hwnd).map(|(_, since)| *since)
    }

    /// The session `title` was answered: every window waiting for it, and
    /// the given windows (the ones that show that title now), lose both
    /// kinds of mark. Returns those windows, in a fixed order, so their
    /// taskbar buttons can stop flashing.
    pub fn answered(&mut self, title: &str, windows: &[isize]) -> Vec<isize> {
        let mut cleared: Vec<isize> = self
            .waiting
            .iter()
            .filter(|(hwnd, (waits_for, _))| waits_for.as_str() == title || windows.contains(hwnd))
            .map(|(hwnd, _)| *hwnd)
            .collect();
        for hwnd in &cleared {
            self.waiting.remove(hwnd);
            self.windows.remove(hwnd);
        }
        for hwnd in windows {
            if self.windows.remove(hwnd) {
                cleared.push(*hwnd);
            }
        }
        cleared.sort_unstable();
        cleared.dedup();
        cleared
    }

    /// The chat with this Claude Code session id waits. A chat that
    /// already waited keeps the time of its first mark; the kind is the
    /// last one said.
    pub fn session_wait(&mut self, session: &str, ask: Ask) {
        let since = self
            .sessions
            .get(session)
            .map_or_else(Instant::now, |(since, _)| *since);
        self.sessions.insert(session.to_string(), (since, ask));
    }

    /// The chat was answered. Returns whether it waited.
    pub fn session_answered(&mut self, session: &str) -> bool {
        self.sessions.remove(session).is_some()
    }

    /// Since when the chat waits, and for what.
    pub fn session_waiting(&self, session: &str) -> Option<(Instant, Ask)> {
        self.sessions.get(session).copied()
    }

    pub fn wants(&self, hwnd: isize) -> bool {
        self.windows.contains(&hwnd) || self.waiting.contains_key(&hwnd)
    }

    pub fn count(&self) -> usize {
        self.windows.len()
            + self
                .waiting
                .keys()
                .filter(|hwnd| !self.windows.contains(hwnd))
                .count()
    }

    /// Forgets every mark of both kinds. Returns the windows that were
    /// marked, in a fixed order, so the user can take the marks back.
    pub fn clear_all(&mut self) -> Vec<isize> {
        let mut windows: Vec<isize> = self.windows.drain().collect();
        windows.extend(self.waiting.keys().copied());
        windows.sort_unstable();
        windows.dedup();
        self.cleared_waiting = std::mem::take(&mut self.waiting);
        self.sessions.clear();
        windows
    }

    /// Marks exactly these windows again, as after `clear_all`: a window
    /// that waited for an answer waits again. Others stay as they are.
    pub fn restore(&mut self, windows: &[isize]) {
        for hwnd in windows {
            match self.cleared_waiting.get(hwnd) {
                Some(mark) => {
                    self.waiting.insert(*hwnd, mark.clone());
                }
                None => {
                    self.windows.insert(*hwnd);
                }
            }
        }
    }
}

/// Tells Windows these windows stopped flashing, so their taskbar buttons
/// lose the orange. A test copy of WinCraft never does it: it must not
/// change the user's real windows (Principle 1). Returns how many windows
/// it was called for.
pub fn stop_flashing(windows: &[isize], test_copy: bool) -> usize {
    stop_flashing_with(windows, test_copy, |hwnd| {
        let info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: hwnd as _,
            dwFlags: FLASHW_STOP,
            uCount: 0,
            dwTimeout: 0,
        };
        unsafe { FlashWindowEx(&info) };
    })
}

fn stop_flashing_with(windows: &[isize], test_copy: bool, mut stop: impl FnMut(isize)) -> usize {
    if test_copy {
        return 0;
    }
    for hwnd in windows {
        stop(*hwnd);
    }
    windows.len()
}

/// What WinCraft says after "Mark all as seen" for `count` windows.
pub fn marked_line(count: usize) -> String {
    match count {
        0 => "No windows to mark as seen".to_string(),
        1 => "Marked 1 window as seen".to_string(),
        n => format!("Marked {n} windows as seen"),
    }
}

/// What WinCraft says after the undo. The taskbar cannot be made to flash
/// again, so the line says so.
pub const UNDONE_LINE: &str = "Orange frames are back. The taskbar stays as it is.";

/// The second line of the palette command.
pub fn palette_subtitle(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 window wants attention".to_string()),
        n => Some(format!("{n} windows want attention")),
    }
}

#[cfg(test)]
mod session_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const FLASH: usize = 0x8006;
    const ACTIVATED: usize = 4;
    const RUDE_ACTIVATED: usize = 0x8004;
    const DESTROYED: usize = 2;

    #[test]
    fn a_flash_marks_the_window_until_it_is_activated() {
        let mut attention = Attention::default();
        assert!(attention.shell_message(FLASH, 10, 99));
        assert!(attention.wants(10));
        // Flashing again changes nothing.
        assert!(!attention.shell_message(FLASH, 10, 99));
        // Another window coming to the front leaves the mark.
        assert!(!attention.shell_message(RUDE_ACTIVATED, 11, 0));
        assert!(attention.wants(10));
        assert!(attention.shell_message(RUDE_ACTIVATED, 10, 0));
        assert!(!attention.wants(10));

        attention.shell_message(FLASH, 10, 99);
        assert!(attention.shell_message(ACTIVATED, 10, 0));
        assert!(!attention.wants(10));
    }

    #[test]
    fn a_closed_window_is_dropped_and_others_are_untouched() {
        let mut attention = Attention::default();
        attention.shell_message(FLASH, 10, 0);
        attention.shell_message(FLASH, 20, 0);
        assert!(attention.shell_message(DESTROYED, 10, 0));
        assert!(!attention.wants(10));
        assert!(attention.wants(20));
        assert!(!attention.wants(30));
    }

    #[test]
    fn the_window_in_front_and_other_messages_mark_nothing() {
        let mut attention = Attention::default();
        assert!(!attention.shell_message(FLASH, 10, 10));
        assert!(!attention.shell_message(FLASH, 0, 10));
        // A plain redraw, without the high bit, is not a flash.
        assert!(!attention.shell_message(6, 10, 0));
        assert!(!attention.shell_message(1, 10, 0));
        assert!(!attention.wants(10));
        attention.mark(10);
        assert!(attention.wants(10));
    }

    #[test]
    fn clear_all_returns_the_marked_windows_and_empties_the_set() {
        let mut attention = Attention::default();
        assert!(attention.clear_all().is_empty());
        attention.mark(30);
        attention.mark(10);
        attention.mark(20);
        assert_eq!(attention.count(), 3);
        assert_eq!(attention.clear_all(), [10, 20, 30]);
        assert_eq!(attention.count(), 0);
        assert!(!attention.wants(10));
        assert!(attention.clear_all().is_empty());
    }

    #[test]
    fn restore_marks_only_the_given_windows() {
        let mut attention = Attention::default();
        attention.mark(5);
        attention.restore(&[10, 20]);
        assert!(attention.wants(5) && attention.wants(10) && attention.wants(20));
        assert!(!attention.wants(30));
        let mut other = Attention::default();
        other.restore(&[]);
        assert_eq!(other.count(), 0);
    }

    #[test]
    fn a_waiting_mark_keeps_the_time_it_was_first_set() {
        let mut attention = Attention::default();
        assert_eq!(attention.waiting_since(10), None);
        attention.wait(&[10], "Chat");
        let first = attention.waiting_since(10).expect("marked");
        attention.wait(&[10], "Chat");
        assert_eq!(attention.waiting_since(10), Some(first));
        // A flash alone is not a waiting mark.
        attention.mark(20);
        assert_eq!(attention.waiting_since(20), None);
        attention.answered("Chat", &[]);
        assert_eq!(attention.waiting_since(10), None);
    }

    #[test]
    fn a_permission_prompt_is_kept_apart_until_a_question_or_an_answer() {
        let mut attention = Attention::default();
        attention.wait_as(&[10], "Chat", Ask::Approval);
        attention.wait(&[20], "Other");
        assert!(attention.waits_for_approval(10));
        assert!(!attention.waits_for_approval(20));
        // A question in the same chat is a question again.
        attention.wait_as(&[10], "Chat", Ask::Question);
        assert!(!attention.waits_for_approval(10));
        attention.wait_as(&[10], "Chat", Ask::Approval);
        attention.answered("Chat", &[]);
        assert!(!attention.waits_for_approval(10));
        // A window that only flashed never waits for approval.
        attention.mark(30);
        assert!(!attention.waits_for_approval(30));
    }

    #[test]
    fn a_waiting_mark_survives_activation_until_answered() {
        let mut attention = Attention::default();
        assert_eq!(attention.wait(&[10, 20], "Fix the build"), 2);
        assert_eq!(attention.wait(&[10], "Fix the build"), 0);
        assert!(!attention.shell_message(ACTIVATED, 10, 0));
        assert!(!attention.shell_message(RUDE_ACTIVATED, 10, 0));
        assert!(attention.wants(10) && attention.wants(20));
        assert_eq!(attention.count(), 2);
        // The answer clears every window waiting for that title, nothing else.
        attention.wait(&[30], "Other chat");
        assert_eq!(attention.answered("Fix the build", &[]), [10, 20]);
        assert!(!attention.wants(10) && !attention.wants(20));
        assert!(attention.wants(30));
        assert!(attention.answered("Fix the build", &[]).is_empty());
    }

    #[test]
    fn an_answer_also_clears_the_flash_mark_of_the_windows_it_names() {
        let mut attention = Attention::default();
        // WinCraft's own flash comes back as a shell flash on the same window.
        attention.wait(&[10], "Chat");
        attention.shell_message(FLASH, 10, 0);
        attention.shell_message(FLASH, 40, 0);
        assert_eq!(attention.count(), 2);
        // A window that took the title since the mark is cleared too.
        attention.shell_message(FLASH, 50, 0);
        assert_eq!(attention.answered("Chat", &[50]), [10, 50]);
        assert!(!attention.wants(10) && !attention.wants(50));
        assert!(attention.wants(40));
    }

    #[test]
    fn a_closed_waiting_window_drops_its_mark() {
        let mut attention = Attention::default();
        attention.wait(&[10, 20], "Chat");
        assert!(attention.shell_message(DESTROYED, 10, 0));
        assert!(!attention.wants(10));
        assert!(attention.wants(20));
        assert!(!attention.shell_message(DESTROYED, 10, 0));
    }

    #[test]
    fn mark_all_as_seen_clears_waiting_marks_and_undo_brings_back_each_kind() {
        let mut attention = Attention::default();
        attention.wait(&[10], "Chat");
        attention.mark(20);
        attention.mark(10);
        assert_eq!(attention.clear_all(), [10, 20]);
        assert_eq!(attention.count(), 0);
        attention.restore(&[10, 20]);
        assert!(attention.wants(10) && attention.wants(20));
        // 10 waits again: activation leaves it, and only the answer clears it.
        attention.shell_message(ACTIVATED, 10, 0);
        attention.shell_message(ACTIVATED, 20, 0);
        assert!(attention.wants(10));
        assert!(!attention.wants(20));
        assert_eq!(attention.answered("Chat", &[]), [10]);
    }

    #[test]
    fn a_test_copy_never_stops_a_real_window_flashing() {
        let mut called = Vec::new();
        assert_eq!(stop_flashing_with(&[1, 2], true, |h| called.push(h)), 0);
        assert!(called.is_empty());
        assert_eq!(stop_flashing_with(&[1, 2], false, |h| called.push(h)), 2);
        assert_eq!(called, [1, 2]);
    }

    #[test]
    fn the_lines_name_the_count() {
        assert_eq!(marked_line(0), "No windows to mark as seen");
        assert_eq!(marked_line(1), "Marked 1 window as seen");
        assert_eq!(marked_line(9), "Marked 9 windows as seen");
        assert_eq!(palette_subtitle(0), None);
        assert_eq!(palette_subtitle(1).unwrap(), "1 window wants attention");
        assert_eq!(palette_subtitle(9).unwrap(), "9 windows want attention");
    }
}
