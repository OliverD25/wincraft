pub mod appid;
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
pub mod package;
pub mod quit;
pub mod taskbar;
pub mod theme;
pub mod traits;
pub mod tray;
pub mod ui_bridge;
pub mod window_names;
pub mod windows_list;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};

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

/// Bash and PowerShell 7 write UTF-8; cmd and Windows PowerShell write the
/// console's OEM code page when their output goes to a pipe.
pub fn decode_console(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    let needed = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide_text = vec![0u16; needed as usize];
    unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide_text.as_mut_ptr(),
            needed,
        )
    };
    String::from_utf16_lossy(&wide_text)
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
