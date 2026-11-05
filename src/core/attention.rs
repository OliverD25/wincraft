//! Windows that want attention: the ones whose taskbar button Windows shows
//! in orange, because the app flashed it (a chat message, a finished
//! download). The host hears of them through the shell hook messages, so no
//! code runs inside Explorer. A window stays marked until it is activated,
//! as the taskbar's orange does, not merely until it stops flashing.
//! Windows that already flashed before WinCraft started cannot be known.

use std::collections::HashSet;

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
}

impl Attention {
    /// Takes one shell hook message: `code` is its wParam, `hwnd` its
    /// lParam, and `foreground` the window in front, which a flash does not
    /// mark since the user is already in it. Returns whether the set
    /// changed.
    pub fn shell_message(&mut self, code: usize, hwnd: isize, foreground: isize) -> bool {
        match (code & 0xFFFF) as u32 {
            HSHELL_FLASH => hwnd != 0 && hwnd != foreground && self.windows.insert(hwnd),
            HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED | HSHELL_WINDOWDESTROYED => {
                self.windows.remove(&hwnd)
            }
            _ => false,
        }
    }

    /// Marks a window as if it had flashed, for a test scene.
    pub fn mark(&mut self, hwnd: isize) {
        self.windows.insert(hwnd);
    }

    pub fn wants(&self, hwnd: isize) -> bool {
        self.windows.contains(&hwnd)
    }

    pub fn count(&self) -> usize {
        self.windows.len()
    }

    /// Forgets every mark. Returns the windows that were marked, in a fixed
    /// order, so the user can take the marks back.
    pub fn clear_all(&mut self) -> Vec<isize> {
        let mut windows: Vec<isize> = self.windows.drain().collect();
        windows.sort_unstable();
        windows
    }

    /// Marks exactly these windows again, as after `clear_all`. Others stay
    /// as they are.
    pub fn restore(&mut self, windows: &[isize]) {
        self.windows.extend(windows.iter().copied());
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
