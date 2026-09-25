#![windows_subsystem = "windows"]

mod core;
mod plugins;
mod search;
mod store;
mod ui;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use crate::core::config::{Config, ThemeChoice};
use crate::core::host::SceneFlags;
use crate::core::ui_bridge::{Page, PeekCard, SettingsTarget};
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

    let (scene, theme, notes) = scene_flags(&args, instance::is_test());
    for note in notes {
        log::info!("{note}");
    }
    if let Some(theme) = theme {
        log::info!("theme set to {} for this test run", theme.label());
        config.theme = theme;
    }
    let flags = host::StartupFlags {
        open_detector: has_flag(&args, "--open-detector"),
        open_palette: has_flag(&args, "--open-palette"),
        open_arrange: has_flag(&args, "--open-arrange"),
        open_settings: has_flag(&args, "--open-settings"),
        scene,
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

/// The test-scene flags: `--open-palette=<query>`, `--open-settings=<page>`,
/// `--open-arrange=<group>`, `--peek-card=<n|chip>` and `--theme=<light|dark>`.
/// Only a test instance takes them, so the user's WinCraft can never be
/// opened on a scene by a stray command line. The notes say what was
/// ignored or could not be read, for the log.
fn scene_flags(
    args: &[String],
    test_instance: bool,
) -> (SceneFlags, Option<ThemeChoice>, Vec<String>) {
    let mut scene = SceneFlags::default();
    let mut theme = None;
    let mut notes = Vec::new();
    for arg in args.iter().skip(1) {
        let Some((flag, value)) = arg.split_once('=') else {
            continue;
        };
        if !matches!(
            flag,
            "--open-palette" | "--open-settings" | "--open-arrange" | "--peek-card" | "--theme"
        ) {
            continue;
        }
        if !test_instance {
            notes.push(format!(
                "{flag}= ignored: only a test instance (WINCRAFT_INSTANCE) takes it"
            ));
            continue;
        }
        let read = match flag {
            "--open-palette" => {
                scene.palette_query = Some(value.to_string());
                true
            }
            "--open-settings" => {
                scene.settings = settings_target(value);
                scene.settings.is_some()
            }
            "--open-arrange" => {
                scene.arrange_focus = Some(value.to_string());
                true
            }
            "--peek-card" => {
                scene.peek_card = peek_card(value);
                scene.peek_card.is_some()
            }
            _ => {
                theme = theme_choice(value);
                theme.is_some()
            }
        };
        if !read {
            notes.push(format!("{flag}={value} cannot be read and is ignored"));
        }
    }
    (scene, theme, notes)
}

/// "general", "plugins", "store", "about", or "plugin:<id>".
fn settings_target(value: &str) -> Option<SettingsTarget> {
    let value = value.trim();
    if let Some(id) = value.strip_prefix("plugin:") {
        let id = id.trim();
        return (!id.is_empty()).then(|| SettingsTarget::Plugin(id.to_string()));
    }
    let page = match value.to_ascii_lowercase().as_str() {
        "general" => Page::General,
        "plugins" => Page::Plugins,
        "store" => Page::Store,
        "about" => Page::About,
        _ => return None,
    };
    Some(SettingsTarget::Page(page))
}

fn peek_card(value: &str) -> Option<PeekCard> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("chip") {
        return Some(PeekCard::Chip);
    }
    value.parse().ok().map(PeekCard::Index)
}

