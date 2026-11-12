//! Which hosts a shell command runs `ssh` to, for ClaudeDeck's "Waits on
//! <server>". Only a word in the place of a command counts: `ssh host`,
//! a path that ends in `ssh` or `ssh.exe`, or `ssh` behind a wrapper such
//! as `timeout 20`, `wsl.exe -e bash -lc '...'` or `env A=b`. A word
//! `ssh` that is only an argument (`echo ssh host`, `grep ssh file`) does
//! not count, nor does `scp` or `git push`.

/// ssh options that take a value: their next word is not the host.
const VALUE_OPTIONS: &str = "BbcDEeFIiJLlmOoPpQRSWw";

/// Words that run the command after them, with options of their own.
const WRAPPERS: [&str; 15] = [
    "timeout", "wsl", "wsl.exe", "bash", "sh", "zsh", "env", "nohup", "time", "exec", "command",
    "sudo", "bash.exe", "sh.exe", "$",
];

/// Wrapper options whose next word is their value, not the command.
const WRAPPER_VALUES: [&str; 8] = [
    "-d",
    "-u",
    "-k",
    "-s",
    "--distribution",
    "--user",
    "-C",
    "-g",
];

/// The hosts a command runs ssh to, in order, with any `user@` removed.
pub fn ssh_hosts(command: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for words in commands(command) {
        if let Some(host) = host_of(&words) {
            hosts.push(host);
        }
    }
    hosts
}

/// The command split into simple commands, each a list of words. Quotes
/// are dropped, so a command inside `bash -lc '...'` is read as words
/// after the wrapper.
fn commands(command: &str) -> Vec<Vec<String>> {
    let mut all = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let end_word = |word: &mut String, words: &mut Vec<String>| {
        if !word.is_empty() {
            words.push(std::mem::take(word));
        }
    };
    for c in command.chars() {
        match c {
            ';' | '|' | '&' | '(' | ')' | '\n' | '`' | '{' | '}' => {
                end_word(&mut word, &mut words);
                if !words.is_empty() {
                    all.push(std::mem::take(&mut words));
                }
            }
            '\'' | '"' => end_word(&mut word, &mut words),
            c if c.is_whitespace() => end_word(&mut word, &mut words),
            c => word.push(c),
        }
    }
    end_word(&mut word, &mut words);
    if !words.is_empty() {
        all.push(words);
    }
    all
}

/// The last part of a path, without `.exe`, in lower case.
fn program(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let name = name.to_ascii_lowercase();
    name.strip_suffix(".exe")
        .map(str::to_string)
        .unwrap_or(name)
}

/// The host of one simple command, when it is ssh.
fn host_of(words: &[String]) -> Option<String> {
    let mut at = 0;
    // Wrappers and variable settings in front of the command.
    loop {
        let word = words.get(at)?;
        let name = program(word);
        if word.contains('=') && !word.starts_with('-') {
            at += 1;
        } else if WRAPPERS.contains(&word.as_str()) || WRAPPERS.contains(&name.as_str()) {
            let timeout = name == "timeout";
            at += 1;
            while let Some(next) = words.get(at) {
                if next.starts_with('-') {
                    at += if WRAPPER_VALUES.contains(&next.as_str()) {
                        2
                    } else {
                        1
                    };
                } else {
                    break;
                }
            }
            // `timeout 20 ssh ...`: the time is not the command.
            if timeout
                && words
                    .get(at)
                    .is_some_and(|w| w.starts_with(|c: char| c.is_ascii_digit()))
            {
                at += 1;
            }
        } else {
            break;
        }
    }
    if program(&words[at]) != "ssh" {
        return None;
    }
    at += 1;
    while let Some(word) = words.get(at) {
        let Some(option) = word.strip_prefix('-') else {
            break;
        };
        let takes_value = option.len() == 1 && VALUE_OPTIONS.contains(option);
        at += if takes_value { 2 } else { 1 };
    }
    let host = words.get(at)?;
    let host = host.rsplit('@').next().unwrap_or(host);
    let plain = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    plain.then(|| host.to_string())
}

#[cfg(test)]
mod tests;
