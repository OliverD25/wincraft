pub mod config;
pub mod hotkeys;
pub mod logging;
pub mod traits;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}
