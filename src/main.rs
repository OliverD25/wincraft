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
use crate::core::{autostart, host, instance, logging, quit, wide};

const SINGLE_INSTANCE_MUTEX: &str = r"Local\WinCraft.SingleInstance";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Before the single-instance guard on purpose: regenerating the index is a
    // one-shot job a contributor runs while WinCraft may already be in the tray.
    if has_flag(&args, "--write-plugin-index") {
        logging::init();
        match store::index::write_committed_copy() {
            Ok(path) => log::info!("wrote {}", path.display()),
            Err(err) => log::error!("{err}"),
        }
        return;
    }

    // A check that reads the screen and exits. It skips logging::init, which
    // would empty the log of the WinCraft already running.
    if let Some(point) = arg_after(&args, "--probe-taskbar-at") {
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        match parse_point(&point) {
            Some((x, y)) => {
                let app = core::taskbar::app_at(x, y);
                println!("{x},{y}: {}", app.as_deref().unwrap_or("none"));
            }
            None => println!("expected --probe-taskbar-at X,Y, got {point}"),
        }
        return;
    }

    // Also before logging::init, for the same reason as the probe.
    if has_flag(&args, "--quit") {
        let outcome = quit::run(quit::TIMEOUT);
        println!("{}", outcome.message());
        std::process::exit(outcome.exit_code());
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

    let flags = host::StartupFlags {
        open_detector: has_flag(&args, "--open-detector"),
        open_palette: has_flag(&args, "--open-palette"),
        open_arrange: has_flag(&args, "--open-arrange"),
        open_settings: has_flag(&args, "--open-settings"),
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

/// Flags are matched whole: the program's own path never counts.
fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().skip(1).any(|arg| arg == flag)
}

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().skip(1).position(|arg| arg == flag)?;
    args.get(at + 2).cloned()
}

/// "X,Y" in screen pixels; either may be negative on a monitor left of or
/// above the primary.
fn parse_point(text: &str) -> Option<(i32, i32)> {
    let (x, y) = text.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn claim_single_instance() -> Option<HANDLE> {
    let name = wide(&instance::name(SINGLE_INSTANCE_MUTEX));
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
    if handle.is_null() {
        return None;
    }
    if unsafe { windows_sys::Win32::Foundation::GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe { CloseHandle(handle) };
        return None;
    }
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn a_flag_is_found_anywhere_after_the_program() {
        assert!(has_flag(&args(&["wincraft.exe", "--quit"]), "--quit"));
        assert!(has_flag(
            &args(&["wincraft.exe", "--open-palette", "--quit"]),
            "--quit"
        ));
        assert!(!has_flag(&args(&["wincraft.exe"]), "--quit"));
        assert!(!has_flag(&args(&[]), "--quit"));
    }

    #[test]
    fn a_flag_must_match_whole() {
        assert!(!has_flag(&args(&["wincraft.exe", "--quit-now"]), "--quit"));
        assert!(!has_flag(&args(&["wincraft.exe", "--QUIT"]), "--quit"));
        assert!(!has_flag(&args(&["wincraft.exe", "quit"]), "--quit"));
        // The program's own path is not an argument, whatever it is called.
        assert!(!has_flag(&args(&["--quit"]), "--quit"));
    }

    #[test]
    fn the_value_after_a_flag_is_read() {
        let list = args(&["wincraft.exe", "--probe-taskbar-at", "10,20"]);
        assert_eq!(
            arg_after(&list, "--probe-taskbar-at").as_deref(),
            Some("10,20")
        );
        let missing = args(&["wincraft.exe", "--probe-taskbar-at"]);
        assert_eq!(arg_after(&missing, "--probe-taskbar-at"), None);
        assert_eq!(
            arg_after(&args(&["wincraft.exe"]), "--probe-taskbar-at"),
            None
        );
        assert_eq!(
            arg_after(&args(&["--probe-taskbar-at", "1,2"]), "--probe-taskbar-at"),
            None
        );
    }

    #[test]
    fn points_are_read_with_negative_and_spaced_numbers() {
        assert_eq!(parse_point("1294,2124"), Some((1294, 2124)));
        assert_eq!(parse_point("-958, 1056"), Some((-958, 1056)));
        assert_eq!(parse_point(" 5 , -7 "), Some((5, -7)));
        assert_eq!(parse_point("1294"), None);
        assert_eq!(parse_point("a,b"), None);
        assert_eq!(parse_point("1,2,3"), None);
        assert_eq!(parse_point(""), None);
    }
}
