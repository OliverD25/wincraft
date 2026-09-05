pub mod about;
pub mod autostart;
pub mod config;
pub mod host;
pub mod hotkeys;
pub mod logging;
pub mod theme;
pub mod traits;
pub mod tray;
pub mod ui_bridge;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
