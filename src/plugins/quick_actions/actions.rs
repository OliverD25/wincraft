//! The actions: their palette rows, what each one does through `System`,
//! what the notice says afterwards, and how Undo puts things back. No
//! Win32 here, so all of it runs in the tests on a fake PC.

use super::geometry::{self, Goal};
use super::lists::ListKind;
use super::system::{Placement, System};
use super::toggles::{self, Setting, Values};
use super::tools::{self, Tool};
use crate::core::notice::Glyph;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Topmost,
    Window(Goal),
    KeepAwake,
    Toggle(Setting),
    Tool(Tool),
    /// Enter shows rows to pick from.
    List(ListKind),
}

/// One palette row. The id is what a hotkey bound with Ctrl+K stores, so
/// it never changes once released.
pub struct Entry {
    pub id: u32,
    pub command: Command,
    pub label: &'static str,
    pub words: &'static str,
    pub subtitle: &'static str,
    /// Not empty: Enter twice within 5 s, and this before the second.
    pub confirm: &'static str,
    /// Enter shows rows to pick from.
    pub list: bool,
}

pub const KEEP_AWAKE: u32 = 10;

pub const ENTRIES: [Entry; 23] = [
    Entry {
        id: 1,
        command: Command::Topmost,
        label: "Window: Toggle Always on Top",
        words: "pin float above topmost front stay",
        subtitle: "Keeps the window you were in above all others, or stops it",
        confirm: "",
        list: false,
    },
    Entry {
        id: 2,
        command: Command::Window(Goal::Center),
        label: "Window: Center",
        words: "centre middle move",
        subtitle: "Moves the window you were in to the middle of its screen",
        confirm: "",
        list: false,
    },
    Entry {
        id: 3,
        command: Command::Window(Goal::Size(1280, 720)),
        label: "Window: Resize to 1280 \u{d7} 720",
        words: "size 720p hd small record video",
        subtitle: "Makes the window you were in 1280 by 720 pixels, in the middle",
        confirm: "",
        list: false,
    },
    Entry {
        id: 4,
        command: Command::Window(Goal::Size(1920, 1080)),
        label: "Window: Resize to 1920 \u{d7} 1080",
        words: "size 1080p full hd record video",
        subtitle: "Makes the window you were in 1920 by 1080 pixels, in the middle",
        confirm: "",
        list: false,
    },
    Entry {
        id: 5,
        command: Command::Window(Goal::Share(80)),
        label: "Window: Resize to 80 % of the Monitor",
        words: "size big large screen percent",
        subtitle: "Makes the window you were in 80 % of its screen, in the middle",
        confirm: "",
        list: false,
    },
    Entry {
        id: KEEP_AWAKE,
        command: Command::KeepAwake,
        label: "Display: Toggle Keep Awake",
        words: "caffeine coffee sleep screen awake insomnia presentation",
        subtitle: "Keeps the PC and the screen from sleeping until you run it again",
        confirm: "",
        list: false,
    },
    Entry {
        id: 20,
        command: Command::Toggle(Setting::DarkMode),
        label: "Theme: Toggle Dark Mode",
        words: "dark night light theme black colour color",
        subtitle: "Switches apps and Windows between dark and light",
        confirm: "",
        list: false,
    },
    Entry {
        id: 21,
        command: Command::Toggle(Setting::DesktopIcons),
        label: "Desktop: Show Desktop Icons",
        // "toggle" keeps the old name findable.
        words: "toggle icons desktop clean screen share hide show presentation",
        subtitle: "Hides or shows the icons on the desktop",
        confirm: "",
        list: false,
    },
    Entry {
        id: 22,
        command: Command::Toggle(Setting::TaskbarAutoHide),
        label: "Taskbar: Toggle Auto-Hide",
        words: "taskbar autohide hide space room",
        subtitle: "Hides the taskbar until the mouse reaches it, or stops it",
        confirm: "",
        list: false,
    },
    Entry {
        id: 23,
        command: Command::Toggle(Setting::HiddenFiles),
        label: "File Explorer: Toggle Hidden Files",
        words: "hidden dotfiles show system explorer folder",
        subtitle: "Shows or hides hidden files in File Explorer",
        confirm: "",
        list: false,
    },
    Entry {
        id: 24,
        command: Command::Toggle(Setting::FileExtensions),
        label: "File Explorer: Toggle File Extensions",
        words: "extension ext type suffix explorer names",
        subtitle: "Shows or hides the end of file names, such as .txt",
        confirm: "",
        list: false,
    },
    Entry {
        id: 30,
        command: Command::Tool(Tool::CopyIp),
        label: "Network: Copy Local IP",
        words: "ip address lan wifi ethernet wsl network local ipv4",
        subtitle: "Copies this PC's address on your network, such as 192.168.1.20",
        confirm: "",
        list: false,
    },
    Entry {
        id: 31,
        command: Command::Tool(Tool::FlushDns),
        label: "Network: Flush DNS Cache",
        words: "dns flush cache clear reset internet network",
        subtitle: "Makes Windows look up web addresses afresh, as ipconfig /flushdns does",
        confirm: "",
        list: false,
    },
    Entry {
        id: 32,
        command: Command::Tool(Tool::PlainText),
        label: "Clipboard: Convert to Plain Text",
        words: "clipboard plain text paste formatting strip clean",
        subtitle: "Keeps only the text on the clipboard, without fonts, colours or links",
        confirm: "",
        list: false,
    },
    Entry {
        id: 33,
        command: Command::Tool(Tool::NewUuid),
        label: "Clipboard: Copy New UUID",
        words: "uuid guid id random unique generate",
        subtitle: "Copies a new random UUID, such as for a database key",
        confirm: "",
        list: false,
    },
    Entry {
        id: 34,
        command: Command::Tool(Tool::Microphone),
        label: "Audio: Toggle Microphone Mute",
        words: "microphone mic mute unmute audio sound call meeting",
        subtitle: "Mutes the default microphone, or turns it on again",
        confirm: "",
        list: false,
    },
    Entry {
        id: 35,
        command: Command::Tool(Tool::EmptyBin),
        label: "System: Empty Recycle Bin",
        words: "recycle bin trash empty delete clean space",
        subtitle: "Deletes everything in the Recycle Bin for good",
        confirm: "Press Enter again to empty the Recycle Bin. This cannot be undone.",
        list: false,
    },
    Entry {
        id: 36,
        command: Command::Tool(Tool::EnvVars),
        label: "Settings: Open Environment Variables",
        words: "environment variables path env system settings developer",
        subtitle: "Opens the window where you edit PATH and other variables",
        confirm: "",
        list: false,
    },
    Entry {
        id: 40,
        command: Command::List(ListKind::Ports),
        label: "Developer: Kill Process on Port",
        words: "port kill process end stop listen tcp server developer localhost",
        subtitle: "Lists the programs that listen on a port; Enter twice ends one",
        confirm: "",
        list: true,
    },
    Entry {
        id: 41,
        command: Command::List(ListKind::Output),
        label: "Audio: Switch Output Device",
        words: "audio sound output speakers headphones headset device switch",
        subtitle: "Picks the speakers or headphones that sound plays on",
        confirm: "",
        list: true,
    },
    Entry {
        id: 42,
        command: Command::List(ListKind::Sizes),
        label: "Window: Resize\u{2026}",
        words: "size resize dimensions width height pixels",
        subtitle: "Picks a size for the window you were in, from a list you can extend",
        confirm: "",
        list: true,
    },
    Entry {
        id: 43,
        command: Command::List(ListKind::Drives),
        label: "System: Eject USB Drive",
        words: "eject usb drive stick memory card safely remove unplug",
        subtitle: "Ejects a USB stick or memory card so you can unplug it safely",
        confirm: "",
        list: true,
    },
    Entry {
        id: 44,
        command: Command::List(ListKind::AwakeFor),
        label: "Display: Keep Awake for\u{2026}",
        words: "caffeine coffee sleep screen awake timer minutes hour",
        subtitle: "Keeps the PC and the screen awake for 30 minutes, 1 hour or 2 hours",
        confirm: "",
        list: true,
    },
];

