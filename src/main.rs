#![windows_subsystem = "windows"]

mod core;
mod plugins;
mod store;
mod ui;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
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

    // A check that reads the screen and exits. It skips logging::init, which
    // would empty the log of the WinCraft already running.
    if let Some(point) = arg_after("--probe-taskbar-at") {
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let parsed = point
            .split_once(',')
            .and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)));
        match parsed {
            Some((x, y)) => {
                let app = core::taskbar::app_at(x, y);
                println!("{x},{y}: {}", app.as_deref().unwrap_or("none"));
            }
            None => println!("expected --probe-taskbar-at X,Y, got {point}"),
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

    let args: Vec<String> = std::env::args().collect();
    let flags = host::StartupFlags {
        open_detector: args.iter().any(|arg| arg == "--open-detector"),
        open_palette: args.iter().any(|arg| arg == "--open-palette"),
        open_arrange: args.iter().any(|arg| arg == "--open-arrange"),
        open_settings: args.iter().any(|arg| arg == "--open-settings"),
    };
    // Plugins talk to shell COM objects from the host thread, and those are
    // apartment-threaded; the message loop host::run pumps is what that needs.
    let com = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    if com < 0 {
        log::warn!(
            "COM could not start on the host thread (0x{:08X})",
            com as u32
        );
    }
    host::run(config, plugins::load_active_plugins(), flags);
    if com >= 0 {
        unsafe { CoUninitialize() };
    }

    unsafe { CloseHandle(mutex) };
}

fn arg_after(flag: &str) -> Option<String> {
    let mut args = std::env::args();
    args.find(|arg| arg == flag)?;
    args.next()
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
