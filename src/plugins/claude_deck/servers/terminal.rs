//! "Open a terminal there": Windows Terminal with ssh into a server chat's
//! folder, or a plain console window when Windows Terminal is missing.
//! Nothing is changed on the server: the shell only starts in the folder.

use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE;

use crate::core::instance;
use crate::plugins::claude_deck::fork::{valid_id, Fork};
use crate::plugins::claude_deck::forks::Launch;

use super::probe::ssh_program;

/// The command run on the server: a login shell in the folder, or in the
/// home folder when the path has a character that could end the quotes.
pub fn remote_command(cwd: Option<&str>) -> String {
    match cwd.filter(|cwd| safe_path(cwd)) {
        Some(cwd) => format!("cd '{cwd}' && exec $SHELL -l"),
        None => "exec $SHELL -l".to_string(),
    }
}

/// A path that can go between single quotes on the server and through
/// Windows Terminal's command line: no quote, no `;` (Windows Terminal
/// splits its tabs there), no control character.
pub(super) fn safe_path(path: &str) -> bool {
    path.starts_with('/')
        && !path
            .chars()
            .any(|c| matches!(c, '\'' | '"' | ';' | '\\' | '`' | '$') || c.is_control())
}

/// The command that forks a chat on the server, with Remote Control on so
/// the fork also shows in the Claude app. None when the session id or the
/// path is not safe to put in a command.
pub fn fork_command(cwd: &str, session: &str, new_id: &str) -> Option<String> {
    (safe_path(cwd) && valid_id(session) && valid_id(new_id)).then(|| {
        format!(
            "cd '{cwd}' && exec $SHELL -lc 'claude --resume {session} --fork-session --session-id {new_id} --remote-control'"
        )
    })
}

/// The arguments of ssh for an interactive shell there.
pub fn ssh_args(host: &str, cwd: Option<&str>) -> Vec<String> {
    ssh_args_for(host, remote_command(cwd))
}

fn ssh_args_for(host: &str, command: String) -> Vec<String> {
    vec![
        "-t".to_string(),
        "--".to_string(),
        host.to_string(),
        command,
    ]
}

/// The arguments of ssh that fork a chat; None when it is not safe.
pub fn fork_args(host: &str, fork: &Fork, new_id: &str) -> Option<Vec<String>> {
    fork_command(&fork.folder, &fork.session, new_id).map(|command| ssh_args_for(host, command))
}

/// Opens the terminal. A test copy of WinCraft opens nothing.
pub fn open(host: &str, cwd: Option<&str>) {
    launch(host, ssh_args(host, cwd), None);
}

/// Opens a terminal that forks a server chat. `parent` is the chat as
/// ClaudeDeck knows it; the terminal shows as its fork.
pub fn open_fork(host: &str, fork: &Fork, parent: &str, parent_title: &str) {
    let Some(launch) = Launch::new(parent_title) else {
        return;
    };
    match fork_args(host, fork, &launch.id) {
        Some(args) => {
            if launch_in_wt(host, args, Some(&launch)) {
                launch.remember(parent, &fork.folder, Some(host));
            }
        }
        None => log::warn!("ClaudeDeck: a chat on {host} cannot be forked safely"),
    }
}

fn launch(host: &str, args: Vec<String>, title: Option<&Launch>) {
    launch_in_wt(host, args, title);
}

/// Starts the terminal. True when it opened in Windows Terminal, the only
/// window ClaudeDeck can find again by its title.
fn launch_in_wt(host: &str, args: Vec<String>, title: Option<&Launch>) -> bool {
    // Unit tests never start ssh.
    if cfg!(test) {
        return false;
    }
    if instance::is_test() {
        log::info!("read-only test instance: opening a terminal on {host} skipped");
        return false;
    }
    let ssh = ssh_program();
    let prefix = title.map(Launch::wt_prefix).unwrap_or_default();
    let terminal = Command::new("wt.exe")
        .args(&prefix)
        .arg(&ssh)
        .args(&args)
        .spawn();
    if terminal.is_ok() {
        return true;
    }
    let console = Command::new(&ssh)
        .args(&args)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn();
    if let Err(err) = console {
        log::warn!("ClaudeDeck: could not open a terminal on {host}: {err}");
    }
    false
}
