//! What the palette's inspector shows for each action while its row is
//! selected: the state now and what Enter will do. Read only at that
//! moment, never in the background, and it never changes anything.

use super::actions::{awake_line, Awake, Command};
use super::geometry::{self, Goal};
use super::system::System;
use super::toggles::{self, Setting};
use super::tools::{self, Tool};
use crate::core::ui_bridge::CommandDetails;

const DOT: &str = " \u{b7} ";

fn line(key: &str, value: impl Into<String>) -> (String, String) {
    (key.to_string(), value.into())
}

fn size(rect: [i32; 4]) -> String {
    format!(
        "{} \u{d7} {}",
        geometry::width(rect),
        geometry::height(rect)
    )
}

/// How the setting is now, in one word: "Dark", "Hidden".
pub fn state_word(setting: Setting, on: bool) -> &'static str {
    match (setting, on) {
        (Setting::DarkMode, true) => "Dark",
        (Setting::DarkMode, false) => "Light",
        (Setting::TaskbarAutoHide, true) => "Hides itself",
        (Setting::TaskbarAutoHide, false) => "Always shown",
        (_, true) => "Shown",
        (_, false) => "Hidden",
    }
}

fn now_then(setting: Setting, on: bool) -> String {
    let next = match (setting, on) {
        (Setting::DarkMode, true) => "Enter switches to Light",
        (Setting::DarkMode, false) => "Enter switches to Dark",
        (Setting::TaskbarAutoHide, true) => "Enter makes it always shown",
        (Setting::TaskbarAutoHide, false) => "Enter makes it hide itself",
        (_, true) => "Enter makes it hidden",
        (_, false) => "Enter makes it shown",
    };
    format!("Now: {}{DOT}{next}", state_word(setting, on))
}

pub fn details(
    command: Command,
    system: &dyn System,
    awake: &Awake,
    uuid: &mut Option<String>,
    now: u64,
) -> CommandDetails {
    let mut details = CommandDetails::default();
    match command {
        Command::Topmost | Command::Window(_) => {
            let Some(state) = system.target_window().and_then(|hwnd| system.window(hwnd)) else {
                details.status =
                    "No window to change. Click one, then open the palette.".to_string();
                return details;
            };
            details.lines.push(line("Window", state.title.clone()));
            match command {
                Command::Topmost => {
                    details.status = if state.topmost {
                        format!("Now: on top{DOT}Enter stops it")
                    } else {
                        format!("Now: not on top{DOT}Enter keeps it on top")
                    };
                }
                Command::Window(goal) => {
                    details.lines.push(line("Size now", size(state.frame)));
                    details.lines.push(line("Screen", size(state.work)));
                    let placed = geometry::place(goal, state.outer, state.frame, state.work);
                    let (w, h) = placed.size;
                    let mut status = match goal {
                        Goal::Center => "Enter moves it to the middle of the screen".to_string(),
                        _ if placed.shrunk => {
                            format!("Enter makes it {w} \u{d7} {h}, to fit the screen")
                        }
                        _ => format!("Enter makes it {w} \u{d7} {h}"),
                    };
                    if state.maximized {
                        status.push_str(", no longer maximized");
                    }
                    details.status = status;
                }
                _ => {}
            }
        }
        Command::KeepAwake => {
            details.status = match awake.since {
                Some(since) => format!(
                    "{}{DOT}Enter turns it off",
                    awake_line(now.saturating_sub(since))
                ),
                None => format!("Off{DOT}Enter keeps the PC and the screen awake"),
            };
        }
        Command::Toggle(setting) => {
            details.status = match toggles::read(system, setting) {
                Some(values) => now_then(setting, setting.is_on(&values)),
                None => "Windows does not say how it is set now".to_string(),
            };
        }
        Command::Tool(tool) => tool_details(tool, system, uuid, &mut details),
    }
    details
}

