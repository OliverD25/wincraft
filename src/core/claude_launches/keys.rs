//! The job keys of a shell command (pure): the hosts it runs ssh to, the
//! units it starts, and the program words of each command in it.

use crate::core::claude_turns::ssh::{commands, host_of, program};

/// At most this many keys per call.
const MOST_KEYS: usize = 12;

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
        // A variable setting or a shell keyword: the program comes later.
        if (word.contains('=') && !word.starts_with('-')) || KEYWORDS.contains(&word.as_str()) {
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
