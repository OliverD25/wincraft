//! The ssh commands a Claude Code chat ran, for ClaudeDeck's "Server job":
//! a job on a homelab server belongs to the PC chat that started it. Read
//! in the same pass over the transcript as the background jobs
//! (`claude_jobs`).
//!
//! From each shell tool call (foreground or `run_in_background`) whose
//! command runs ssh, this keeps the hosts, the time, the command (cut at
//! `COMMAND_CAP`) and the job keys: the units after `systemctl --user
//! start|restart|enable --now`, the name of `systemd-run --user
//! --unit=<name>`, and the program words of every command in it (`nohup
//! python -m tools.archive write` gives `tools.archive write`). A
//! background one stays open until its task notice names its tool call,
//! or the chat stops it with `TaskStop`. Only calls of the last
//! `KEEP_MS` are kept.
//!
//! What was learned from real transcripts (2026-11-13): the tool result of
//! a background call holds `"backgroundTaskId":"<id>"` with the call's
//! `"tool_use_id"`; its notice holds `<tool-use-id>toolu_...</tool-use-id>`,
//! a `<status>` and "(exit code N)". A script sent on ssh's standard input
//! is in the same command as a here-document, so its words count too.

use std::collections::HashMap;

use serde_json::Value;

use crate::core::claude_turns::ssh::{commands, host_of, program};
use crate::core::claude_turns::words;

/// Calls older than this are dropped: seven days.
pub const KEEP_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// How much of a command is kept to show.
pub const COMMAND_CAP: usize = 120;
/// At most this many calls are kept per chat, newest last.
const MOST: usize = 400;
/// At most this many keys per call.
const MOST_KEYS: usize = 12;
/// A record longer than this is not read as JSON.
const MOST_LINE: usize = 512 * 1024;
const SHELL_TOOLS: [&str; 2] = ["Bash", "PowerShell"];

/// How a background ssh command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// It failed, with its exit code when the notice gave one.
    Failed(Option<i32>),
    /// The chat or the user stopped it.
    Stopped,
}

/// One shell command that ran ssh.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    /// The hosts it runs ssh to, as written.
    pub hosts: Vec<String>,
    /// The command, cleaned, at most `COMMAND_CAP` characters.
    pub command: String,
    /// When Claude called it, in Unix ms.
    pub at: Option<u64>,
    /// Units it started, without `.service`.
    pub units: Vec<String>,
    /// The program words of the commands in it.
    pub words: Vec<String>,
    /// The program words of the first command it runs on the server.
    pub remote: Option<String>,
    pub background: bool,
    /// A background call that ended: when, and how.
    pub ended: Option<(Option<u64>, Outcome)>,
    pub(crate) tool_id: String,
}

impl Launch {
    /// A background call with no end notice yet.
    pub fn is_open(&self) -> bool {
        self.background && self.ended.is_none()
    }
}

/// The ssh calls of one chat, kept while its transcript is read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launches {
    list: Vec<Launch>,
    /// Background task id to tool call id.
    tasks: HashMap<String, String>,
}

