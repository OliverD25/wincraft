//! How the palette shows the actions that switch something on and off: the
//! icon pair and tile colour of each (fixed), and its state in words, read
//! from Windows each time the palette asks. Reading never changes anything.

use super::actions::{Awake, Command};
use super::system::System;
use super::toggles::{self, Setting};
use super::tools::Tool;
use crate::core::ui_bridge::{ToggleLook, ToggleState};

const fn look(icon_on: &'static str, icon_off: &'static str, tile: u32) -> ToggleLook {
    ToggleLook {
        icon_on,
        icon_off,
        tile: [(tile >> 16) as u8, (tile >> 8) as u8, tile as u8],
    }
}

/// The icons and tile of a command that is a toggle; None for the rest.
/// The microphone's "on" is muted, so its tile is red.
pub fn look_of(command: Command) -> Option<ToggleLook> {
    Some(match command {
        Command::Topmost => look("pin", "pin-off", 0x5a6578),
        Command::KeepAwake => look("coffee", "cup", 0xa8673a),
        Command::Toggle(Setting::DarkMode) => look("moon", "sun", 0x6b55b8),
        Command::Toggle(Setting::FileExtensions) => look("file-text", "file", 0xb8862e),
        Command::Toggle(Setting::HiddenFiles) => look("eye", "eye-off", 0xb8862e),
        Command::Toggle(Setting::DesktopIcons) => look("layout-grid", "grid-off", 0x2f7f8f),
        Command::Toggle(Setting::TaskbarAutoHide) => {
            look("panel-bottom-dashed", "panel-bottom", 0x3f6fb8)
        }
        Command::Tool(Tool::Microphone) => look("mic-off", "mic", 0xb8483a),
        _ => return None,
    })
}

/// The state line when on and when off.
fn lines(setting: Setting) -> (&'static str, &'static str) {
    match setting {
        Setting::DarkMode => ("Dark for apps and Windows", "Light for apps and Windows"),
        Setting::FileExtensions => (
            "Endings like .txt are shown",
            "Endings like .txt are hidden",
        ),
        Setting::HiddenFiles => ("Hidden files are shown", "Hidden files are not shown"),
        Setting::DesktopIcons => ("Desktop icons are shown", "Desktop icons are hidden"),
        Setting::TaskbarAutoHide => (
            "Hides until you point at it",
            "Taskbar always stays visible",
        ),
    }
}

fn either(on: bool, (when_on, when_off): (&str, &str)) -> ToggleState {
    if on {
        ToggleState::on(when_on)
    } else {
        ToggleState::off(when_off)
    }
}

