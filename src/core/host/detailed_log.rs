//! The host's side of the detailed log (`core::detailed`): the switch,
//! which is a host setting, its notices and tray tooltip, and the palette
//! commands "Mark this moment" and "Delete the files".

use super::*;
use crate::core::detailed;

/// The name the host answers prompts under; no plugin has it.
pub(super) const HOST_PROMPT: &str = "wincraft";
/// The host's prompt for the words of a mark.
pub(super) const PROMPT_MARK: u32 = 1;

const NOTICE_TITLE: &str = "WinCraft: detailed log";
const ON_TEXT: &str = "WinCraft now records keys, window titles and palette text in a detailed log. It stays on this PC. Turn it off from the palette or the tray menu.";
const STILL_ON_TEXT: &str = "The detailed log is still on. WinCraft records keys, window titles and palette text on this PC. Turn it off from the palette or the tray menu.";
const OFF_TEXT: &str =
    "WinCraft stopped the detailed log. Its files stay on this PC until you delete them.";

/// The palette's label for the switch, which says what Enter will do.
pub(super) fn toggle_label(on: bool) -> &'static str {
    if on {
        "Detailed log: Turn off"
    } else {
        "Detailed log: Turn on"
    }
}

/// The first thing a start does: a detailed log left on starts again.
pub(super) fn start_detailed_log(config: &Config) {
    if config.detailed_log {
        detailed::set(true, "at start (left on)", None);
    }
}

impl Host {
    /// The ids of the plugins that are on, for the session header.
    fn plugins_on(&self) -> Vec<String> {
        self.plugins
            .slots
            .iter()
            .filter(|slot| slot.enabled)
            .map(|slot| slot.plugin.metadata().id.to_string())
            .collect()
    }

    /// At start, after the plugins and the tray icon: the session header,
    /// and a notice that the log is still on. The log itself started
    /// before anything else (`start_detailed_log`), so the first second
    /// after a start is in it.
    pub(super) fn detailed_log_at_start(&mut self) {
        if !self.config.detailed_log {
            return;
        }
        detailed::header::write_later(self.plugins_on());
        self.tray.set_detailed(true);
        self.balloon_click = BalloonClick::Nothing;
        self.tray.balloon(NOTICE_TITLE, STILL_ON_TEXT);
    }

    /// Turns the detailed log on or off, saves the choice, and says so.
    pub(super) fn set_detailed_log(&mut self, on: bool) {
        if on == detailed::on() && on == self.config.detailed_log {
            return;
        }
        self.config.detailed_log = on;
        log::info!("detailed log: {}", if on { "on" } else { "off" });
        detailed::set(on, "by the user", Some(self.plugins_on()));
        self.tray.set_detailed(on);
        self.balloon_click = BalloonClick::Nothing;
        self.tray
            .balloon(NOTICE_TITLE, if on { ON_TEXT } else { OFF_TEXT });
    }

    /// Asks for a few words, then writes the mark (`answer_mark`). With the
    /// log off there is nothing to mark; a notice says so.
    pub(super) fn ask_mark(&mut self) {
        if !detailed::on() {
            self.balloon_click = BalloonClick::Nothing;
            self.tray.balloon(
                NOTICE_TITLE,
                "The detailed log is off, so there is nothing to mark. Turn it on first.",
            );
            return;
        }
        // The moment is the command, not the Enter after the words.
        detailed::mark("(the command; words asked for)");
        ask(Prompt {
            plugin: HOST_PROMPT.to_string(),
            id: PROMPT_MARK,
            chip: "Mark".to_string(),
            placeholder: "A few words on what just happened".to_string(),
            action: "Write the mark".to_string(),
            empty_action: Some("Write the mark without words".to_string()),
            subtitle: "Goes into the detailed log with the time".to_string(),
            ..Default::default()
        });
    }

    /// The answer to one of the host's own prompts.
    pub(super) fn host_prompt_answer(&mut self, id: u32, text: &str) {
        if id == PROMPT_MARK {
            detailed::mark(text);
        }
    }

    /// Deletes the detailed log's files, and says how many went.
    pub(super) fn delete_detailed_log(&mut self) {
        let count = detailed::delete_files();
        log::info!("detailed log: {count} files deleted");
        let text = match count {
            0 => "There were no detailed log files to delete.".to_string(),
            1 => "WinCraft deleted 1 detailed log file.".to_string(),
            n => format!("WinCraft deleted {n} detailed log files."),
        };
        self.balloon_click = BalloonClick::Nothing;
        self.tray.balloon(NOTICE_TITLE, &text);
    }
}

/// The second line of the detailed log's palette commands.
pub(super) fn subtitle(command: HostCommand) -> Option<String> {
    let text = match command {
        HostCommand::DetailedLog if detailed::on() => {
            "Recording keys, window titles and palette text on this PC"
        }
        HostCommand::DetailedLog => {
            "Records everything WinCraft does, to find a bug. Stays on this PC"
        }
        HostCommand::DetailedLogMark => "Writes the time and a few words into the detailed log",
        HostCommand::DetailedLogDelete => "Deletes every detailed log file on this PC",
        _ => return None,
    };
    Some(text.to_string())
}

/// A host message that took this long gets a "host busy" line.
pub(super) const BUSY_MS: f64 = 50.0;
/// A timer message quicker than this gets no line: timers come often.
const QUIET_TIMER_MS: f64 = 5.0;

/// The detailed log's line for one host message, `name`, that took `ms`.
pub(super) fn message_line(msg: u32, name: &str, ms: f64) -> Option<String> {
    if ms >= BUSY_MS {
        return Some(format!("host busy {ms:.0} ms in {name}"));
    }
    if msg == WM_TIMER && ms < QUIET_TIMER_MS {
        return None;
    }
    Some(format!("host: {name} {ms:.2} ms"))
}
