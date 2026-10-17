pub mod appid;
pub mod autostart;
pub mod clipboard;
pub mod clock;
pub mod com;
pub mod config;
pub mod desktop_manager;
pub mod host;
pub mod hotkeys;
pub mod install;
pub mod instance;
pub mod layered;
pub mod log_tail;
pub mod logging;
pub mod monitors;
pub mod package;
pub mod quit;
pub mod release_notes;
pub mod session;
pub mod sha256;
pub mod taskbar;
pub mod theme;
pub mod traits;
pub mod tray;
pub mod ui_bridge;
pub mod update;
pub mod update_notice;
pub mod version_info;
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

/// `%NAME%` replaced by the environment variable, when there is one.
pub fn expand_env(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    loop {
        let Some(start) = rest.find('%') else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        match std::env::var(&after[..end]) {
            Ok(value) if end > 0 => {
                out.push_str(&value);
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
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
    #[test]
    fn variables_in_a_path_are_expanded_when_they_exist() {
        std::env::set_var("WINCRAFT_TEST_DIR", r"C:\Tools");
        assert_eq!(expand_env(r"%WINCRAFT_TEST_DIR%\a.exe"), r"C:\Tools\a.exe");
        assert_eq!(
            expand_env(r"%NO_SUCH_VARIABLE_X%\a.exe"),
            r"%NO_SUCH_VARIABLE_X%\a.exe"
        );
        assert_eq!(expand_env("100% sure"), "100% sure");
        assert_eq!(expand_env("plain"), "plain");
    }

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