fn tool_details(
    tool: Tool,
    system: &dyn System,
    uuid: &mut Option<String>,
    details: &mut CommandDetails,
) {
    match tool {
        Tool::CopyIp => {
            let addresses = system.local_addresses();
            for address in &addresses {
                details
                    .lines
                    .push(line(address.kind.name(), address.ip.clone()));
            }
            details.status = match addresses.first() {
                Some(first) => format!("Enter copies {}", first.ip),
                None => "No network address found".to_string(),
            };
        }
        Tool::FlushDns => {
            details.status =
                "Enter makes Windows forget the web addresses it looked up".to_string();
        }
        Tool::PlainText => match system.clipboard_text() {
            Some(text) => {
                details.lines.push(line("Text", tools::preview(&text, 40)));
                details.lines.push(line(
                    "Length",
                    format!("{} characters", text.chars().count()),
                ));
                details.status = "Enter keeps only the text, without formatting".to_string();
            }
            None => details.status = "The clipboard has no text".to_string(),
        },
        Tool::NewUuid => match tools::next_uuid(system, uuid) {
            Ok(made) => {
                details.lines.push(line("UUID", made));
                details.status = "Enter copies this UUID".to_string();
            }
            Err(reason) => details.status = reason,
        },
        Tool::Microphone => match system.microphone() {
            Ok((muted, name)) => {
                if !name.is_empty() {
                    details.lines.push(line("Microphone", name));
                }
                details.status = if muted {
                    format!("Now: muted{DOT}Enter turns it on")
                } else {
                    format!("Now: on{DOT}Enter mutes it")
                };
            }
            Err(reason) => details.status = reason,
        },
        Tool::EmptyBin => match system.recycle_bin() {
            Some((0, _)) => details.status = "The Recycle Bin is empty".to_string(),
            Some((items, bytes)) => {
                details.lines.push(line("Items", tools::items_text(items)));
                details.lines.push(line("Size", tools::size_text(bytes)));
                details.status = "Enter twice empties it. This cannot be undone.".to_string();
            }
            None => details.status = "Windows does not say what is in it".to_string(),
        },
        Tool::EnvVars => {
            details.status = "Enter opens the Environment Variables window".to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::actions::{entry, ENTRIES};
    use super::super::network::{Address, Kind};
    use super::super::system::tests::Fake;
    use super::super::toggles::PERSONALIZE;
    use super::*;

    fn status(id: u32, fake: &Fake) -> String {
        let command = entry(id).expect("an entry").command;
        details(command, fake, &Awake::default(), &mut None, 0).status
    }

    #[test]
    fn every_action_says_something_and_changes_nothing() {
        let fake = Fake::default().with_window(7);
        for entry in &ENTRIES {
            let shown = details(entry.command, &fake, &Awake::default(), &mut None, 0);
            assert!(!shown.status.is_empty(), "{}", entry.label);
        }
        assert!(fake.log.is_empty(), "{:?}", fake.log);
    }

    #[test]
    fn a_setting_says_how_it_is_and_what_enter_does() {
        let mut fake = Fake::default();
        assert_eq!(
            status(20, &fake),
            "Now: Light \u{b7} Enter switches to Dark"
        );
        fake.set_dword(PERSONALIZE, "AppsUseLightTheme", 0);
        assert_eq!(
            status(20, &fake),
            "Now: Dark \u{b7} Enter switches to Light"
        );
        assert_eq!(status(21, &fake), "Now: Shown \u{b7} Enter makes it hidden");
        assert_eq!(status(23, &fake), "Now: Hidden \u{b7} Enter makes it shown");
        assert_eq!(
            status(22, &fake),
            "Now: Always shown \u{b7} Enter makes it hide itself"
        );
        fake.taskbar = 0x3;
        assert_eq!(
            status(22, &fake),
            "Now: Hides itself \u{b7} Enter makes it always shown"
        );
        fake.icons = None;
        assert_eq!(status(21, &fake), "Windows does not say how it is set now");
    }

    #[test]
    fn window_actions_name_the_window_its_size_and_the_new_one() {
        let mut fake = Fake::default();
        assert!(status(1, &fake).starts_with("No window to change"));
        fake = fake.with_window(7);
        assert_eq!(
            status(1, &fake),
            "Now: not on top \u{b7} Enter keeps it on top"
        );
        let shown = details(
            entry(3).expect("an entry").command,
            &fake,
            &Awake::default(),
            &mut None,
            0,
        );
        assert_eq!(shown.status, "Enter makes it 1280 \u{d7} 720");
        assert_eq!(
            shown.lines,
            [
                line("Window", "Notes"),
                line("Size now", "800 \u{d7} 600"),
                line("Screen", "1920 \u{d7} 1032"),
            ]
        );
        assert_eq!(
            status(4, &fake),
            "Enter makes it 1920 \u{d7} 1032, to fit the screen"
        );
        assert_eq!(
            status(2, &fake),
            "Enter moves it to the middle of the screen"
        );
        if let Some(window) = fake.windows.get_mut(&7) {
            window.topmost = true;
            window.maximized = true;
        }
        assert_eq!(status(1, &fake), "Now: on top \u{b7} Enter stops it");
        assert!(status(5, &fake).ends_with(", no longer maximized"));
    }

    #[test]
    fn keep_awake_says_how_long() {
        let fake = Fake::default();
        let command = entry(10).expect("an entry").command;
        let on = Awake { since: Some(1000) };
        let shown = details(command, &fake, &on, &mut None, 1000 + 12 * 60_000);
        assert_eq!(shown.status, "On for 12 min \u{b7} Enter turns it off");
        assert!(status(10, &fake).starts_with("Off"));
    }

    #[test]
    fn the_tools_show_what_enter_will_use() {
        let mut fake = Fake::default();
        assert_eq!(status(30, &fake), "No network address found");
        fake.addresses = vec![
            Address {
                kind: Kind::WiFi,
                adapter: "Wi-Fi".to_string(),
                ip: "192.168.1.20".to_string(),
            },
            Address {
                kind: Kind::Wsl,
                adapter: "vEthernet (WSL)".to_string(),
                ip: "172.28.160.1".to_string(),
            },
        ];
        let shown = details(
            entry(30).expect("an entry").command,
            &fake,
            &Awake::default(),
            &mut None,
            0,
        );
        assert_eq!(shown.status, "Enter copies 192.168.1.20");
        assert_eq!(
            shown.lines,
            [line("Wi-Fi", "192.168.1.20"), line("WSL", "172.28.160.1")]
        );

        assert_eq!(status(32, &fake), "The clipboard has no text");
        fake.clipboard = Some("Hello\nworld".to_string());
        let shown = details(
            entry(32).expect("an entry").command,
            &fake,
            &Awake::default(),
            &mut None,
            0,
        );
        assert_eq!(shown.lines[0], line("Text", "Hello world"));
        assert_eq!(shown.lines[1], line("Length", "11 characters"));

        let mut uuid = None;
        let shown = details(
            entry(33).expect("an entry").command,
            &fake,
            &Awake::default(),
            &mut uuid,
            0,
        );
        assert_eq!(Some(&shown.lines[0].1), uuid.as_ref(), "kept for Enter");

        assert_eq!(status(34, &fake), "Now: on \u{b7} Enter mutes it");
        fake.microphone = Some(true);
        assert_eq!(status(34, &fake), "Now: muted \u{b7} Enter turns it on");
        fake.microphone = None;
        assert_eq!(status(34, &fake), "No microphone is connected");

        assert_eq!(status(35, &fake), "The Recycle Bin is empty");
        fake.bin = Some((3, 2048));
        let shown = details(
            entry(35).expect("an entry").command,
            &fake,
            &Awake::default(),
            &mut None,
            0,
        );
        assert_eq!(
            shown.lines,
            [line("Items", "3 items"), line("Size", "2 KB")]
        );
        assert!(shown.status.contains("cannot be undone"));
        fake.bin = None;
        assert_eq!(status(35, &fake), "Windows does not say what is in it");
        assert!(fake.log.is_empty());
    }
}
