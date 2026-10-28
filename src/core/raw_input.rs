//! Keeps WinCraft's low-level keyboard hooks working while one of its own
//! windows is in front.
//!
//! winit registers every event loop for raw keyboard input when it starts
//! (`register_all_mice_and_keyboards_for_raw_input`, `RIDEV_DEVNOTIFY`,
//! the hidden message window as the target). While a window of that
//! thread is in front, Windows then stops calling this process's own
//! low-level keyboard hooks for key presses; the releases still arrive,
//! and so do the presses while another program's hook sits ahead in the
//! chain, which hides the fault behind any diagnostic hook. Seen on
//! 2026-10-28: Win+1 opened WindowDeck, and Win+2 then went to Windows,
//! which switched to app 2, while the hook never heard of it. Others have
//! met it with Chromium windows in front (tauri-apps/wry#1761), where
//! removing the registration cured it. WinCraft never reads raw keyboard
//! input (egui gets its keys as window messages), so the registration is
//! removed. The mouse registration stays; it does no harm.

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::UI::Input::{
    GetRegisteredRawInputDevices, RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_REMOVE,
};

/// HID usage page and usage of a keyboard.
const GENERIC_DESKTOP: u16 = 1;
const KEYBOARD: u16 = 6;

/// Whether a registration (usage page, usage) is the keyboard one.
pub fn is_keyboard(device: (u16, u16)) -> bool {
    device == (GENERIC_DESKTOP, KEYBOARD)
}

/// The raw input registrations of this process, as (usage page, usage).
pub fn registered() -> Vec<(u16, u16)> {
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    let mut count = 0u32;
    unsafe { GetRegisteredRawInputDevices(std::ptr::null_mut(), &mut count, size) };
    if count == 0 {
        return Vec::new();
    }
    let mut devices: Vec<RAWINPUTDEVICE> = Vec::with_capacity(count as usize);
    let got = unsafe { GetRegisteredRawInputDevices(devices.as_mut_ptr(), &mut count, size) };
    if got == u32::MAX {
        return Vec::new();
    }
    unsafe { devices.set_len(got as usize) };
    devices
        .iter()
        .map(|device| (device.usUsagePage, device.usUsage))
        .collect()
}

/// Removes this process's raw keyboard input registration. None when there
/// is none; otherwise whether Windows accepted the removal.
pub fn drop_keyboard() -> Option<bool> {
    if !registered().into_iter().any(is_keyboard) {
        return None;
    }
    let remove = RAWINPUTDEVICE {
        usUsagePage: GENERIC_DESKTOP,
        usUsage: KEYBOARD,
        dwFlags: RIDEV_REMOVE,
        hwndTarget: std::ptr::null_mut(),
    };
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    Some(unsafe { RegisterRawInputDevices(&remove, 1, size) } != 0)
}

static SAID: AtomicBool = AtomicBool::new(false);

/// Called once per UI frame: removes the registration whenever it is
/// there (winit adds it when the event loop starts) and says so once.
/// One cheap system call per frame when there is nothing to do.
pub fn keep_hooks_working() {
    match drop_keyboard() {
        None => {}
        Some(true) => {
            if !SAID.swap(true, Ordering::Relaxed) {
                log::info!(
                    "raw keyboard input registration removed, so Windows keeps calling WinCraft's keyboard hooks while a WinCraft window is in front"
                );
            }
        }
        Some(false) => log::warn!(
            "could not remove the raw keyboard input registration: {}; Win+number may fail while a WinCraft window is in front",
            std::io::Error::last_os_error()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::RIDEV_DEVNOTIFY;

    #[test]
    fn only_the_keyboard_registration_counts() {
        assert!(is_keyboard((1, 6)));
        assert!(!is_keyboard((1, 2)), "a mouse");
        assert!(!is_keyboard((1, 0)), "the whole page");
    }

    #[test]
    fn the_keyboard_registration_is_found_and_removed() {
        // The test process has none to begin with.
        assert!(!registered().into_iter().any(is_keyboard));
        assert_eq!(drop_keyboard(), None);
        // Registered the way winit does it, with no target window.
        let device = RAWINPUTDEVICE {
            usUsagePage: GENERIC_DESKTOP,
            usUsage: KEYBOARD,
            dwFlags: RIDEV_DEVNOTIFY,
            hwndTarget: std::ptr::null_mut(),
        };
        let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
        assert_ne!(unsafe { RegisterRawInputDevices(&device, 1, size) }, 0);
        assert!(registered().into_iter().any(is_keyboard));
        assert_eq!(drop_keyboard(), Some(true));
        assert!(!registered().into_iter().any(is_keyboard));
        assert_eq!(drop_keyboard(), None, "nothing left to remove");
    }
}
