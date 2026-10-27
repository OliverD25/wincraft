use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::danger::danger;
use super::process::{open_terminal, Process};
use super::shell::{pwsh_installed, terminal_tab, Shell};
use crate::core::config;
use crate::search::{
    Action, Choice, Completion, Context, Glyph, IconRef, Query, Reply, ResultItem, SearchProvider,
    Tone,
};
use crate::ui::fuzzy;

const HISTORY_LENGTH: usize = 100;
/// How long a "press Enter again" stays armed.
const CONFIRM_FOR: Duration = Duration::from_secs(5);
const RUN: &str = "run";
const TERMINAL: &str = "terminal";

/// A dangerous command waiting for its second Enter.
struct Armed {
    command: String,
    to_terminal: bool,
    at: Instant,
}

/// Runs a command typed after `>` in the shell chosen in the settings, with
/// its output streaming into the palette. Typing never runs anything; Enter
/// does, and Ctrl+Enter hands the command to a real terminal instead.
pub struct Terminal {
    history: Vec<String>,
    process: Option<Process>,
    /// A command that could not be started, and why.
    failure: Option<(String, String)>,
    armed: Option<Armed>,
    /// The palette should ask again: a confirmation was cancelled.
    dirty: bool,
    /// The last answer was the output list, not the "Run" row.
    showing_output: bool,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            history: load_history(),
            process: None,
            failure: None,
            armed: None,
            dirty: false,
            showing_output: false,
        }
    }

    fn running(&self) -> bool {
        self.process.as_ref().is_some_and(Process::running)
    }

    fn armed_for(&self, command: &str, to_terminal: bool) -> bool {
        self.armed.as_ref().is_some_and(|armed| {
            armed.command == command
                && armed.to_terminal == to_terminal
                && armed.at.elapsed() < CONFIRM_FOR
        })
    }

    fn remember(&mut self, command: &str, context: &Context) {
        if !context.search.terminal_history {
            return;
        }
        remember(&mut self.history, command);
        save_history(&self.history);
    }

    fn run(&mut self, command: &str, context: &Context) -> Reply {
        if danger(command).is_some() && !self.armed_for(command, false) {
            self.arm(command, false);
            return Reply::Stay;
        }
        self.armed = None;
        let shell = shell(context);
        let launch = shell.run(command, working_folder(&shell, context).as_deref());
        // Dropping the old process closes its job, which ends it and all it
        // started.
        self.process = None;
        self.failure = None;
        match Process::start(command, &launch) {
            Ok(process) => self.process = Some(process),
            Err(err) => {
                log::warn!("terminal: {err}");
                self.failure = Some((command.to_string(), err));
            }
        }
        self.remember(command, context);
        Reply::ClearQuery
    }

    fn open_in_terminal(&mut self, command: &str, context: &Context) -> Reply {
        if danger(command).is_some() && !self.armed_for(command, true) {
            self.arm(command, true);
            return Reply::Stay;
        }
        self.armed = None;
        let shell = shell(context);
        let launch = shell.interactive(command, working_folder(&shell, context).as_deref());
        if let Err(err) = open_terminal(&terminal_tab(&launch), &launch) {
            log::warn!("terminal: {err}");
        }
        self.remember(command, context);
        Reply::Hide
    }

    fn arm(&mut self, command: &str, to_terminal: bool) {
        self.armed = Some(Armed {
            command: command.to_string(),
            to_terminal,
            at: Instant::now(),
        });
        self.dirty = true;
    }

    fn output_rows(&self, shell: &Shell) -> Vec<ResultItem> {
        let mut rows = Vec::new();
        if let Some((command, err)) = &self.failure {
            rows.push(ResultItem {
                group: "Output".to_string(),
                title: format!("Could not start: {err}"),
                subtitle: command.clone(),
                icon: IconRef::Glyph(Glyph::Terminal),
                tone: Tone::Warning,
                ..Default::default()
            });
            return rows;
        }
        let Some(process) = &self.process else {
            return rows;
        };
        let Ok(run) = process.run.lock() else {
            return rows;
        };
        let (status, tone) = match run.ended {
            None if run.lines.is_empty() && *shell == Shell::Wsl => {
                ("Starting WSL\u{2026}".to_string(), Tone::Normal)
            }
            None => (
                format!(
                    "Running\u{2026} {:.1} s",
                    run.started.elapsed().as_secs_f32()
                ),
                Tone::Normal,
            ),
            Some((_, took)) if run.stopped => (
                format!("Stopped \u{b7} {:.1} s", took.as_secs_f32()),
                Tone::Warning,
            ),
            Some((code, took)) => (
                format!("Exit {code} \u{b7} {:.1} s", took.as_secs_f32()),
                if code == 0 {
                    Tone::Normal
                } else {
                    Tone::Warning
                },
            ),
        };
        rows.push(ResultItem {
            group: "Output".to_string(),
            title: status,
            subtitle: run.command.clone(),
            icon: IconRef::Glyph(Glyph::Terminal),
            tone,
            enter: Some(action("Run again", RUN, &run.command)),
            ctrl_enter: Some(action("Terminal", TERMINAL, &run.command)),
            ..Default::default()
        });
        rows.extend(run.lines.iter().map(|line| ResultItem {
            group: "Output".to_string(),
            title: line.text.clone(),
            tone: if line.error {
                Tone::Warning
            } else {
                Tone::Normal
            },
            compact: true,
            enter: Some(Choice {
                label: "Copy line".to_string(),
                action: Action::Copy(line.text.clone()),
            }),
            ..Default::default()
        }));
        rows
    }

    fn history_rows<'a>(
        &'a self,
        filter: &'a str,
        limit: usize,
    ) -> impl Iterator<Item = ResultItem> + 'a {
        self.history
            .iter()
            .filter_map(move |entry| {
                if filter.is_empty() {
                    return Some((0, entry));
                }
                fuzzy::score(filter, entry).map(|score| (score, entry))
            })
            .take(limit)
            .map(|(score, entry)| ResultItem {
                group: "History".to_string(),
                title: entry.clone(),
                icon: IconRef::Glyph(Glyph::Terminal),
                score,
                enter: Some(action("Run", RUN, entry)),
                ctrl_enter: Some(action("Terminal", TERMINAL, entry)),
                tab: Some(Completion {
                    label: "Complete".to_string(),
                    text: entry.clone(),
                }),
                ..Default::default()
            })
    }
}

