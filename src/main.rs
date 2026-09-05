#![windows_subsystem = "windows"]

mod core;
mod modules;
mod store;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use crate::core::config::Config;
use crate::core::{autostart, host, logging, wide};

const SINGLE_INSTANCE_MUTEX: &str = r"Local\WinCraft.SingleInstance";

fn main() {
    // Before the single-instance guard on purpose: regenerating the index is a
    // one-shot job a contributor runs while WinCraft may already be in the tray.
    if std::env::args().any(|arg| arg == "--write-plugin-index") {
        logging::init();
        match store::index::write_committed_copy() {
            Ok(path) => log::info!("wrote {}", path.display()),
            Err(err) => log::error!("{err}"),
        }
        return;
    }

    let Some(mutex) = claim_single_instance() else {
        return;
    };

    // The manifest already asks for PerMonitorV2; this repeats it in case the
    // exe is ever launched in a way that ignores the embedded manifest.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    logging::init();

    // The exe has no console, so without this a panic disappears completely:
    // the tray icon just vanishes and the log ends mid-sentence.
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
    }));

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

    let open_detector = std::env::args().any(|arg| arg == "--open-detector");
    host::run(config, modules::load_active_modules(), open_detector);

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