pub fn entry(id: u32) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.id == id)
}

/// What Undo puts back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Undo {
    Setting(Setting, Values),
    Placement {
        hwnd: isize,
        placement: Placement,
    },
    Topmost {
        hwnd: isize,
        was: bool,
    },
    Awake(bool),
    /// The microphone was muted before (true) or on.
    Microphone(bool),
    /// The sound output device in use before, by its id.
    Output(String),
}

/// What the notice says after an action, and its Undo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub glyph: Glyph,
    pub text: String,
    pub detail: Option<String>,
    pub undo: Option<Undo>,
}

impl Outcome {
    pub fn new(glyph: Glyph, text: String, detail: Option<String>) -> Self {
        Self {
            glyph,
            text,
            detail,
            undo: None,
        }
    }

    pub fn undo(mut self, undo: Undo) -> Self {
        self.undo = Some(undo);
        self
    }
}

pub fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

fn no_window() -> Outcome {
    Outcome::new(
        Glyph::Info,
        "There is no window to change".to_string(),
        Some("Click the window you want, then run the command again.".to_string()),
    )
}

pub fn failed(what: &str, reason: String) -> Outcome {
    log::warn!("QuickActions: {what} did not change: {reason}");
    Outcome::new(
        Glyph::Warning,
        format!("{what} did not change"),
        Some(reason),
    )
}