fn theme_choice(value: &str) -> Option<ThemeChoice> {
    match value.trim().to_ascii_lowercase().as_str() {
        "light" => Some(ThemeChoice::Light),
        "dark" => Some(ThemeChoice::Dark),
        _ => None,
    }
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
    fn scene_flags_are_ignored_outside_a_test_instance() {
        let list = args(&[
            "wincraft.exe",
            "--open-palette=/",
            "--open-settings=about",
            "--open-arrange=0",
            "--peek-card=chip",
            "--theme=light",
        ]);
        let (scene, theme, notes) = scene_flags(&list, false);
        assert_eq!(scene, SceneFlags::default());
        assert_eq!(theme, None);
        assert_eq!(notes.len(), 5);
        assert!(notes
            .iter()
            .all(|note| note.contains("only a test instance")));
    }

    #[test]
    fn a_test_instance_reads_every_scene_flag() {
        let list = args(&[
            "wincraft.exe",
            "--open-palette==2+2*3",
            "--open-settings=plugin:layout_keeper",
            "--open-arrange=Chrome.UserData.Profile3",
            "--peek-card=2",
            "--theme=Dark",
        ]);
        let (scene, theme, notes) = scene_flags(&list, true);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(scene.palette_query.as_deref(), Some("=2+2*3"));
        assert_eq!(
            scene.settings,
            Some(SettingsTarget::Plugin("layout_keeper".to_string()))
        );
        assert_eq!(
            scene.arrange_focus.as_deref(),
            Some("Chrome.UserData.Profile3")
        );
        assert_eq!(scene.peek_card, Some(PeekCard::Index(2)));
        assert_eq!(theme, Some(ThemeChoice::Dark));
    }

    #[test]
    fn palette_queries_keep_every_prefix_as_typed() {
        for query in ["/", "=2+2*3", "<", "<chrome", "?", "> ls", ""] {
            let list = args(&["wincraft.exe", &format!("--open-palette={query}")]);
            let (scene, _, notes) = scene_flags(&list, true);
            assert!(notes.is_empty());
            assert_eq!(scene.palette_query.as_deref(), Some(query));
        }
    }

    #[test]
    fn flags_without_a_value_are_left_to_the_plain_flags() {
        let list = args(&["wincraft.exe", "--open-palette", "--open-settings"]);
        let (scene, theme, notes) = scene_flags(&list, true);
        assert_eq!(scene, SceneFlags::default());
        assert_eq!(theme, None);
        assert!(notes.is_empty());
        assert!(has_flag(&list, "--open-palette"));
    }

    #[test]
    fn unreadable_scene_values_are_noted_and_skipped() {
        let list = args(&[
            "wincraft.exe",
            "--open-settings=nowhere",
            "--open-settings=plugin:",
            "--peek-card=first",
            "--theme=blue",
            "--other=1",
        ]);
        let (scene, theme, notes) = scene_flags(&list, true);
        assert_eq!(scene, SceneFlags::default());
        assert_eq!(theme, None);
        assert_eq!(notes.len(), 4, "{notes:?}");
    }

    #[test]
    fn settings_pages_are_named_in_any_case() {
        assert_eq!(
            settings_target("General"),
            Some(SettingsTarget::Page(Page::General))
        );
        assert_eq!(
            settings_target("PLUGINS"),
            Some(SettingsTarget::Page(Page::Plugins))
        );
        assert_eq!(
            settings_target("store"),
            Some(SettingsTarget::Page(Page::Store))
        );
        assert_eq!(
            settings_target(" about "),
            Some(SettingsTarget::Page(Page::About))
        );
        assert_eq!(
            settings_target("plugin: screen_dimmer"),
            Some(SettingsTarget::Plugin("screen_dimmer".to_string()))
        );
        assert_eq!(settings_target(""), None);
    }

    #[test]
    fn a_peek_card_is_a_place_or_the_chip() {
        assert_eq!(peek_card("0"), Some(PeekCard::Index(0)));
        assert_eq!(peek_card(" 12 "), Some(PeekCard::Index(12)));
        assert_eq!(peek_card("CHIP"), Some(PeekCard::Chip));
        assert_eq!(peek_card("-1"), None);
        assert_eq!(peek_card(""), None);
    }

    #[test]
    fn a_theme_is_light_or_dark() {
        assert_eq!(theme_choice("light"), Some(ThemeChoice::Light));
        assert_eq!(theme_choice(" DARK "), Some(ThemeChoice::Dark));
        assert_eq!(theme_choice("system"), None);
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
