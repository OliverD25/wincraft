pub mod autostart;
pub mod clock;
pub mod config;
pub mod host;
pub mod hotkeys;
pub mod logging;
pub mod monitors;
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