impl SearchProvider for Terminal {
    fn id(&self) -> &'static str {
        "terminal"
    }

    fn name(&self) -> &'static str {
        "Terminal"
    }

    fn description(&self) -> &'static str {
        "Run a command in the shell chosen in the settings"
    }

    fn describe(&self, context: &Context) -> String {
        format!("Run a command in {}", shell(context).label())
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some(">")
    }

    fn placeholder(&self) -> &'static str {
        "Type a command: Enter runs it here, Ctrl+Enter in a terminal"
    }

    /// Underlining the matched letters of a command line reads as noise.
    fn needle<'a>(&self, _text: &'a str) -> &'a str {
        ""
    }

    fn waiting(&self) -> bool {
        self.running() || self.armed.is_some()
    }

    fn has_news(&mut self) -> bool {
        if self
            .armed
            .as_ref()
            .is_some_and(|armed| armed.at.elapsed() >= CONFIRM_FOR)
        {
            self.armed = None;
            self.dirty = true;
        }
        let output = self.process.as_ref().is_some_and(|process| {
            process
                .run
                .lock()
                .map(|mut run| std::mem::take(&mut run.news) || run.ended.is_none())
                .unwrap_or(false)
        });
        std::mem::take(&mut self.dirty) || (output && self.showing_output)
    }

    fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem> {
        let shell = shell(context);
        let command = query.text.trim();
        self.showing_output =
            command.is_empty() && (self.process.is_some() || self.failure.is_some());
        if command.is_empty() {
            if self.showing_output {
                return self.output_rows(&shell);
            }
            return self.history_rows("", query.limit).collect();
        }

        let mut rows = Vec::new();
        if let Some(armed) = self
            .armed
            .as_ref()
            .filter(|armed| armed.command == command && armed.at.elapsed() < CONFIRM_FOR)
        {
            let (again, kind) = if armed.to_terminal {
                ("Ctrl+Enter again to open it in a terminal", TERMINAL)
            } else {
                ("Enter again to run it", RUN)
            };
            let choice = action("Run anyway", kind, command);
            rows.push(ResultItem {
                group: "Run".to_string(),
                title: format!("This command can delete data or stop the PC. Press {again}."),
                subtitle: command.to_string(),
                icon: IconRef::Glyph(Glyph::Terminal),
                tone: Tone::Danger,
                score: fuzzy::EXACT + 2,
                enter: (!armed.to_terminal).then(|| choice.clone()),
                ctrl_enter: armed.to_terminal.then_some(choice),
                ..Default::default()
            });
        }
        let folder = working_folder(&shell, context);
        rows.push(ResultItem {
            group: "Run".to_string(),
            title: format!("Run in {}: {command}", shell.label()),
            subtitle: match &folder {
                Some(folder) => format!("in {folder}"),
                None => "in your home folder".to_string(),
            },
            icon: IconRef::Glyph(Glyph::Terminal),
            score: fuzzy::EXACT + 1,
            enter: Some(action("Run", RUN, command)),
            ctrl_enter: Some(action("Terminal", TERMINAL, command)),
            ..Default::default()
        });
        rows.extend(self.history_rows(command, 20));
        rows
    }

    fn act(&mut self, command: &str, context: &Context) -> Reply {
        match command.split_once('\n') {
            Some((RUN, text)) => self.run(text, context),
            Some((TERMINAL, text)) => self.open_in_terminal(text, context),
            _ => Reply::Stay,
        }
    }

    fn escape(&mut self) -> bool {
        if self.running() && self.showing_output {
            if let Some(process) = &self.process {
                process.stop();
            }
            return true;
        }
        false
    }

    fn escape_label(&self) -> Option<&'static str> {
        (self.running() && self.showing_output).then_some("Stop")
    }

    fn interrupted(&mut self) {
        if self.armed.take().is_some() {
            self.dirty = true;
        }
    }

    fn copy_all(&self) -> Option<String> {
        let process = self.process.as_ref()?;
        let run = process.run.lock().ok()?;
        let lines: Vec<&str> = run.lines.iter().map(|line| line.text.as_str()).collect();
        Some(lines.join("\r\n"))
    }

    fn follows_end(&self) -> bool {
        self.showing_output
    }
}

