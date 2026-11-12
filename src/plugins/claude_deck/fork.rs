//! "Fork in a terminal": opens a terminal that starts a fork of a chat,
//! `claude --resume <session id> --fork-session`. The fork is a new chat
//! with the whole history. The chat it came from is not changed.
//! The server version lives in `servers/terminal.rs`.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE;

use crate::core::instance;

use super::forks::Launch;

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

/// The arguments of claude for a fork. `--session-id` names the new chat,
/// so WinCraft knows which transcript is the fork.
pub fn claude_args(session: &str, new_id: &str) -> Vec<String> {
    [
        "--resume",
        session,
        "--fork-session",
        "--session-id",
        new_id,
    ]
    .map(str::to_string)
    .to_vec()
}

/// The arguments of Windows Terminal: a new window with a fixed title, its
/// folder, then the claude command.
pub fn wt_args(fork: &Fork, claude: &str, launch: &Launch) -> Vec<String> {
    let mut args = launch.wt_prefix();
    args.extend(["-d".to_string(), fork.folder.clone(), claude.to_string()]);
    args.extend(claude_args(&fork.session, &launch.id));
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

/// Opens the terminal. A test copy of WinCraft opens nothing. `parent` is
/// the chat as ClaudeDeck knows it; the new terminal shows as its fork.
pub fn open(fork: &Fork, parent: &str, parent_title: &str) {
    // Unit tests never start a terminal.
    if cfg!(test) {
        return;
    }
    if instance::is_test() {
        log::info!("read-only test instance: forking a chat skipped");
        return;
    }
    let claude = claude_program();
    let launch = Launch::new(parent_title);
    if let Some(launch) = &launch {
        let opened = Command::new("wt.exe")
            .args(wt_args(fork, &claude, launch))
            .spawn();
        if opened.is_ok() {
            launch.remember(parent, &fork.folder, None);
            return;
        }
    }
    // The console has no title ClaudeDeck can find, so it gets no row.
    let new_id = launch.map_or_else(|| fork.session.clone(), |launch| launch.id);
    let console = Command::new(&claude)
        .args(claude_args(&fork.session, &new_id))
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
    const NEW: &str = "11111111-2222-4333-8444-555555555555";

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
    fn windows_terminal_gets_a_new_window_a_title_and_the_new_session_id() {
        let fork = Fork::new(ID, Some("C:/Dev/app")).unwrap();
        let launch = Launch::with_id(NEW, "Chat");
        assert_eq!(
            wt_args(&fork, "C:/bin/claude.exe", &launch),
            [
                "-w",
                "new",
                "--title",
                "Fork of Chat \u{b7} 11111111",
                "--suppressApplicationTitle",
                "-d",
                "C:/Dev/app",
                "C:/bin/claude.exe",
                "--resume",
                ID,
                "--fork-session",
                "--session-id",
                NEW,
            ]
        );
    }

    #[test]
    fn the_console_fallback_runs_the_same_claude_command() {
        assert_eq!(
            claude_args(ID, NEW),
            ["--resume", ID, "--fork-session", "--session-id", NEW]
        );
    }
}