/// The detail line under a changed setting.
fn setting_detail(setting: Setting, on: bool) -> &'static str {
    match (setting, on) {
        (Setting::DarkMode, true) => "Apps and Windows are dark now.",
        (Setting::DarkMode, false) => "Apps and Windows are light now.",
        (Setting::DesktopIcons, true) => "The desktop shows its icons again.",
        (Setting::DesktopIcons, false) => {
            "The desktop icons are hidden. Your files stay where they are."
        }
        (Setting::TaskbarAutoHide, true) => {
            "The taskbar shows when the mouse reaches the screen edge."
        }
        (Setting::TaskbarAutoHide, false) => "The taskbar stays on screen.",
        (Setting::HiddenFiles, true) => "File Explorer shows hidden files.",
        (Setting::HiddenFiles, false) => "File Explorer hides hidden files.",
        (Setting::FileExtensions, true) => "File Explorer shows names such as notes.txt.",
        (Setting::FileExtensions, false) => "File Explorer shows names such as notes.",
    }
}

/// Keep awake: on since this many milliseconds on the plugin's clock,
/// and, when it was set for a time, when it ends and the local time it
/// ends at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Awake {
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub ends_at: Option<(u16, u16)>,
}

impl Awake {
    pub fn is_on(&self) -> bool {
        self.since.is_some()
    }

    /// "On for 12 min", or "On until 15:20, 28 min left".
    pub fn line(&self, now: u64) -> Option<String> {
        let since = self.since?;
        Some(match (self.until, self.ends_at) {
            (Some(until), Some((hour, minute))) => {
                let left = until.saturating_sub(now).div_ceil(60_000);
                format!("On until {hour:02}:{minute:02}, {left} min left")
            }
            _ => awake_line(now.saturating_sub(since)),
        })
    }
}

/// "On for 12 min", for the status line and the palette.
pub fn awake_line(on_for_ms: u64) -> String {
    let minutes = on_for_ms / 60_000;
    match minutes {
        0 => "On for less than a minute".to_string(),
        1..=59 => format!("On for {minutes} min"),
        _ => format!("On for {} h {} min", minutes / 60, minutes % 60),
    }
}

