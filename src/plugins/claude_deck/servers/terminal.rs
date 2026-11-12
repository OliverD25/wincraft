//! "Open a terminal there": Windows Terminal with ssh into a server chat's
//! folder, or a plain console window when Windows Terminal is missing.
//! Nothing is changed on the server: the shell only starts in the folder.

use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE;

use crate::core::instance;

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
fn safe_path(path: &str) -> bool {
    path.starts_with('/')
        && !path
            .chars()
            .any(|c| matches!(c, '\'' | '"' | ';' | '\\' | '`' | '$') || c.is_control())
}

/// The arguments of ssh for an interactive shell there.
pub fn ssh_args(host: &str, cwd: Option<&str>) -> Vec<String> {
    vec![
        "-t".to_string(),
        "--".to_string(),
        host.to_string(),
        remote_command(cwd),
    ]
}

/// Opens the terminal. A test copy of WinCraft opens nothing.
pub fn open(host: &str, cwd: Option<&str>) {
    if instance::is_test() {
        log::info!("read-only test instance: opening a terminal on {host} skipped");
        return;
    }
    let ssh = ssh_program();
    let args = ssh_args(host, cwd);
    let terminal = Command::new("wt.exe").arg(&ssh).args(&args).spawn();
    if terminal.is_ok() {
        return;
    }
    let console = Command::new(&ssh)
        .args(&args)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn();
    if let Err(err) = console {
        log::warn!("ClaudeDeck: could not open a terminal on {host}: {err}");
    }
}