fn shell(context: &Context) -> Shell {
    Shell::from_settings(
        &context.search.terminal_shell,
        &context.search.terminal_custom,
        pwsh_installed(),
    )
    .0
}

/// The folder browsed under `/` in this opening of the palette; otherwise
/// WSL starts in its home and the Windows shells in the user's profile.
fn working_folder(shell: &Shell, context: &Context) -> Option<String> {
    if let Some(folder) = context.folder {
        return Some(folder.to_string());
    }
    if *shell == Shell::Wsl {
        return None;
    }
    std::env::var("USERPROFILE").ok()
}

fn action(label: &str, kind: &str, command: &str) -> Choice {
    Choice {
        label: label.to_string(),
        action: Action::Provider {
            provider: "terminal".to_string(),
            command: format!("{kind}\n{command}"),
        },
    }
}

/// Most recent first, each command once.
pub fn remember(history: &mut Vec<String>, command: &str) {
    history.retain(|entry| entry != command);
    history.insert(0, command.to_string());
    history.truncate(HISTORY_LENGTH);
}

fn history_path() -> PathBuf {
    config::data_dir().join("terminal_history.json")
}

fn load_history() -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(history_path()) else {
        return Vec::new();
    };
    serde_json::from_str(config::strip_bom(&text)).unwrap_or_else(|err| {
        log::warn!("terminal_history.json cannot be read ({err}); starting a new one");
        Vec::new()
    })
}