impl Launches {
    /// Looks at one line of the transcript. `now` drops old calls.
    pub fn line(&mut self, line: &str, now: u64) {
        if line.contains("<tool-use-id>") {
            self.notice(line);
        }
        if line.contains("\"name\":\"TaskStop\"") {
            for task in values_after(line, "\"task_id\":\"") {
                if let Some(tool) = self.tasks.get(&task).cloned() {
                    self.end(&tool, timestamp(line), Outcome::Stopped);
                }
            }
        }
        if line.contains("backgroundTaskId") && line.contains("\"tool_use_id\":\"") {
            self.task_ids(line);
        }
        if line.len() > MOST_LINE || !line.contains("\"tool_use\"") || !line.contains("ssh") {
            return;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let at = timestamp(line);
        if at.is_some_and(|at| now.saturating_sub(at) > KEEP_MS) {
            return;
        }
        let Some(Value::Array(blocks)) = record.get("message").and_then(|m| m.get("content"))
        else {
            return;
        };
        for block in blocks {
            if let Some(launch) = launch_of(block, at) {
                if self.list.len() == MOST {
                    self.list.remove(0);
                }
                self.list.push(launch);
            }
        }
    }

    pub fn reset(&mut self) {
        *self = Launches::default();
    }

    /// The calls of the last `KEEP_MS`, oldest first.
    pub fn recent(&self, now: u64) -> Vec<Launch> {
        self.list
            .iter()
            .filter(|launch| launch.at.is_none_or(|at| now.saturating_sub(at) <= KEEP_MS))
            .cloned()
            .collect()
    }

    fn task_ids(&mut self, line: &str) {
        let text = line.replace("\\\"", "\"");
        let tools = values_after(&text, "\"tool_use_id\":\"");
        let tasks = values_after(&text, "\"backgroundTaskId\":\"");
        if let (Some(tool), Some(task)) = (tools.first(), tasks.first()) {
            if self.list.iter().any(|launch| &launch.tool_id == tool) {
                self.tasks.insert(task.clone(), tool.clone());
            }
        }
    }

    /// A task notice: `<tool-use-id>` with a `<status>` after it.
    fn notice(&mut self, line: &str) {
        let at = timestamp(line);
        let mut rest = line;
        while let Some(found) = rest.find("<tool-use-id>") {
            rest = &rest[found + "<tool-use-id>".len()..];
            let Some(close) = rest.find("</tool-use-id>") else {
                return;
            };
            let tool = rest[..close].to_string();
            let after = &rest[close..];
            let window = &after[..floor(after, 800)];
            let window = window
                .find("<tool-use-id>")
                .map_or(window, |next| &window[..next]);
            let Some(status) = between(window, "<status>", "</status>") else {
                continue;
            };
            let outcome = match status {
                "completed" => match exit_code(window) {
                    Some(code) if code != 0 => Outcome::Failed(Some(code)),
                    _ => Outcome::Done,
                },
                "failed" => Outcome::Failed(exit_code(window)),
                _ => Outcome::Stopped,
            };
            self.end(&tool, at, outcome);
        }
    }

    fn end(&mut self, tool: &str, at: Option<u64>, outcome: Outcome) {
        if let Some(launch) = self
            .list
            .iter_mut()
            .find(|launch| launch.tool_id == tool && launch.background)
        {
            launch.ended.get_or_insert((at, outcome));
        }
    }
}

fn floor(text: &str, most: usize) -> usize {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let from = text.find(open)? + open.len();
    let to = text[from..].find(close)? + from;
    Some(&text[from..to])
}

/// "(exit code 1)" as 1.
fn exit_code(text: &str) -> Option<i32> {
    let from = text.find("exit code ")? + "exit code ".len();
    let digits: String = text[from..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits.parse().ok()
}

fn timestamp(line: &str) -> Option<u64> {
    crate::core::claude_jobs::timestamp_ms(line)
}

/// The values written right after `key` (which ends with an opening
/// quote), when they look like ids.
fn values_after(text: &str, key: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(key) {
        rest = &rest[at + key.len()..];
        let end = rest.find('"').unwrap_or(rest.len());
        let id = &rest[..end];
        if !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            found.push(id.to_string());
        }
    }
    found
}

/// A shell tool call whose command runs ssh.
fn launch_of(block: &Value, at: Option<u64>) -> Option<Launch> {
    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
        return None;
    }
    let name = block.get("name").and_then(Value::as_str)?;
    if !SHELL_TOOLS.contains(&name) {
        return None;
    }
    let input = block.get("input")?;
    let command = input.get("command").and_then(Value::as_str)?;
    let keys = keys_of(command);
    if keys.hosts.is_empty() {
        return None;
    }
    Some(Launch {
        hosts: keys.hosts,
        command: words::cap(&words::clean(command), COMMAND_CAP),
        at,
        units: keys.units,
        words: keys.words,
        remote: keys.remote,
        background: input.get("run_in_background").and_then(Value::as_bool) == Some(true),
        ended: None,
        tool_id: block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// What a command tells about the jobs it starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub hosts: Vec<String>,
    pub units: Vec<String>,
    pub words: Vec<String>,
    /// The program words of the first command it runs on the server.
    pub remote: Option<String>,
}

/// The hosts, units and program words of a shell command.
pub fn keys_of(command: &str) -> Keys {
    let mut keys = Keys::default();
    for words in commands(command) {
        let remote = match host_of(&words) {
            Some(host) => {
                keys.hosts.push(host);
                after_host(&words)
            }
            None => words.as_slice(),
        };
        let found = program_words(remote);
        // `ssh h 'cd x && python -m a b'` splits at `&&`: the first named
        // program from the ssh command on is what it runs there.
        if keys.remote.is_none() && !keys.hosts.is_empty() {
            keys.remote = found.clone();
        }
        add(&mut keys.words, found);
        for unit in units_of(remote) {
            add(&mut keys.units, Some(unit));
        }
    }
    keys
}

fn add(list: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value {
        if !list.contains(&value) && list.len() < MOST_KEYS {
            list.push(value);
        }
    }
}

/// The words after the host of an ssh command: what it runs there.
fn after_host(words: &[String]) -> &[String] {
    let Some(ssh) = words.iter().position(|word| program(word) == "ssh") else {
        return &[];
    };
    let mut at = ssh + 1;
    while let Some(option) = words.get(at).and_then(|w| w.strip_prefix('-')) {
        let takes_value = option.len() == 1 && "BbcDEeFIiJLlmOoPpQRSWw".contains(option);
        at += if takes_value { 2 } else { 1 };
    }
    words.get(at + 1..).unwrap_or_default()
}

/// Commands that start nothing worth naming.
const PLAIN: [&str; 60] = [
    "cd",
    "echo",
    "printf",
    "cat",
    "ls",
    "grep",
    "egrep",
    "rg",
    "tail",
    "head",
    "date",
    "set",
    "export",
    "unset",
    "source",
    ".",
    "sed",
    "awk",
    "wc",
    "sort",
    "uniq",
    "ps",
    "pgrep",
    "pkill",
    "kill",
    "systemctl",
    "journalctl",
    "ssh",
    "scp",
    "sleep",
    "true",
    "false",
    "test",
    "[",
    "exit",
    "return",
    "mkdir",
    "rm",
    "cp",
    "mv",
    "touch",
    "chmod",
    "ln",
    "find",
    "du",
    "df",
    "free",
    "uptime",
    "tee",
    "xargs",
    "which",
    "read",
    "wait",
    "cut",
    "tr",
    "stat",
    "readlink",
    "id",
    "curl",
    "jq",
];

/// Words in front of the program that only change how it runs.
const WRAPPERS: [&str; 12] = [
    "nohup", "setsid", "env", "nice", "ionice", "timeout", "exec", "command", "time", "sudo",
    "stdbuf", "chrt",
];

/// Shell words that start a block, not a program.
const KEYWORDS: [&str; 10] = [
    "if", "then", "else", "elif", "do", "while", "until", "for", "!", "time",
];

/// Wrapper options whose next word is their value.
const VALUE_OPTIONS: [&str; 8] = ["-n", "-c", "-u", "-s", "-k", "-p", "-o", "-g"];

fn is_shell(name: &str) -> bool {
    matches!(name, "bash" | "sh" | "dash" | "zsh")
}

fn is_python(name: &str) -> bool {
    name.starts_with("python") && name[6..].chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The last part of a path, as written (not lower case).
fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// The words that name what a command runs: `tools.archive write` for
/// `nice -n 10 .venv/bin/python -m tools.archive write --limit 5`,
/// `office_fetch.py run` for `python office_fetch.py run`, `sync_law.sh`
/// for `bash sync_law.sh`, `docker compose` for `docker compose up`. None
/// for a plain command (`cd`, `cat`, `systemctl` ...) or none at all.
pub fn program_words(words: &[String]) -> Option<String> {
    let mut at = 0;
    loop {
        let word = words.get(at)?;
        let name = program(word);
        if word.contains('=') && !word.starts_with('-') {
            at += 1;
        } else if KEYWORDS.contains(&word.as_str()) {
            at += 1;
        } else if WRAPPERS.contains(&name.as_str()) {
            let timeout = name == "timeout";
            let nice = matches!(name.as_str(), "nice" | "ionice");
            at += 1;
            while let Some(next) = words.get(at) {
                if !next.starts_with('-') {
                    break;
                }
                let joined = next.len() > 2 && !next.starts_with("--");
                at += if !joined && (nice || VALUE_OPTIONS.contains(&next.as_str())) {
                    2
                } else {
                    1
                };
            }
            if timeout
                && words
                    .get(at)
                    .is_some_and(|w| w.starts_with(|c: char| c.is_ascii_digit()))
            {
                at += 1;
            }
        } else if is_shell(&name) && words.get(at + 1).is_some_and(|w| w.starts_with('-')) {
            // `bash -c '...'`, `bash -lc`, `sh -s`: the words after it.
            at += 2;
        } else if name == "uv" && words.get(at + 1).is_some_and(|w| w == "run") {
            at += 2;
        } else {
            break;
        }
    }
    let name = program(&words[at]);
    let shouting = words[at].chars().all(|c| c.is_ascii_uppercase());
    if PLAIN.contains(&name.as_str()) || shouting || !is_word(&words[at]) {
        return None;
    }
    let rest = &words[at + 1..];
    let first_arg = |from: &[String]| -> Option<String> {
        from.iter()
            .find(|w| !w.starts_with('-'))
            .filter(|w| is_word(w))
            .map(|w| w.to_string())
    };
    if is_python(&name) {
        let mut i = 0;
        while let Some(word) = rest.get(i) {
            if word == "-m" {
                let module = rest.get(i + 1)?;
                return Some(join(module, first_arg(rest.get(i + 2..)?)));
            }
            if !word.starts_with('-') {
                return Some(join(base(word), first_arg(&rest[i + 1..])));
            }
            i += if word == "-X" || word == "-W" { 2 } else { 1 };
        }
        return None;
    }
    if is_shell(&name) || matches!(name.as_str(), "node" | "deno" | "bun" | "tsx") {
        let script = rest.iter().position(|w| !w.starts_with('-'))?;
        return Some(join(base(&rest[script]), first_arg(&rest[script + 1..])));
    }
    Some(join(base(&words[at]), first_arg(rest)))
}

/// A plain word: a name or a path, not a redirect, a variable or a value.
fn is_word(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '~'))
        && !word.contains(['<', '>', '$', '='])
}

fn join(first: &str, second: Option<String>) -> String {
    match second {
        Some(second) => format!("{first} {second}"),
        None => first.to_string(),
    }
}

/// Units a command starts: `systemctl --user start|restart|enable --now
/// <units>` and `systemd-run --user --unit=<name>`, without `.service`.
pub fn units_of(words: &[String]) -> Vec<String> {
    let mut units = Vec::new();
    let Some(at) = words.iter().position(|w| {
        let name = program(w);
        name == "systemctl" || name == "systemd-run"
    }) else {
        return units;
    };
    let rest = &words[at + 1..];
    if !rest.iter().any(|w| w == "--user") {
        return units;
    }
    let unit_name = |word: &str| -> Option<String> {
        let name = word.strip_suffix(".service").unwrap_or(word);
        let plain = !name.is_empty()
            && !name.contains('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '@'));
        plain.then(|| name.to_string())
    };
    if program(&words[at]) == "systemd-run" {
        let mut i = 0;
        while let Some(word) = rest.get(i) {
            if let Some(name) = word.strip_prefix("--unit=") {
                units.extend(unit_name(name));
            } else if word == "--unit" || word == "-u" {
                units.extend(rest.get(i + 1).and_then(|w| unit_name(w)));
            }
            i += 1;
        }
        return units;
    }
    let Some(verb) = rest.iter().position(|w| !w.starts_with('-')) else {
        return units;
    };
    let starts = match rest[verb].as_str() {
        "start" | "restart" | "try-restart" | "reload-or-restart" => true,
        "enable" => rest.iter().any(|w| w == "--now"),
        _ => false,
    };
    if starts {
        units.extend(
            rest[verb + 1..]
                .iter()
                .filter(|w| !w.starts_with('-'))
                .filter_map(|w| unit_name(w)),
        );
    }
    units
}

#[cfg(test)]
mod tests;