/// Runs the command. `now` is the plugin's clock in milliseconds, and
/// `uuid` the UUID the inspector showed, which Copy New UUID copies. None
/// when there is nothing to say, as after opening a window.
pub fn run(
    command: Command,
    system: &mut dyn System,
    awake: &mut Awake,
    uuid: &mut Option<String>,
    now: u64,
) -> Option<Outcome> {
    Some(match command {
        Command::Topmost => topmost(system),
        Command::Window(goal) => window(system, goal),
        Command::KeepAwake => set_awake(system, awake, !awake.is_on(), now),
        Command::Toggle(setting) => toggle(system, setting),
        Command::Tool(tool) => return tools::run(tool, system, uuid),
        // The palette shows the rows instead of running it.
        Command::List(_) => return None,
    })
}

fn topmost(system: &mut dyn System) -> Outcome {
    let Some((hwnd, state)) = system
        .target_window()
        .and_then(|hwnd| system.window(hwnd).map(|state| (hwnd, state)))
    else {
        return no_window();
    };
    let on = !state.topmost;
    match system.set_topmost(hwnd, on) {
        Ok(()) => Outcome::new(
            Glyph::Pin,
            format!("Always on top {}", on_off(on)),
            Some(state.title),
        )
        .undo(Undo::Topmost {
            hwnd,
            was: state.topmost,
        }),
        Err(reason) => failed("The window", reason),
    }
}

fn window(system: &mut dyn System, goal: Goal) -> Outcome {
    let Some((hwnd, before)) = system
        .target_window()
        .and_then(|hwnd| system.window(hwnd).map(|state| (hwnd, state)))
    else {
        return no_window();
    };
    let mut state = before.clone();
    if state.maximized || state.minimized {
        system.restore(hwnd);
        match system.window(hwnd) {
            Some(now) => state = now,
            None => return no_window(),
        }
    }
    let placed = geometry::place(goal, state.outer, state.frame, state.work);
    if let Err(reason) = system.move_window(hwnd, placed.outer) {
        return failed("The window", reason);
    }
    let (w, h) = placed.size;
    let text = match goal {
        Goal::Center => "Window centered".to_string(),
        Goal::Size(..) | Goal::Share(_) => format!("Window resized to {w} \u{d7} {h}"),
    };
    let detail = if placed.shrunk {
        format!("Made smaller to fit the screen \u{b7} {}", state.title)
    } else {
        state.title
    };
    Outcome::new(Glyph::Monitor, text, Some(detail)).undo(Undo::Placement {
        hwnd,
        placement: before.placement,
    })
}

fn set_awake(system: &mut dyn System, awake: &mut Awake, on: bool, now: u64) -> Outcome {
    let was = awake.is_on();
    if let Err(reason) = system.keep_awake(on) {
        return failed("Keep awake", reason);
    }
    awake.since = on.then_some(awake.since.unwrap_or(now));
    awake.until = None;
    awake.ends_at = None;
    log::info!("QuickActions: keep awake {}", on_off(on));
    let detail = if on {
        "The PC and the screen stay awake until you run it again."
    } else {
        "The PC and the screen can sleep again."
    };
    Outcome::new(
        Glyph::Check,
        format!("Keep awake {}", on_off(on)),
        Some(detail.to_string()),
    )
    .undo(Undo::Awake(was))
}

fn toggle(system: &mut dyn System, setting: Setting) -> Outcome {
    let Some(before) = toggles::read(system, setting) else {
        return failed(
            setting.name(),
            "Windows did not say how it is set now".to_string(),
        );
    };
    let on = !setting.is_on(&before);
    let wanted = setting.values_for(on, &before);
    if let Err(reason) = toggles::write(system, setting, &wanted) {
        return failed(setting.name(), reason);
    }
    log::info!(
        "QuickActions: {} {} ({:?} -> {:?})",
        setting.name(),
        on_off(on),
        before.0,
        wanted.0
    );
    Outcome::new(
        Glyph::Settings,
        format!("{} {}", setting.name(), on_off(on)),
        Some(setting_detail(setting, on).to_string()),
    )
    .undo(Undo::Setting(setting, before))
}