fn save_history(history: &[String]) {
    let result = serde_json::to_string_pretty(history)
        .map_err(|err| err.to_string())
        .and_then(|text| config::write_atomic(&history_path(), &text));
    if let Err(err) = result {
        log::warn!("could not save the terminal history: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_the_newest_first_and_each_command_once() {
        let mut history = vec!["ls".to_string(), "pwd".to_string()];
        remember(&mut history, "pwd");
        assert_eq!(history, ["pwd", "ls"]);
        remember(&mut history, "uname -a");
        assert_eq!(history, ["uname -a", "pwd", "ls"]);
        for index in 0..200 {
            remember(&mut history, &format!("echo {index}"));
        }
        assert_eq!(history.len(), HISTORY_LENGTH);
        assert_eq!(history[0], "echo 199");
    }

    #[test]
    fn typing_shows_a_run_row_and_never_runs_anything() {
        let mut terminal = Terminal {
            history: vec!["ls -la".to_string(), "git status".to_string()],
            process: None,
            failure: None,
            armed: None,
            dirty: false,
            showing_output: false,
        };
        let query = Query {
            text: "ls".to_string(),
            prefix: Some(">".to_string()),
            limit: 500,
        };
        let rows = terminal.query(&query, &crate::search::test_context());
        assert_eq!(rows[0].title, "Run in WSL bash: ls");
        assert_eq!(rows[0].subtitle, "in your home folder");
        assert_eq!(rows[1].title, "ls -la");
        assert!(terminal.process.is_none());
    }

    #[test]
    fn a_dangerous_command_needs_a_second_enter() {
        let mut terminal = Terminal {
            history: Vec::new(),
            process: None,
            failure: None,
            armed: None,
            dirty: false,
            showing_output: false,
        };
        let context = crate::search::test_context();
        assert_eq!(terminal.run("rm -rf build/", &context), Reply::Stay);
        assert!(terminal.process.is_none());
        let query = Query {
            text: "rm -rf build/".to_string(),
            prefix: Some(">".to_string()),
            limit: 500,
        };
        let rows = terminal.query(&query, &context);
        assert_eq!(rows[0].tone, Tone::Danger);
        terminal.interrupted();
        assert!(!terminal.armed_for("rm -rf build/", false));
    }
    fn idle(history: &[&str]) -> Terminal {
        Terminal {
            history: history.iter().map(|entry| entry.to_string()).collect(),
            process: None,
            failure: None,
            armed: None,
            dirty: false,
            showing_output: false,
        }
    }

    fn query_of(text: &str) -> Query {
        Query {
            text: text.to_string(),
            prefix: Some(">".to_string()),
            limit: 500,
        }
    }

    #[test]
    fn an_empty_query_lists_the_history_with_run_and_complete_choices() {
        let mut terminal = idle(&["ls -la", "git status"]);
        let rows = terminal.query(&query_of("  "), &crate::search::test_context());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].group, "History");
        assert_eq!(rows[0].title, "ls -la");
        assert_eq!(rows[0].enter.as_ref().unwrap().label, "Run");
        assert_eq!(rows[0].ctrl_enter.as_ref().unwrap().label, "Terminal");
        assert_eq!(rows[0].tab.as_ref().unwrap().text, "ls -la");
        assert!(!terminal.showing_output);
    }

    #[test]
    fn a_command_that_could_not_start_is_shown_as_a_warning_in_the_output() {
        let mut terminal = idle(&["ls"]);
        terminal.failure = Some(("nope".to_string(), "no such shell".to_string()));
        let rows = terminal.query(&query_of(""), &crate::search::test_context());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Could not start: no such shell");
        assert_eq!(rows[0].subtitle, "nope");
        assert_eq!(rows[0].tone, Tone::Warning);
        assert!(terminal.showing_output);
        assert!(terminal.follows_end());
        assert!(!terminal.escape());
        assert_eq!(terminal.escape_label(), None);
        assert_eq!(terminal.copy_all(), None);
    }

    #[test]
    fn the_provider_describes_itself_for_the_help_list() {
        let terminal = idle(&[]);
        let context = crate::search::test_context();
        assert_eq!(terminal.id(), "terminal");
        assert_eq!(terminal.name(), "Terminal");
        assert_eq!(terminal.default_prefix(), Some(">"));
        assert!(terminal.description().contains("shell"));
        assert!(terminal.placeholder().contains("Ctrl+Enter"));
        assert_eq!(terminal.needle("ls -la"), "");
        assert_eq!(terminal.describe(&context), "Run a command in WSL bash");
    }

    #[test]
    fn an_unknown_provider_command_does_nothing() {
        let mut terminal = idle(&[]);
        let context = crate::search::test_context();
        assert_eq!(terminal.act("garbage", &context), Reply::Stay);
        assert_eq!(terminal.act("other\nls", &context), Reply::Stay);
        assert!(terminal.process.is_none());
    }

    #[test]
    fn a_dangerous_command_for_a_terminal_asks_for_ctrl_enter_again() {
        let mut terminal = idle(&[]);
        let context = crate::search::test_context();
        let reply = terminal.act("terminal\nrm -rf build/", &context);
        assert_eq!(reply, Reply::Stay);
        assert!(terminal.waiting());
        assert!(terminal.has_news());
        assert!(!terminal.has_news());
        let rows = terminal.query(&query_of("rm -rf build/"), &context);
        assert_eq!(rows[0].tone, Tone::Danger);
        assert!(rows[0].title.contains("Ctrl+Enter again"));
        assert!(rows[0].enter.is_none());
        assert_eq!(rows[0].ctrl_enter.as_ref().unwrap().label, "Run anyway");
        assert_eq!(rows[1].title, "Run in WSL bash: rm -rf build/");
        terminal.interrupted();
        assert!(!terminal.waiting());
        assert!(terminal.has_news());
    }

    #[test]
    fn a_dangerous_command_for_the_palette_asks_for_enter_again() {
        let mut terminal = idle(&[]);
        let context = crate::search::test_context();
        terminal.arm("rm -rf build/", false);
        let rows = terminal.query(&query_of("rm -rf build/"), &context);
        assert!(rows[0].title.contains("Enter again to run it"));
        assert!(rows[0].enter.is_some());
        assert!(rows[0].ctrl_enter.is_none());
        assert!(terminal.armed_for("rm -rf build/", false));
        assert!(!terminal.armed_for("rm -rf build/", true));
        assert!(!terminal.armed_for("ls", false));
    }

    #[test]
    fn an_old_confirmation_runs_out_and_the_palette_asks_again() {
        let mut terminal = idle(&[]);
        terminal.armed = Some(Armed {
            command: "rm -rf build/".to_string(),
            to_terminal: false,
            at: Instant::now() - CONFIRM_FOR - Duration::from_secs(1),
        });
        assert!(!terminal.armed_for("rm -rf build/", false));
        assert!(terminal.has_news());
        assert!(terminal.armed.is_none());
    }

    #[test]
    fn a_folder_browsed_in_the_palette_is_where_commands_run() {
        let mut terminal = idle(&[]);
        let mut context = crate::search::test_context();
        context.folder = Some("C:/Projects");
        let rows = terminal.query(&query_of("dir"), &context);
        assert_eq!(rows[0].subtitle, "in C:/Projects");
        assert_eq!(
            working_folder(&Shell::Wsl, &context).as_deref(),
            Some("C:/Projects")
        );
    }

    #[test]
    fn history_rows_are_filtered_by_what_is_typed_and_limited() {
        let terminal = idle(&["git status", "git stash", "cargo test", "ls"]);
        let titles: Vec<String> = terminal
            .history_rows("git", 10)
            .map(|row| row.title)
            .collect();
        assert!(titles.contains(&"git status".to_string()));
        assert!(!titles.contains(&"cargo test".to_string()));
        assert_eq!(terminal.history_rows("", 2).count(), 2);
    }

    #[test]
    fn the_action_choice_carries_the_kind_and_the_command() {
        let choice = action("Run", RUN, "ls");
        assert_eq!(choice.label, "Run");
        assert_eq!(
            choice.action,
            Action::Provider {
                provider: "terminal".to_string(),
                command: "run\nls".to_string()
            }
        );
    }
}
