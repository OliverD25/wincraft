//! Commands that can delete data or stop the PC, which the `>` prefix runs
//! only after a second Enter. WinCraft takes no regex crate, so each rule is
//! a small function over the words of one simple command; to add a rule, add
//! one line to `RULES`.

/// A rule gets the lower-case words of one simple command, with `sudo` and
/// cmd's `/c` wrapper already taken off, so `words[0]` is the program.
type Rule = fn(&[String]) -> bool;

const RULES: &[(&str, Rule)] = &[
    ("rm -r, rm -f, or rm on /, ~ or /mnt/c", rm),
    ("mkfs", |w| w[0] == "mkfs" || w[0].starts_with("mkfs.")),
    ("dd writing with of=", |w| {
        w[0] == "dd" && w.iter().any(|a| a.starts_with("of="))
    }),
    ("shutdown, reboot, poweroff, halt", |w| {
        matches!(w[0].as_str(), "shutdown" | "reboot" | "poweroff" | "halt")
    }),
    ("chmod -R or chown -R on /", |w| {
        matches!(w[0].as_str(), "chmod" | "chown")
            && w.iter().any(|a| a == "-r" || a == "--recursive")
            && w.iter().any(|a| a == "/" || a == "/*")
    }),
    ("git push --force, git reset --hard, git clean -fd", git),
    ("format", |w| w[0] == "format"),
    ("diskpart", |w| w[0] == "diskpart"),
    ("reg delete", |w| {
        w[0] == "reg" && w.get(1).is_some_and(|a| a == "delete")
    }),
    ("Remove-Item -Recurse", |w| {
        w[0] == "remove-item" && w.iter().skip(1).any(|a| a.starts_with("-r"))
    }),
    ("del /s, rmdir /s", |w| {
        matches!(w[0].as_str(), "del" | "erase" | "rmdir" | "rd") && w.iter().any(|a| a == "/s")
    }),
];

/// Checked on the whole text, spaces removed, because these are not about
/// one program's words.
const RAW_RULES: &[(&str, &str)] = &[
    ("the fork bomb", ":(){:|:&};:"),
    ("writing to a disk device", ">/dev/sd"),
    ("writing to a disk device", ">/dev/nvme"),
];

/// Which rule the command breaks, if any.
pub fn danger(command: &str) -> Option<&'static str> {
    let squeezed: String = command.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some((name, _)) = RAW_RULES
        .iter()
        .find(|(_, needle)| squeezed.contains(needle))
    {
        return Some(name);
    }
    simple_commands(command).find_map(|words| {
        RULES
            .iter()
            .find(|(_, rule)| rule(&words))
            .map(|(name, _)| *name)
    })
}

/// Splits at ; & | and line breaks, and unwraps sudo and cmd /c, so a
/// dangerous command hidden after another one is still seen.
fn simple_commands(command: &str) -> impl Iterator<Item = Vec<String>> + '_ {
    command.split([';', '&', '|', '\n']).filter_map(|part| {
        let mut words: Vec<String> = part
            .split_whitespace()
            .map(|word| word.trim_matches(['"', '\'']).to_lowercase())
            .collect();
        loop {
            let first = words.first()?.clone();
            let program = first
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&first)
                .trim_end_matches(".exe")
                .to_string();
            let wrapper = program == "sudo"
                || (program == "cmd" && words.get(1).is_some_and(|a| a == "/c" || a == "/k"));
            if !wrapper {
                words[0] = program;
                return Some(words);
            }
            let skip = if program == "sudo" { 1 } else { 2 };
            words.drain(..skip);
        }
    })
}

fn rm(words: &[String]) -> bool {
    if words[0] != "rm" {
        return false;
    }
    // After "--" every word is a file name, even one that starts with "-".
    let options_end = words
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(words.len());
    words.iter().enumerate().skip(1).any(|(index, arg)| {
        let is_option = index < options_end;
        let flags =
            is_option && arg.starts_with('-') && !arg.starts_with("--") && arg.contains(['r', 'f']);
        let long = matches!(arg.as_str(), "--recursive" | "--force");
        let target = matches!(
            arg.as_str(),
            "/" | "/*" | "~" | "~/" | "~/*" | "/mnt/c" | "/mnt/c/" | "/mnt/c/*"
        );
        flags || long || target
    })
}

fn git(words: &[String]) -> bool {
    if words[0] != "git" {
        return false;
    }
    let args = &words[1..];
    match args.first().map(String::as_str) {
        Some("push") => args.iter().any(|a| a == "--force" || a == "-f"),
        Some("reset") => args.iter().any(|a| a == "--hard"),
        Some("clean") => args.iter().any(|a| {
            a.starts_with('-') && !a.starts_with("--") && a.contains('f') && a.contains('d')
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_commands_are_caught() {
        for command in [
            "rm -rf build/",
            "rm -r old",
            "rm -f notes.txt",
            "rm --recursive x",
            "rm /",
            "rm ~",
            "sudo rm -rf /mnt/c",
            "ls && rm -rf *",
            "mkfs.ext4 /dev/sdb1",
            "dd if=/dev/zero of=/dev/sda bs=1M",
            "sudo shutdown -h now",
            "reboot",
            ":(){ :|:& };:",
            "chmod -R 777 /",
            "chown -R me /",
            "echo x > /dev/sda",
            "git push --force origin main",
            "git push -f",
            "git reset --hard HEAD~1",
            "git clean -fd",
            "git clean -xdf",
            "cmd.exe /c format D:",
            "diskpart",
            "reg delete HKCU\\Software\\X /f",
            "Remove-Item -Recurse -Force .\\build",
            "del /s *.tmp",
            "RMDIR /S /Q old",
            "cmd /c rd /s x",
        ] {
            assert!(danger(command).is_some(), "{command} should be dangerous");
        }
    }

    #[test]
    fn everyday_commands_are_not() {
        for command in [
            "rm file.txt",
            "rm -- -weird-name",
            "ls -la /",
            "echo rm -rf is scary",
            "git push origin main",
            "git reset HEAD file",
            "git clean -n",
            "chmod -R 755 ./site",
            "chmod 644 /etc/x",
            "dd if=a.img",
            "cat /dev/sda1.txt",
            "Remove-Item old.txt",
            "del file.txt",
            "uname -a",
            "sleep 3",
            "format-table",
            "reg query HKCU",
            "echo shutdown",
        ] {
            assert_eq!(danger(command), None, "{command} should be allowed");
        }
    }
}