/// Puts back what `undo` holds. `what` is the text of the notice that
/// offered it.
pub fn undo(
    undo: Undo,
    what: &str,
    system: &mut dyn System,
    awake: &mut Awake,
    now: u64,
) -> Outcome {
    let result = match undo {
        Undo::Setting(setting, values) => toggles::write(system, setting, &values),
        Undo::Placement { hwnd, placement } => match system.window(hwnd) {
            Some(_) => system.set_placement(hwnd, &placement),
            None => Err("The window has closed".to_string()),
        },
        Undo::Topmost { hwnd, was } => match system.window(hwnd) {
            Some(_) => system.set_topmost(hwnd, was),
            None => Err("The window has closed".to_string()),
        },
        Undo::Awake(was) => {
            let result = system.keep_awake(was);
            if result.is_ok() {
                awake.since = was.then_some(awake.since.unwrap_or(now));
                awake.until = None;
                awake.ends_at = None;
            }
            result
        }
        Undo::Microphone(was) => system.set_microphone_muted(was),
        Undo::Output(id) => system.set_output_device(&id),
    };
    match result {
        Ok(()) => {
            log::info!("QuickActions: undone: {what}");
            Outcome::new(Glyph::Info, "Undone".to_string(), Some(what.to_string()))
        }
        Err(reason) => {
            log::warn!("QuickActions: could not undo {what}: {reason}");
            Outcome::new(Glyph::Warning, "Could not undo".to_string(), Some(reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::system::tests::Fake;
    use super::super::toggles::{ADVANCED, PERSONALIZE};
    use super::*;

    #[test]
    fn every_entry_has_its_own_id_an_area_title_and_search_words() {
        let mut ids = Vec::new();
        for entry in &ENTRIES {
            assert!(!ids.contains(&entry.id), "id {} twice", entry.id);
            ids.push(entry.id);
            let (area, verb) = entry.label.split_once(": ").expect("Area: Verb");
            assert!(!area.is_empty() && !verb.is_empty(), "{}", entry.label);
            assert!(!entry.words.trim().is_empty(), "{}", entry.label);
            assert!(!entry.subtitle.is_empty(), "{}", entry.label);
            assert!(entry
                .words
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == ' '));
            assert_eq!(super::entry(entry.id).map(|e| e.label), Some(entry.label));
        }
        assert!(super::entry(999).is_none());
        for setting in Setting::ALL {
            assert!(
                ENTRIES
                    .iter()
                    .any(|entry| entry.command == Command::Toggle(setting)),
                "{setting:?} has no row"
            );
        }
    }

    #[test]
    fn the_promised_search_words_find_their_rows() {
        let words = |id: u32| entry(id).expect("an entry").words;
        for (id, word) in [
            (20, "night"),
            (20, "light"),
            (20, "theme"),
            (1, "pin"),
            (1, "float"),
            (1, "above"),
            (KEEP_AWAKE, "caffeine"),
            (KEEP_AWAKE, "sleep"),
            (KEEP_AWAKE, "screen"),
            (21, "desktop"),
            (21, "clean"),
            (21, "share"),
        ] {
            assert!(
                crate::ui::fuzzy::score_words(word, words(id)).is_some(),
                "{word} for {id}"
            );
        }
    }

    #[test]
    fn the_awake_line_counts_minutes_then_hours() {
        assert_eq!(awake_line(30_000), "On for less than a minute");
        assert_eq!(awake_line(12 * 60_000 + 5), "On for 12 min");
        assert_eq!(awake_line(125 * 60_000), "On for 2 h 5 min");
    }

    fn run_id(id: u32, fake: &mut Fake, awake: &mut Awake) -> Outcome {
        run(
            entry(id).expect("an entry").command,
            fake,
            awake,
            &mut None,
            1000,
        )
        .expect("an outcome")
    }

    #[test]
    fn a_toggle_switches_the_setting_and_undo_puts_it_back() {
        let mut fake = Fake::default();
        let mut awake = Awake::default();
        let done = run_id(20, &mut fake, &mut awake);
        assert_eq!(done.text, "Dark mode on");
        assert_eq!(done.glyph, Glyph::Settings);
        assert_eq!(fake.dword(PERSONALIZE, "AppsUseLightTheme"), Some(0));
        let back = undo(
            done.undo.expect("an undo"),
            &done.text,
            &mut fake,
            &mut awake,
            2000,
        );
        assert_eq!(back.text, "Undone");
        assert_eq!(back.detail.as_deref(), Some("Dark mode on"));
        assert_eq!(fake.dword(PERSONALIZE, "AppsUseLightTheme"), Some(1));
    }

    #[test]
    fn running_a_toggle_again_switches_it_back() {
        let mut fake = Fake::default();
        let mut awake = Awake::default();
        assert_eq!(run_id(23, &mut fake, &mut awake).text, "Hidden files on");
        assert_eq!(fake.dword(ADVANCED, "Hidden"), Some(1));
        assert_eq!(run_id(23, &mut fake, &mut awake).text, "Hidden files off");
        assert_eq!(fake.dword(ADVANCED, "Hidden"), Some(2));
        assert_eq!(run_id(24, &mut fake, &mut awake).text, "File extensions on");
        assert_eq!(
            run_id(22, &mut fake, &mut awake).text,
            "Taskbar auto-hide on"
        );
        assert_eq!(run_id(21, &mut fake, &mut awake).text, "Desktop icons off");
    }

    #[test]
    fn a_silent_desktop_says_so_and_offers_no_undo() {
        let mut fake = Fake {
            icons_ignore: true,
            ..Fake::default()
        };
        let done = run_id(21, &mut fake, &mut Awake::default());
        assert_eq!(done.text, "Desktop icons did not change");
        assert_eq!(done.glyph, Glyph::Warning);
        assert!(done.detail.expect("a reason").contains("did not answer"));
        assert!(done.undo.is_none());
        let mut blind = Fake {
            icons: None,
            ..Fake::default()
        };
        let done = run_id(21, &mut blind, &mut Awake::default());
        assert_eq!(done.text, "Desktop icons did not change");
    }

    #[test]
    fn keep_awake_goes_on_and_off_and_keeps_its_start() {
        let mut fake = Fake::default();
        let mut awake = Awake::default();
        let on =
            run(Command::KeepAwake, &mut fake, &mut awake, &mut None, 5000).expect("an outcome");
        assert_eq!(on.text, "Keep awake on");
        assert!(fake.awake);
        assert_eq!(awake.since, Some(5000));
        let off =
            run(Command::KeepAwake, &mut fake, &mut awake, &mut None, 9000).expect("an outcome");
        assert_eq!(off.text, "Keep awake off");
        assert!(!fake.awake);
        assert_eq!(awake.since, None);
        undo(
            off.undo.expect("an undo"),
            &off.text,
            &mut fake,
            &mut awake,
            9500,
        );
        assert!(fake.awake);
        assert_eq!(awake.since, Some(9500));
    }

    #[test]
    fn a_window_action_without_a_window_says_so() {
        let mut fake = Fake::default();
        for id in [1, 2, 3, 4, 5] {
            let done = run_id(id, &mut fake, &mut Awake::default());
            assert_eq!(done.text, "There is no window to change");
            assert!(done.undo.is_none());
        }
        assert!(fake.log.is_empty());
    }

    #[test]
    fn resize_moves_the_window_and_undo_puts_its_place_back() {
        let mut fake = Fake::default().with_window(7);
        let mut awake = Awake::default();
        let done = run_id(3, &mut fake, &mut awake);
        assert_eq!(done.text, "Window resized to 1280 \u{d7} 720");
        assert_eq!(done.detail.as_deref(), Some("Notes"));
        assert_eq!(fake.calls(), ["move [313, 156, 1607, 883]"]);
        let window = fake.window(7).expect("a window");
        assert_eq!(geometry::width(window.frame), 1280);
        assert_eq!(geometry::height(window.frame), 720);
        undo(
            done.undo.expect("an undo"),
            &done.text,
            &mut fake,
            &mut awake,
            0,
        );
        assert_eq!(fake.window(7).expect("a window").outer, [93, 100, 907, 707]);
    }

    #[test]
    fn a_size_too_big_for_the_screen_says_it_was_made_smaller() {
        let mut fake = Fake::default().with_window(7);
        let done = run_id(4, &mut fake, &mut Awake::default());
        assert_eq!(done.text, "Window resized to 1920 \u{d7} 1032");
        assert_eq!(
            done.detail.as_deref(),
            Some("Made smaller to fit the screen \u{b7} Notes")
        );
        let done = run_id(5, &mut fake, &mut Awake::default());
        assert_eq!(done.text, "Window resized to 1536 \u{d7} 825");
        assert_eq!(
            run_id(2, &mut fake, &mut Awake::default()).text,
            "Window centered"
        );
    }

    #[test]
    fn a_maximised_window_is_restored_first_and_undo_maximises_it_again() {
        let mut fake = Fake::default().with_window(7);
        if let Some(window) = fake.windows.get_mut(&7) {
            window.maximized = true;
            window.outer = [-8, -8, 1928, 1040];
            window.frame = [0, 0, 1920, 1032];
            window.placement.show = 3;
        }
        let mut awake = Awake::default();
        let done = run_id(2, &mut fake, &mut awake);
        assert_eq!(fake.calls()[0], "restore");
        assert_eq!(done.text, "Window centered");
        let window = fake.window(7).expect("a window");
        assert_eq!(geometry::width(window.frame), 800);
        assert!(!window.maximized);
        undo(
            done.undo.expect("an undo"),
            &done.text,
            &mut fake,
            &mut awake,
            0,
        );
        assert!(fake.window(7).expect("a window").maximized);
    }

    #[test]
    fn always_on_top_flips_and_undo_flips_it_back() {
        let mut fake = Fake::default().with_window(7);
        let mut awake = Awake::default();
        let done = run_id(1, &mut fake, &mut awake);
        assert_eq!(done.text, "Always on top on");
        assert!(fake.window(7).expect("a window").topmost);
        assert_eq!(run_id(1, &mut fake, &mut awake).text, "Always on top off");
        undo(
            done.undo.expect("an undo"),
            &done.text,
            &mut fake,
            &mut awake,
            0,
        );
        assert!(!fake.window(7).expect("a window").topmost);
    }

    #[test]
    fn a_refused_window_change_says_why() {
        let mut fake = Fake {
            refuse: true,
            ..Fake::default().with_window(7)
        };
        for id in [1, 2] {
            let done = run_id(id, &mut fake, &mut Awake::default());
            assert_eq!(done.text, "The window did not change");
            assert!(done.detail.expect("a reason").contains("administrator"));
        }
    }

    #[test]
    fn undo_on_a_closed_window_says_it_has_closed() {
        let mut fake = Fake::default().with_window(7);
        let mut awake = Awake::default();
        let done = run_id(1, &mut fake, &mut awake);
        fake.windows.clear();
        let back = undo(
            done.undo.expect("an undo"),
            &done.text,
            &mut fake,
            &mut awake,
            0,
        );
        assert_eq!(back.text, "Could not undo");
        assert_eq!(back.detail.as_deref(), Some("The window has closed"));
        let placement = Undo::Placement {
            hwnd: 7,
            placement: Placement::default(),
        };
        let back = undo(placement, "x", &mut fake, &mut awake, 0);
        assert_eq!(back.detail.as_deref(), Some("The window has closed"));
    }
}
