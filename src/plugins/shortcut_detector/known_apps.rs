//! Common apps and the global hotkeys they register out of the box. Windows
//! cannot say which app holds a hotkey, so when a taken combination is one
//! of these and the app is running, the detector names it as a likely
//! owner: the user may have changed the key in that app.
//!
//! Only keys an app registers for the whole desktop belong here, not keys
//! that work inside its own window, and not keys read through a keyboard
//! hook (which never show as taken). A wrong entry is worse than a missing
//! one. To add an app, see "Adding an app" in this plugin's README.

use crate::core::hotkeys;
use crate::core::traits::Hotkey;

#[derive(Debug)]
pub struct KnownApp {
    pub name: &'static str,
    /// Program file names, compared without case.
    pub exes: &'static [&'static str],
    /// Default keys and what each does.
    pub keys: &'static [(&'static str, &'static str)],
    /// Where in the app the user changes them.
    pub change: &'static str,
}

pub const KNOWN_APPS: &[KnownApp] = &[
    KnownApp {
        name: "Windows Terminal",
        exes: &["WindowsTerminal.exe"],
        keys: &[("Win+`", "Open the Quake mode window")],
        change: "Windows Terminal: Settings \u{2192} Actions \u{2192} Quake mode",
    },
    KnownApp {
        name: "PowerToys Run",
        exes: &["PowerToys.PowerLauncher.exe"],
        keys: &[("Alt+Space", "Open PowerToys Run")],
        change: "PowerToys: Settings \u{2192} PowerToys Run \u{2192} Activation shortcut",
    },
    KnownApp {
        name: "Flow Launcher",
        exes: &["Flow.Launcher.exe"],
        keys: &[("Alt+Space", "Open Flow Launcher")],
        change: "Flow Launcher: Settings \u{2192} Hotkey \u{2192} Flow Launcher Hotkey",
    },
    KnownApp {
        name: "Wox",
        exes: &["Wox.exe"],
        keys: &[("Alt+Space", "Open Wox")],
        change: "Wox: Settings \u{2192} Hotkey",
    },
    KnownApp {
        name: "ueli",
        exes: &["ueli.exe"],
        keys: &[("Alt+Space", "Open ueli")],
        change: "ueli: Settings \u{2192} General \u{2192} Hotkey",
    },
    KnownApp {
        name: "Raycast",
        exes: &["Raycast.exe"],
        keys: &[("Alt+Space", "Open Raycast")],
        change: "Raycast: Settings \u{2192} General \u{2192} Raycast Hotkey",
    },
    KnownApp {
        name: "ChatGPT",
        exes: &["ChatGPT.exe"],
        keys: &[("Alt+Space", "Open the ChatGPT companion window")],
        change: "ChatGPT: Settings \u{2192} General \u{2192} Keyboard shortcut",
    },
    KnownApp {
        name: "Claude",
        exes: &["claude.exe"],
        keys: &[("Ctrl+Alt+Space", "Open Claude quick entry")],
        change: "Claude: Settings \u{2192} General \u{2192} Quick entry shortcut",
    },
    KnownApp {
        name: "Keypirinha",
        exes: &["keypirinha.exe", "keypirinha-x64.exe", "keypirinha-x86.exe"],
        keys: &[("Win+Ctrl+K", "Open Keypirinha")],
        change: "Keypirinha: tray menu \u{2192} Configure Keypirinha \u{2192} hotkey_run",
    },
    KnownApp {
        name: "Ditto",
        exes: &["Ditto.exe"],
        keys: &[("Ctrl+`", "Open the Ditto clipboard list")],
        change: "Ditto: tray menu \u{2192} Options \u{2192} Keyboard Shortcuts",
    },
    KnownApp {
        name: "KeePass",
        exes: &["KeePass.exe"],
        keys: &[("Ctrl+Alt+A", "Type the user name and password (auto-type)")],
        change: "KeePass: Tools \u{2192} Options \u{2192} Integration \u{2192} Global auto-type",
    },
    KnownApp {
        name: "1Password",
        exes: &["1Password.exe"],
        keys: &[("Ctrl+Shift+Space", "Open 1Password Quick Access")],
        change: "1Password: Settings \u{2192} General \u{2192} Keyboard shortcuts",
    },
    KnownApp {
        name: "ShareX",
        exes: &["ShareX.exe"],
        keys: &[
            ("Ctrl+PrintScreen", "Capture a region"),
            ("Alt+PrintScreen", "Capture the active window"),
            ("Shift+PrintScreen", "Start or stop a screen recording"),
            ("Ctrl+Shift+PrintScreen", "Start or stop a GIF recording"),
        ],
        change: "ShareX: main window \u{2192} Hotkey settings",
    },
    KnownApp {
        name: "Greenshot",
        exes: &["Greenshot.exe"],
        keys: &[
            ("Alt+PrintScreen", "Capture the active window"),
            ("Ctrl+PrintScreen", "Capture the full screen"),
            ("Shift+PrintScreen", "Capture the last region again"),
        ],
        change: "Greenshot: tray menu \u{2192} Preferences \u{2192} General \u{2192} Hotkeys",
    },
    KnownApp {
        name: "NVIDIA overlay",
        exes: &["NVIDIA Share.exe", "NVIDIA Overlay.exe"],
        keys: &[
            ("Alt+Z", "Open the NVIDIA overlay"),
            ("Alt+F9", "Start or stop recording"),
            ("Alt+F10", "Save the last minutes (Instant Replay)"),
            ("Alt+F1", "Take a screenshot"),
        ],
        change: "NVIDIA overlay: press Alt+Z \u{2192} Settings \u{2192} Keyboard shortcuts",
    },
    KnownApp {
        name: "AMD Software",
        exes: &["RadeonSoftware.exe"],
        keys: &[
            ("Alt+R", "Open AMD Software"),
            ("Ctrl+Shift+O", "Show or hide the performance overlay"),
            ("Ctrl+Shift+E", "Start or stop recording"),
            ("Ctrl+Shift+I", "Take a screenshot"),
        ],
        change: "AMD Software: Settings (gear) \u{2192} Hotkeys",
    },
    KnownApp {
        name: "f.lux",
        exes: &["flux.exe"],
        keys: &[
            ("Alt+End", "Turn f.lux off or on"),
            ("Alt+PageUp", "Make the screen brighter"),
            ("Alt+PageDown", "Make the screen dimmer"),
        ],
        change: "f.lux: f.lux menu \u{2192} Options and Smart Lighting \u{2192} hotkeys",
    },
];

