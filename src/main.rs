#![windows_subsystem = "windows"]

mod core;
mod modules;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use crate::core::config::Config;
use crate::core::{autostart, host, logging, wide};

const SINGLE_INSTANCE_MUTEX: &str = r"Local\WinCraft.SingleInstance";

fn main() {
    let Some(mutex) = claim_single_instance() else {
        return;
    };

    // The manifest already asks for PerMonitorV2; this repeats it in case the
    // exe is ever launched in a way that ignores the embedded manifest.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    logging::init();
    let mut config = Config::load();

    // The registry is the truth for autostart, so a value removed by hand or by
    // another tool does not leave config.json claiming it is still on.
    let in_registry = autostart::is_enabled();
    if in_registry != config.start_with_windows {
        if let Err(err) = autostart::set(config.start_with_windows) {
            log::warn!("could not apply start_with_windows: {err}");
            config.start_with_windows = in_registry;
        }
    }

    host::run(config, modules::load_active_modules());

    unsafe { CloseHandle(mutex) };
}

fn claim_single_instance() -> Option<HANDLE> {
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide(SINGLE_INSTANCE_MUTEX).as_ptr()) };
    if handle.is_null() {
        return None;
    }
    if unsafe { windows_sys::Win32::Foundation::GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return None;
    }
    Some(handle)
}
