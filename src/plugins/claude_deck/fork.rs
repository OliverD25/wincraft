//! "Fork in a terminal": opens a terminal that starts a fork of a chat,
//! `claude --resume <session id> --fork-session`. The fork is a new chat
//! with the whole history. The chat it came from is not changed.
//! The server version lives in `servers/terminal.rs`.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE;

use crate::core::instance;

/// What is needed to fork one chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fork {
    /// The Claude Code session id (the name of the transcript file).
    pub session: String,
    /// The folder the chat runs in; `--resume` looks for the session there.
    pub folder: String,
}

impl Fork {
    /// None when the session id is not UUID-like or the folder is missing
    /// or has a character that could break a command line.
    pub fn new(session: &str, folder: Option<&str>) -> Option<Fork> {
        let folder = folder?.trim();
        let fine = !folder.is_empty() && !folder.chars().any(|c| c == ';' || c.is_control());
        (valid_id(session) && fine).then(|| Fork {
            session: session.to_string(),
            folder: folder.to_string(),
        })
    }
}

/// A session id is only hex digits and dashes, so it is safe in any command.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The arguments of claude for a fork.
pub fn claude_args(session: &str) -> Vec<String> {
    ["--resume", session, "--fork-session"]
        .map(str::to_string)
        .to_vec()
}

/// The arguments of Windows Terminal: its folder, then the claude command.
pub fn wt_args(fork: &Fork, claude: &str) -> Vec<String> {
    let mut args = vec!["-d".to_string(), fork.folder.clone(), claude.to_string()];
    args.extend(claude_args(&fork.session));
    args
}

/// `claude.exe` on PATH, else in `%USERPROFILE%\.local\bin`.
fn claude_program() -> String {
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("claude.exe"))
            .find(|file| file.is_file())
    });
    let home = std::env::var_os("USERPROFILE")
        .map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("bin")
                .join("claude.exe")
        })
        .filter(|file| file.is_file());
    on_path.or(home).map_or_else(
        || "claude.exe".to_string(),
        |file| file.display().to_string(),
    )
}

/// Opens the terminal. A test copy of WinCraft opens nothing.
pub fn open(fork: &Fork) {
    // Unit tests never start a terminal.
    if cfg!(test) {
        return;
    }
    if instance::is_test() {
        log::info!("read-only test instance: forking a chat skipped");
        return;
    }
    let claude = claude_program();
    if Command::new("wt.exe")
        .args(wt_args(fork, &claude))
        .spawn()
        .is_ok()
    {
        return;
    }
    let console = Command::new(&claude)
        .args(claude_args(&fork.session))
        .current_dir(&fork.folder)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn();
    if let Err(err) = console {
        log::warn!("ClaudeDeck: could not fork a chat in a terminal: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0f3a9c1e-1b2d-4c5e-8f60-123456789abc";

    #[test]
    fn a_fork_needs_a_clean_id_and_folder() {
        assert!(Fork::new(ID, Some("C:/Dev")).is_some());
        assert!(Fork::new("", Some("C:/Dev")).is_none());
        assert!(Fork::new("abc; calc", Some("C:/Dev")).is_none());
        assert!(Fork::new("../x", Some("C:/Dev")).is_none());
        assert!(Fork::new(ID, None).is_none());
        assert!(Fork::new(ID, Some("  ")).is_none());
        assert!(Fork::new(ID, Some("C:/a;b")).is_none());
    }

    #[test]
    fn windows_terminal_gets_the_folder_and_the_claude_command() {
        let fork = Fork::new(ID, Some("C:/Dev/app")).unwrap();
        assert_eq!(
            wt_args(&fork, "C:/bin/claude.exe"),
            [
                "-d",
                "C:/Dev/app",
                "C:/bin/claude.exe",
                "--resume",
                ID,
                "--fork-session"
            ]
        );
    }

    #[test]
    fn the_console_fallback_runs_the_same_claude_command() {
        assert_eq!(claude_args(ID), ["--resume", ID, "--fork-session"]);
    }
}
