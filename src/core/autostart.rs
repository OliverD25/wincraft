use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

use crate::core::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "WinCraft";

fn open(write: bool) -> Option<HKEY> {
    let access = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
    let mut key: HKEY = std::ptr::null_mut();
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut key) };
    if status == ERROR_SUCCESS {
        Some(key)
    } else {
        log::warn!("cannot open the Run registry key (error {status})");
        None
    }
}

pub fn is_enabled() -> bool {
    let Some(key) = open(false) else {
        return false;
    };
    let mut size: u32 = 0;
    let status = unsafe {
        RegQueryValueExW(
            key,
            wide(VALUE_NAME).as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    unsafe { RegCloseKey(key) };
    status == ERROR_SUCCESS
}

pub fn set(enabled: bool) -> Result<(), String> {
    let key = open(true).ok_or_else(|| "cannot open the Run registry key".to_string())?;
    let name = wide(VALUE_NAME);

    let status = if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("cannot find own path: {e}"))?;
        let command = wide(&format!("\"{}\"", exe.display()));
        unsafe {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                command.as_ptr() as *const u8,
                (command.len() * 2) as u32,
            )
        }
    } else {
        let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
        // Deleting a value that is not there is the state the caller asked for.
        if status == 2 {
            ERROR_SUCCESS
        } else {
            status
        }
    };

    unsafe { RegCloseKey(key) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("registry write failed (error {status})"))
    }
}
