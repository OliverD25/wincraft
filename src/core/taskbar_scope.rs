//! Which windows a taskbar shows when there are several monitors. Windows
//! keeps the choice under "Multiple displays" in Settings, Personalization,
//! Taskbar, and a secondary taskbar's hover popup holds only what that
//! taskbar shows. The hover strip follows the same rule.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};

use crate::core::wide;

const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

/// `MMTaskbarEnabled` and `MMTaskbarMode`. Windows writes neither until the
/// setting is changed, and then shows a taskbar on every monitor with every
/// window on all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiMonitor {
    /// A taskbar on every monitor; off means only the main one exists.
    pub enabled: bool,
    /// 0: every taskbar shows all windows. 1: the main taskbar shows all,
    /// the others only the windows on their monitor. 2: every taskbar shows
    /// only the windows on its own monitor.
    pub mode: u32,
}

impl Default for MultiMonitor {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: 0,
        }
    }
}

impl MultiMonitor {
    pub fn read() -> Self {
        Self::from_values(read_dword("MMTaskbarEnabled"), read_dword("MMTaskbarMode"))
    }

    pub fn from_values(enabled: Option<u32>, mode: Option<u32>) -> Self {
        let default = Self::default();
        Self {
            enabled: enabled.map_or(default.enabled, |value| value != 0),
            mode: mode.unwrap_or(default.mode),
        }
    }

    /// Whether the taskbar on a monitor shows only that monitor's windows.
    /// `main` is the taskbar on the primary monitor. A mode this code does
    /// not know shows everything, like Windows' own default.
    pub fn shows_only_own(&self, main: bool) -> bool {
        if !self.enabled {
            return false;
        }
        match self.mode {
            1 => !main,
            2 => true,
            _ => false,
        }
    }
}

/// How many of the windows, given by the monitor each is on, a taskbar on
/// `hovered` shows.
pub fn count_shown(window_monitors: &[usize], hovered: usize, own_only: bool) -> usize {
    if own_only {
        window_monitors.iter().filter(|m| **m == hovered).count()
    } else {
        window_monitors.len()
    }
}

fn read_dword(name: &str) -> Option<u32> {
    let mut key: HKEY = std::ptr::null_mut();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(ADVANCED).as_ptr(),
            0,
            KEY_READ,
            &mut key,
        )
    };
    if opened != ERROR_SUCCESS {
        return None;
    }
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let read = unsafe {
        RegQueryValueExW(
            key,
            wide(name).as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut u8,
            &mut size,
        )
    };
    unsafe { RegCloseKey(key) };
    (read == ERROR_SUCCESS).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(enabled: u32, mode: u32) -> MultiMonitor {
        MultiMonitor::from_values(Some(enabled), Some(mode))
    }

    #[test]
    fn nothing_set_means_every_taskbar_shows_every_window() {
        let none = MultiMonitor::from_values(None, None);
        assert!(!none.shows_only_own(true));
        assert!(!none.shows_only_own(false));
    }

    #[test]
    fn mode_0_shows_all_windows_on_every_taskbar() {
        assert!(!setting(1, 0).shows_only_own(true));
        assert!(!setting(1, 0).shows_only_own(false));
    }

    #[test]
    fn mode_1_filters_only_the_secondary_taskbars() {
        assert!(!setting(1, 1).shows_only_own(true));
        assert!(setting(1, 1).shows_only_own(false));
    }

    #[test]
    fn mode_2_filters_every_taskbar() {
        assert!(setting(1, 2).shows_only_own(true));
        assert!(setting(1, 2).shows_only_own(false));
    }

    #[test]
    fn with_one_taskbar_only_the_main_exists_and_shows_everything() {
        for mode in 0..=2 {
            assert!(!setting(0, mode).shows_only_own(true));
            assert!(!setting(0, mode).shows_only_own(false));
        }
    }

    #[test]
    fn an_unknown_mode_shows_everything() {
        assert!(!setting(1, 7).shows_only_own(false));
    }

    #[test]
    fn the_count_follows_the_filter() {
        let windows = [0, 2, 2, 1, 2];
        assert_eq!(count_shown(&windows, 2, true), 3);
        assert_eq!(count_shown(&windows, 1, true), 1);
        assert_eq!(count_shown(&windows, 0, false), 5);
        assert_eq!(count_shown(&[], 0, true), 0);
    }
}
