//! The host's side of the detailed log (`core::detailed`): the switch,
//! which is a host setting, its notices and tray tooltip, and the palette
//! commands "Mark this moment" and "Delete the files".

use super::*;
use crate::core::detailed;
use crate::core::ui_bridge::ToggleLook;

/// The name the host answers prompts under; no plugin has it.
pub(super) const HOST_PROMPT: &str = "wincraft";
/// The host's prompt for the words of a mark.
pub(super) const PROMPT_MARK: u32 = 1;

const NOTICE_TITLE: &str = "WinCraft: detailed log";
const ON_TEXT: &str = "WinCraft now records keys, window titles and palette text in a detailed log. It stays on this PC. Turn it off from the palette or the tray menu.";
const STILL_ON_TEXT: &str = "The detailed log is still on. WinCraft records keys, window titles and palette text on this PC. Turn it off from the palette or the tray menu.";
const OFF_TEXT: &str =
    "WinCraft stopped the detailed log. Its files stay on this PC until you delete them.";

/// The switch's title in the palette. It never changes with the state, so
/// search and recall stay stable.
pub(super) const TOGGLE_TITLE: &str = "Detailed log";
/// Words that find the switch: what the old titles said, and what it is.
pub(super) const TOGGLE_WORDS: &str = "turn on turn off toggle debug logging record";
const ON_LINE: &str = "Recording keys, window titles and palette text on this PC";
const OFF_LINE: &str = "Off. Nothing is recorded";

/// The switch's row look. The tile is red when on, because it records.
pub(super) const fn toggle_look() -> ToggleLook {
    ToggleLook {
        icon_on: "circle-dot",
        icon_off: "circle",
        tile: [0xb8, 0x48, 0x3a],
    }
}

/// The switch's state in the palette, from the log's state.
pub(super) fn toggle_state(on: bool) -> ToggleState {
    if on {
        ToggleState::on(ON_LINE)
    } else {
        ToggleState::off(OFF_LINE)
    }
}

/// The row look of a host command that is a toggle.
pub(super) fn host_toggle_look(command: HostCommand) -> Option<ToggleLook> {
    (command == HostCommand::DetailedLog).then(toggle_look)
}

/// Every host toggle command and its state now.
pub(super) fn host_toggle_states() -> Vec<(CommandId, ToggleState)> {
    vec![(
        CommandId::Host(HostCommand::DetailedLog),
        toggle_state(detailed::on()),
    )]
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

    /// Saves a report folder and opens it in File Explorer.
    pub(super) fn save_detailed_report(&mut self) {
        match detailed::save_report() {
            Ok(folder) => {
                log::info!("detailed log: report saved in {}", folder.display());
                let path = wide(&folder.to_string_lossy());
                unsafe {
                    ShellExecuteW(
                        std::ptr::null_mut(),
                        wide("open").as_ptr(),
                        path.as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                        SW_SHOWNORMAL,
                    )
                };
            }
            Err(err) => {
                log::warn!("detailed log: the report could not be saved: {err}");
                self.balloon_click = BalloonClick::Nothing;
                self.tray.balloon(
                    NOTICE_TITLE,
                    "WinCraft could not save the report. The log file says why.",
                );
            }
        }
    }

    /// Deletes the detailed log's files, and says how many went.
    pub(super) fn delete_detailed_log(&mut self) {
        let count = detailed::delete_files();
        log::info!("detailed log: {count} files deleted");
        let text = match count {
            0 => "There were no detailed log files to delete.".to_string(),
            1 => "WinCraft deleted 1 detailed log file or report.".to_string(),
            n => format!("WinCraft deleted {n} detailed log files and reports."),
        };
        self.balloon_click = BalloonClick::Nothing;
        self.tray.balloon(NOTICE_TITLE, &text);
    }
}

/// The second line of the detailed log's palette commands.
pub(super) fn subtitle(command: HostCommand) -> Option<String> {
    let text = match command {
        HostCommand::DetailedLog if detailed::on() => ON_LINE,
        HostCommand::DetailedLog => OFF_LINE,
        HostCommand::DetailedLogMark => "Writes the time and a few words into the detailed log",
        HostCommand::DetailedLogReport => {
            "Copies the logs and settings into one folder and opens it, to hand over"
        }
        HostCommand::DetailedLogDelete => "Deletes every detailed log file and report on this PC",
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

/// Runs `call`, a call into a plugin, and writes to the detailed log how
/// long it took: "plugin <id>: <what> N ms". A call quicker than
/// `quiet_below_ms` gets no line (timers come often). Off, it only runs
/// `call`; `what` is not even built.
pub(super) fn plugin_call<R>(
    plugin: &mut dyn WinCraftPlugin,
    what: impl FnOnce() -> String,
    quiet_below_ms: f64,
    call: impl FnOnce(&mut dyn WinCraftPlugin) -> R,
) -> R {
    if !detailed::on() {
        return call(plugin);
    }
    let id = plugin.metadata().id;
    let began = std::time::Instant::now();
    let result = call(plugin);
    let ms = began.elapsed().as_secs_f64() * 1000.0;
    if let Some(line) = plugin_line(id, &what(), ms, quiet_below_ms) {
        detailed::write(line);
    }
    result
}

/// A plugin call at or over this many ms is called slow in its line.
pub(super) const SLOW_PLUGIN_MS: f64 = 20.0;

pub(super) fn plugin_line(id: &str, what: &str, ms: f64, quiet_below_ms: f64) -> Option<String> {
    if ms < quiet_below_ms {
        return None;
    }
    let slow = if ms >= SLOW_PLUGIN_MS { " SLOW" } else { "" };
    Some(format!("plugin {id}: {what} {ms:.2} ms{slow}"))
}

/// The `quiet_below_ms` for timer messages.
pub(super) const QUIET_PLUGIN_TIMER_MS: f64 = QUIET_TIMER_MS;
