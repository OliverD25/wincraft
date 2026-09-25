pub mod autostart;
pub mod clipboard;
pub mod clock;
pub mod com;
pub mod config;
pub mod desktop_manager;
pub mod host;
pub mod hotkeys;
pub mod instance;
pub mod logging;
pub mod monitors;
pub mod quit;
pub mod taskbar;
pub mod theme;
pub mod traits;
pub mod tray;
pub mod ui_bridge;
pub mod windows_list;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// A NUL-terminated UTF-16 string that Windows handed out, as a String;
/// empty for a null pointer.
///
/// # Safety
/// `text` must be null or point to a NUL-terminated UTF-16 string.
pub unsafe fn from_wide_ptr(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut len = 0;
    while unsafe { *text.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_string_ends_with_one_nul() {
        assert_eq!(wide("ab"), [97, 98, 0]);
        assert_eq!(wide(""), [0]);
    }

    #[test]
    fn a_wide_pointer_is_read_up_to_its_nul() {
        let text = wide("C:\\Program Files");
        assert_eq!(unsafe { from_wide_ptr(text.as_ptr()) }, "C:\\Program Files");
        let empty = wide("");
        assert_eq!(unsafe { from_wide_ptr(empty.as_ptr()) }, "");
        assert_eq!(unsafe { from_wide_ptr(std::ptr::null()) }, "");
        let cut_short: Vec<u16> = vec![0x0442, 0x0435, 0, 0x0441, 0];
        assert_eq!(
            unsafe { from_wide_ptr(cut_short.as_ptr()) },
            "\u{442}\u{435}"
        );
    }

    #[test]
    fn a_wide_round_trip_keeps_non_ascii_text() {
        let text = wide("Карточный Офис");
        assert_eq!(unsafe { from_wide_ptr(text.as_ptr()) }, "Карточный Офис");
    }
}