/// The toggle's state now; None for a command that is not a toggle.
pub fn state_of(command: Command, system: &dyn System, awake: &Awake) -> Option<ToggleState> {
    look_of(command)?;
    Some(match command {
        Command::Topmost => match system.target_window().and_then(|hwnd| system.window(hwnd)) {
            Some(window) => {
                let title = window.title.trim();
                let title = if title.is_empty() {
                    "The window"
                } else {
                    title
                };
                either(
                    window.topmost,
                    (
                        &format!("{title} stays on top"),
                        &format!("{title} is not on top"),
                    ),
                )
            }
            None => ToggleState::without_switch("No window selected"),
        },
        Command::KeepAwake => either(awake.is_on(), ("PC stays awake", "PC sleeps as usual")),
        Command::Toggle(setting) => match toggles::read(system, setting) {
            Some(values) => either(setting.is_on(&values), lines(setting)),
            None => ToggleState::unknown(),
        },
        Command::Tool(Tool::Microphone) => match system.microphone() {
            Ok((muted, _)) => either(muted, ("Microphone is muted", "Microphone is on")),
            Err(_) => ToggleState::unknown(),
        },
        _ => ToggleState::unknown(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::actions::{entry, ENTRIES};
    use super::super::system::tests::Fake;
    use super::super::toggles::{ADVANCED, PERSONALIZE};
    use super::*;
    use crate::ui::widgets::icons::Icon;

    fn state(id: u32, fake: &Fake, awake: &Awake) -> ToggleState {
        state_of(entry(id).expect("entry").command, fake, awake).expect("a toggle")
    }

    #[test]
    fn the_eight_toggles_have_a_look_and_their_icons_are_drawn() {
        let toggles: Vec<_> = ENTRIES
            .iter()
            .filter_map(|entry| look_of(entry.command).map(|look| (entry.label, look)))
            .collect();
        assert_eq!(toggles.len(), 8, "{toggles:?}");
        for (label, look) in &toggles {
            assert!(Icon::named(look.icon_on).is_some(), "{label}");
            assert!(Icon::named(look.icon_off).is_some(), "{label}");
            assert_ne!(look.icon_on, look.icon_off, "{label}");
        }
        let mic = look_of(Command::Tool(Tool::Microphone)).unwrap();
        assert_eq!(mic.tile, [0xb8, 0x48, 0x3a], "muted is red");
        assert_eq!(look_of(Command::Tool(Tool::CopyIp)), None);
    }

    #[test]
    fn a_registry_setting_says_its_state_in_words() {
        let mut fake = Fake::default();
        let awake = Awake::default();
        fake.set_dword(PERSONALIZE, "AppsUseLightTheme", 0);
        fake.set_dword(ADVANCED, "Hidden", 2);
        fake.set_dword(ADVANCED, "HideFileExt", 0);
        assert_eq!(
            state(20, &fake, &awake),
            ToggleState::on("Dark for apps and Windows")
        );
        assert_eq!(
            state(23, &fake, &awake),
            ToggleState::off("Hidden files are not shown")
        );
        assert_eq!(
            state(24, &fake, &awake),
            ToggleState::on("Endings like .txt are shown")
        );
        fake.set_dword(PERSONALIZE, "AppsUseLightTheme", 1);
        fake.set_dword(ADVANCED, "Hidden", 1);
        fake.set_dword(ADVANCED, "HideFileExt", 1);
        assert_eq!(
            state(20, &fake, &awake),
            ToggleState::off("Light for apps and Windows")
        );
        assert_eq!(
            state(23, &fake, &awake),
            ToggleState::on("Hidden files are shown")
        );
        assert_eq!(
            state(24, &fake, &awake),
            ToggleState::off("Endings like .txt are hidden")
        );
    }

    #[test]
    fn the_desktop_and_the_taskbar_say_their_state_or_that_it_is_unknown() {
        let mut fake = Fake::default();
        let awake = Awake::default();
        fake.icons = Some(true);
        fake.taskbar = 0x3;
        assert_eq!(
            state(21, &fake, &awake),
            ToggleState::on("Desktop icons are shown")
        );
        assert_eq!(
            state(22, &fake, &awake),
            ToggleState::on("Hides until you point at it")
        );
        fake.icons = Some(false);
        fake.taskbar = 0x2;
        assert_eq!(
            state(21, &fake, &awake),
            ToggleState::off("Desktop icons are hidden")
        );
        assert_eq!(
            state(22, &fake, &awake),
            ToggleState::off("Taskbar always stays visible")
        );
        fake.icons = None;
        assert_eq!(state(21, &fake, &awake), ToggleState::unknown());
        assert_eq!(state(21, &fake, &awake).line, "State unknown");
    }

    #[test]
    fn the_microphone_is_on_when_muted_and_unknown_without_one() {
        let mut fake = Fake::default();
        let awake = Awake::default();
        fake.microphone = Some(true);
        assert_eq!(
            state(34, &fake, &awake),
            ToggleState::on("Microphone is muted")
        );
        fake.microphone = Some(false);
        assert_eq!(
            state(34, &fake, &awake),
            ToggleState::off("Microphone is on")
        );
        fake.microphone = None;
        assert_eq!(state(34, &fake, &awake), ToggleState::unknown());
    }

    #[test]
    fn keep_awake_says_whether_the_pc_stays_awake() {
        let fake = Fake::default();
        let mut awake = Awake::default();
        assert_eq!(
            state(10, &fake, &awake),
            ToggleState::off("PC sleeps as usual")
        );
        awake.since = Some(0);
        assert_eq!(state(10, &fake, &awake), ToggleState::on("PC stays awake"));
    }

    #[test]
    fn always_on_top_names_the_window_or_says_there_is_none() {
        let awake = Awake::default();
        let fake = Fake::default();
        assert_eq!(
            state(1, &fake, &awake),
            ToggleState::without_switch("No window selected")
        );
        assert_eq!(state(1, &fake, &awake).on, None);
        let mut fake = Fake::default().with_window(7);
        assert_eq!(
            state(1, &fake, &awake),
            ToggleState::off("Notes is not on top")
        );
        fake.windows.get_mut(&7).unwrap().topmost = true;
        assert_eq!(
            state(1, &fake, &awake),
            ToggleState::on("Notes stays on top")
        );
        fake.target = Some(8);
        assert_eq!(
            state(1, &fake, &awake),
            ToggleState::without_switch("No window selected"),
            "a window that is gone counts as none"
        );
    }

    #[test]
    fn a_command_that_is_not_a_toggle_has_no_state() {
        let fake = Fake::default();
        for entry in ENTRIES
            .iter()
            .filter(|entry| look_of(entry.command).is_none())
        {
            assert!(
                state_of(entry.command, &fake, &Awake::default()).is_none(),
                "{}",
                entry.label
            );
        }
    }
}