/// The first known app among `running` (lowercase program file names) that
/// registers `hotkey` by default, and what the key does there.
pub fn running_owner(
    hotkey: Hotkey,
    running: &[String],
) -> Option<(&'static KnownApp, &'static str)> {
    KNOWN_APPS.iter().find_map(|app| {
        let runs = app
            .exes
            .iter()
            .any(|exe| running.iter().any(|name| name.eq_ignore_ascii_case(exe)));
        if !runs {
            return None;
        }
        app.keys
            .iter()
            .find(|(keys, _)| hotkeys::parse(keys).is_ok_and(|parsed| parsed == hotkey))
            .map(|(_, meaning)| (app, *meaning))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Hotkey {
        hotkeys::parse(text).expect(text)
    }

    fn running(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_lowercase()).collect()
    }

    #[test]
    fn every_key_parses_and_every_app_has_a_way_to_change_it() {
        for app in KNOWN_APPS {
            assert!(!app.exes.is_empty(), "{}", app.name);
            assert!(!app.change.is_empty(), "{}", app.name);
            assert!(app.change.starts_with(app.name.split(' ').next().unwrap()));
            for (keys, meaning) in app.keys {
                let hotkey = hotkeys::parse(keys).unwrap_or_else(|err| panic!("{keys}: {err}"));
                assert_ne!(
                    hotkey.modifiers
                        & !windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_NOREPEAT,
                    0,
                    "{keys}: the scan only lists combinations with a modifier"
                );
                assert!(!meaning.is_empty());
            }
        }
    }

    #[test]
    fn only_a_running_app_is_named() {
        assert!(running_owner(key("Win+`"), &[]).is_none());
        assert!(running_owner(key("Win+`"), &running(&["notepad.exe"])).is_none());
        let (app, meaning) =
            running_owner(key("Win+`"), &running(&["WindowsTerminal.exe"])).expect("terminal");
        assert_eq!(app.name, "Windows Terminal");
        assert_eq!(meaning, "Open the Quake mode window");
        assert!(running_owner(key("Win+Alt+`"), &running(&["windowsterminal.exe"])).is_none());
    }

    #[test]
    fn of_two_running_apps_with_the_same_key_the_first_in_the_table_is_named() {
        let both = running(&["Greenshot.exe", "ShareX.exe"]);
        let (app, meaning) = running_owner(key("Ctrl+PrintScreen"), &both).expect("one");
        assert_eq!(app.name, "ShareX");
        assert_eq!(meaning, "Capture a region");
        let (app, _) =
            running_owner(key("Ctrl+PrintScreen"), &running(&["greenshot.exe"])).expect("g");
        assert_eq!(app.name, "Greenshot");
    }
}
