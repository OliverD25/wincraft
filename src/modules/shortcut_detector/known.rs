use crate::core::hotkeys;
use crate::core::traits::Hotkey;

/// Global Windows 11 desktop shortcuts: ones that work anywhere, not shortcuts
/// that belong to a single app. Every entry is parsed by a unit test, so a typo
/// here fails the build rather than quietly dropping a row from the table.
pub const WINDOWS_SHORTCUTS: &[(&str, &str)] = &[
    ("Win+A", "Open Quick Settings"),
    ("Win+B", "Focus the notification area"),
    ("Win+C", "Open Copilot"),
    ("Win+D", "Show or hide the desktop"),
    ("Win+E", "Open File Explorer"),
    ("Win+F", "Open Feedback Hub"),
    ("Win+G", "Open Xbox Game Bar"),
    ("Win+H", "Start voice typing"),
    ("Win+I", "Open Settings"),
    ("Win+K", "Open Cast"),
    ("Win+L", "Lock the PC"),
    ("Win+M", "Minimise all windows"),
    ("Win+N", "Open the notification centre"),
    ("Win+O", "Lock the device orientation"),
    ("Win+P", "Choose a presentation display mode"),
    ("Win+Q", "Open search"),
    ("Win+R", "Open the Run box"),
    ("Win+S", "Open search"),
    ("Win+T", "Step through the taskbar buttons"),
    ("Win+U", "Open Accessibility settings"),
    ("Win+V", "Open clipboard history"),
    ("Win+W", "Open Widgets"),
    ("Win+X", "Open the Quick Link menu"),
    ("Win+Z", "Open the snap layouts"),
    ("Win+0", "Open the tenth taskbar app"),
    ("Win+1", "Open the first taskbar app"),
    ("Win+2", "Open the second taskbar app"),
    ("Win+3", "Open the third taskbar app"),
    ("Win+4", "Open the fourth taskbar app"),
    ("Win+5", "Open the fifth taskbar app"),
    ("Win+6", "Open the sixth taskbar app"),
    ("Win+7", "Open the seventh taskbar app"),
    ("Win+8", "Open the eighth taskbar app"),
    ("Win+9", "Open the ninth taskbar app"),
    ("Win+Tab", "Open Task view"),
    ("Win+Space", "Switch the keyboard layout"),
    ("Win+Enter", "Open Narrator"),
    ("Win+Pause", "Open the System page in Settings"),
    ("Win+PrintScreen", "Save a screenshot to the Screenshots folder"),
    ("Win+Up", "Maximise the window"),
    ("Win+Down", "Restore or minimise the window"),
    ("Win+Left", "Snap the window to the left"),
    ("Win+Right", "Snap the window to the right"),
    ("Win+Home", "Minimise everything except the active window"),
    ("Win+,", "Peek at the desktop"),
    ("Win+.", "Open the emoji panel"),
    ("Win+;", "Open the emoji panel"),
    ("Win+=", "Zoom in with Magnifier"),
    ("Win+-", "Zoom out with Magnifier"),
    ("Win+/", "Start IME reconversion"),
    ("Win+Shift+S", "Take a screenshot with Snipping Tool"),
    ("Win+Shift+M", "Restore the minimised windows"),
    ("Win+Shift+V", "Step through the notifications"),
    ("Win+Shift+Left", "Move the window to the monitor on the left"),
    ("Win+Shift+Right", "Move the window to the monitor on the right"),
    ("Win+Shift+Up", "Stretch the window to the top and bottom"),
    ("Win+Shift+Down", "Restore or minimise the window vertically"),
    ("Win+Ctrl+D", "Add a virtual desktop"),
    ("Win+Ctrl+F4", "Close the current virtual desktop"),
    ("Win+Ctrl+Left", "Switch to the virtual desktop on the left"),
    ("Win+Ctrl+Right", "Switch to the virtual desktop on the right"),
    ("Win+Ctrl+Enter", "Turn Narrator on"),
    ("Win+Ctrl+O", "Open the on-screen keyboard"),
    ("Win+Ctrl+Q", "Open Quick Assist"),
    ("Win+Ctrl+C", "Turn the colour filters on or off"),
    ("Win+Ctrl+Shift+B", "Restart the graphics driver"),
    ("Win+Alt+D", "Show or hide the date and time on the desktop"),
    ("Win+Alt+B", "Turn HDR on or off"),
    ("Win+Alt+G", "Record the last moments with Game Bar"),
    ("Win+Alt+R", "Start or stop recording with Game Bar"),
    ("Win+Alt+K", "Mute or unmute the microphone in a call"),
    ("Win+Alt+PrintScreen", "Screenshot the active window with Game Bar"),
    ("Ctrl+Shift+Esc", "Open Task Manager"),
    ("Ctrl+Alt+Delete", "Open the security options screen"),
    ("Ctrl+Esc", "Open Start"),
    ("Ctrl+Alt+Tab", "Show the open apps and keep them shown"),
    ("Alt+Tab", "Switch between the open apps"),
    ("Alt+Shift+Tab", "Switch between the open apps, backwards"),
    ("Alt+Esc", "Step through the windows in the order they were opened"),
    ("Alt+F4", "Close the active window"),
    ("Alt+Space", "Open the window menu of the active window"),
];

/// Windows keeps these for itself: no program can ever register them, so the
/// detector says "reserved" instead of "free" when a probe unexpectedly wins.
pub const RESERVED_SHORTCUTS: &[&str] = &["Win+L", "Ctrl+Alt+Delete"];

pub fn meaning(hotkey: Hotkey) -> Option<&'static str> {
    lookup(WINDOWS_SHORTCUTS.iter().map(|(keys, text)| (*keys, *text)), hotkey)
}

pub fn is_reserved(hotkey: Hotkey) -> bool {
    lookup(RESERVED_SHORTCUTS.iter().map(|keys| (*keys, "")), hotkey).is_some()
}

fn lookup(
    entries: impl Iterator<Item = (&'static str, &'static str)>,
    hotkey: Hotkey,
) -> Option<&'static str> {
    for (keys, text) in entries {
        match hotkeys::parse(keys) {
            Ok(parsed) if parsed == hotkey => return Some(text),
            Ok(_) => {}
            Err(err) => log::debug!("built-in shortcut \"{keys}\" does not parse: {err}"),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_shortcut_parses() {
        for (keys, meaning) in WINDOWS_SHORTCUTS {
            let hotkey = hotkeys::parse(keys)
                .unwrap_or_else(|err| panic!("\"{keys}\" ({meaning}) does not parse: {err}"));
            assert_eq!(&hotkeys::format(hotkey), keys, "\"{keys}\" is not canonical");
        }
        for keys in RESERVED_SHORTCUTS {
            hotkeys::parse(keys).unwrap_or_else(|err| panic!("\"{keys}\" does not parse: {err}"));
        }
    }

    #[test]
    fn no_built_in_shortcut_is_listed_twice() {
        let mut seen: Vec<Hotkey> = Vec::new();
        for (keys, _) in WINDOWS_SHORTCUTS {
            let hotkey = hotkeys::parse(keys).expect(keys);
            assert!(!seen.contains(&hotkey), "\"{keys}\" is listed twice");
            seen.push(hotkey);
        }
    }

    #[test]
    fn finds_a_meaning_and_a_reserved_key() {
        let explorer = hotkeys::parse("Win+E").unwrap();
        assert_eq!(meaning(explorer), Some("Open File Explorer"));
        assert!(!is_reserved(explorer));
        assert!(is_reserved(hotkeys::parse("Win+L").unwrap()));
    }
}
