//! Windows that want attention: the ones whose taskbar button Windows shows
//! in orange, because the app flashed it (a chat message, a finished
//! download). The host hears of them through the shell hook messages, so no
//! code runs inside Explorer. A window stays marked until it is activated,
//! as the taskbar's orange does, not merely until it stops flashing.
//! Windows that already flashed before WinCraft started cannot be known.

use std::collections::HashSet;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    HSHELL_HIGHBIT, HSHELL_REDRAW, HSHELL_WINDOWACTIVATED, HSHELL_WINDOWDESTROYED,
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
}
