use std::sync::OnceLock;

use windows_sys::Win32::Storage::FileSystem::SearchPathW;

use crate::core::wide;

/// The names in config.json's `search.terminal_shell`, in the order the
/// settings dropdown lists them.
pub const SHELL_NAMES: [&str; 5] = [
    "WSL bash",
    "PowerShell 7",
    "Windows PowerShell",
    "Command Prompt",
    "Custom",
];
pub const DEFAULT_SHELL: &str = "WSL bash";

/// What the `>` prefix runs a command in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shell {
    Wsl,
    Pwsh,
    WindowsPowerShell,
    Cmd,
    /// A command template holding `{cmd}` and perhaps `{cwd}`.
    Custom(String),
}

/// A process to start: its whole command line, and the folder it starts in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub command_line: String,
    pub current_dir: Option<String>,
}

impl Shell {
    /// The shell the settings name. One that cannot be used falls back to WSL
    /// bash and says why, for the warning under the setting.
    pub fn from_settings(
        name: &str,
        custom: &str,
        pwsh_installed: bool,
    ) -> (Shell, Option<String>) {
        match name {
            "PowerShell 7" if pwsh_installed => (Shell::Pwsh, None),
            "PowerShell 7" => (
                Shell::Wsl,
                Some("PowerShell 7 is not installed, so WSL bash is used.".to_string()),
            ),
            "Windows PowerShell" => (Shell::WindowsPowerShell, None),
            "Command Prompt" => (Shell::Cmd, None),
            "Custom" => match custom_problem(custom) {
                None => (Shell::Custom(custom.trim().to_string()), None),
                Some(problem) => (Shell::Wsl, Some(format!("{problem} WSL bash is used."))),
            },
            _ => (Shell::Wsl, None),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Shell::Wsl => "WSL bash",
            Shell::Pwsh => "PowerShell 7",
            Shell::WindowsPowerShell => "Windows PowerShell",
            Shell::Cmd => "Command Prompt",
            Shell::Custom(_) => "the custom shell",
        }
    }

    /// Runs `cmd` once and exits. `cwd` is a Windows folder.
    pub fn run(&self, cmd: &str, cwd: Option<&str>) -> Launch {
        match self {
            Shell::Wsl => Launch {
                command_line: format!(
                    "wsl.exe --cd {} -e bash -lc {}",
                    quote(&wsl_folder(cwd)),
                    quote(cmd)
                ),
                current_dir: None,
            },
            Shell::Pwsh | Shell::WindowsPowerShell => Launch {
                command_line: format!(
                    "{} -NoLogo -NoProfile -Command {}",
                    self.program(),
                    quote(cmd)
                ),
                current_dir: cwd.map(str::to_string),
            },
            // With /s, cmd drops the first and the last quote and runs what
            // is between them exactly as typed.
            Shell::Cmd => Launch {
                command_line: format!("cmd.exe /d /s /c \"{cmd}\""),
                current_dir: cwd.map(str::to_string),
            },
            Shell::Custom(template) => Launch {
                command_line: template
                    .replace("{cwd}", &quote(cwd.unwrap_or("")))
                    .replace("{cmd}", &quote(cmd)),
                current_dir: cwd.map(str::to_string),
            },
        }
    }

    /// Runs `cmd` and then stays open for more, for a real terminal.
    pub fn interactive(&self, cmd: &str, cwd: Option<&str>) -> Launch {
        match self {
            Shell::Wsl => Launch {
                command_line: format!(
                    "wsl.exe --cd {} -e bash -lc {}",
                    quote(&wsl_folder(cwd)),
                    quote(&format!("{cmd}; exec bash"))
                ),
                current_dir: None,
            },
            Shell::Pwsh | Shell::WindowsPowerShell => Launch {
                command_line: format!("{} -NoLogo -NoExit -Command {}", self.program(), quote(cmd)),
                current_dir: cwd.map(str::to_string),
            },
            Shell::Cmd => Launch {
                command_line: format!("cmd.exe /d /s /k \"{cmd}\""),
                current_dir: cwd.map(str::to_string),
            },
            Shell::Custom(_) => self.run(cmd, cwd),
        }
    }

    fn program(&self) -> &'static str {
        match self {
            Shell::Pwsh => "pwsh.exe",
            Shell::WindowsPowerShell => "powershell.exe",
            Shell::Cmd => "cmd.exe",
            Shell::Wsl | Shell::Custom(_) => "wsl.exe",
        }
    }
}

/// Windows Terminal's command line for a new tab running `launch`. Windows
/// Terminal splits its own command line at every `;` unless it is written
/// `\;`, which would cut "cmd; exec bash" in two.
pub fn terminal_tab(launch: &Launch) -> String {
    let folder = launch
        .current_dir
        .as_deref()
        .map(|dir| format!("-d {} ", quote(dir)))
        .unwrap_or_default();
    format!(
        "wt.exe -w 0 nt {folder}{}",
        launch.command_line.replace(';', "\\;")
    )
}

/// Why a custom template cannot be used, if it cannot.
pub fn custom_problem(template: &str) -> Option<&'static str> {
    if template.trim().is_empty() {
        Some("The custom command is empty.")
    } else if !template.contains("{cmd}") {
        Some("The custom command needs {cmd} where the command goes.")
    } else {
        None
    }
}

fn wsl_folder(cwd: Option<&str>) -> String {
    cwd.and_then(wsl_path).unwrap_or_else(|| "~".to_string())
}

/// C:\Users\Admin\ → /mnt/c/Users/Admin. A network path has no /mnt form.
pub fn wsl_path(windows: &str) -> Option<String> {
    let mut chars = windows.chars();
    let drive = chars.next()?.to_ascii_lowercase();
    if !drive.is_ascii_alphabetic() || chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str().replace('\\', "/");
    let rest = rest.trim_end_matches('/');
    Some(format!("/mnt/{drive}{rest}"))
}

/// Quotes one argument so CommandLineToArgvW, and every program that parses
/// its command line the Microsoft C way, reads it back unchanged.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Whether pwsh.exe is on PATH. Asked once; installing PowerShell 7 while
/// WinCraft runs needs a restart to be noticed.
pub fn pwsh_installed() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        let mut buffer = [0u16; 1024];
        let found = unsafe {
            SearchPathW(
                std::ptr::null(),
                wide("pwsh.exe").as_ptr(),
                std::ptr::null(),
                buffer.len() as u32,
                buffer.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        };
        found != 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

    /// Splits a command line the way the started program will.
    fn argv(line: &str) -> Vec<String> {
        let mut count = 0;
        let list = unsafe { CommandLineToArgvW(wide(line).as_ptr(), &mut count) };
        assert!(!list.is_null());
        let args = (0..count as usize)
            .map(|index| unsafe { crate::core::from_wide_ptr(*list.add(index)) })
            .collect();
        unsafe { LocalFree(list as _) };
        args
    }

    const TRICKY: &str = r#"echo "a  b" 'c d' \"e\" C:\path\ end\"#;

    #[test]
    fn quoting_survives_the_windows_command_line_parser() {
        for arg in [TRICKY, "", "plain", r"C:\dir with space\", "x\"y", r"a\\b"] {
            let parsed = argv(&format!("program.exe {}", quote(arg)));
            assert_eq!(parsed, ["program.exe", arg], "for {arg:?}");
        }
    }

    #[test]
    fn wsl_gets_the_command_as_one_argument_and_a_linux_folder() {
        let launch = Shell::Wsl.run(TRICKY, Some(r"C:\Users\Admin\"));
        assert_eq!(
            argv(&launch.command_line),
            [
                "wsl.exe",
                "--cd",
                "/mnt/c/Users/Admin",
                "-e",
                "bash",
                "-lc",
                TRICKY
            ]
        );
        assert_eq!(launch.current_dir, None);
        let home = Shell::Wsl.run("ls", None);
        assert_eq!(argv(&home.command_line)[2], "~");
    }

    #[test]
    fn powershell_gets_the_command_as_one_argument_and_a_windows_folder() {
        for (shell, program) in [
            (Shell::Pwsh, "pwsh.exe"),
            (Shell::WindowsPowerShell, "powershell.exe"),
        ] {
            let launch = shell.run(TRICKY, Some(r"D:\work"));
            assert_eq!(
                argv(&launch.command_line),
                [program, "-NoLogo", "-NoProfile", "-Command", TRICKY]
            );
            assert_eq!(launch.current_dir.as_deref(), Some(r"D:\work"));
        }
    }

    #[test]
    fn cmd_gets_the_command_between_one_pair_of_outer_quotes() {
        let launch = Shell::Cmd.run(r#"dir "C:\Program Files" & echo "done""#, None);
        assert_eq!(
            launch.command_line,
            r#"cmd.exe /d /s /c "dir "C:\Program Files" & echo "done"""#
        );
    }

    #[test]
    fn a_custom_template_fills_in_the_command_and_folder() {
        let shell = Shell::Custom(r"C:\msys64\usr\bin\bash.exe -lc {cmd} --dir {cwd}".to_string());
        let launch = shell.run("echo \"hi there\"", Some(r"C:\x y"));
        assert_eq!(
            argv(&launch.command_line),
            [
                r"C:\msys64\usr\bin\bash.exe",
                "-lc",
                "echo \"hi there\"",
                "--dir",
                r"C:\x y"
            ]
        );
    }

    #[test]
    fn a_shell_that_cannot_be_used_falls_back_to_wsl_and_says_why() {
        assert_eq!(
            Shell::from_settings("Command Prompt", "", false).0,
            Shell::Cmd
        );
        let (shell, why) = Shell::from_settings("PowerShell 7", "", false);
        assert_eq!(shell, Shell::Wsl);
        assert!(why.unwrap().contains("not installed"));
        assert_eq!(
            Shell::from_settings("PowerShell 7", "", true).0,
            Shell::Pwsh
        );
        let (shell, why) = Shell::from_settings("Custom", "bash -lc", true);
        assert_eq!(shell, Shell::Wsl);
        assert!(why.unwrap().contains("{cmd}"));
        assert_eq!(
            Shell::from_settings("nonsense", "", true),
            (Shell::Wsl, None)
        );
    }

    #[test]
    fn the_terminal_keeps_the_shell_open_and_escapes_semicolons() {
        let tab = terminal_tab(&Shell::Wsl.interactive("make; ls", Some(r"E:\dev")));
        assert_eq!(
            tab,
            r#"wt.exe -w 0 nt wsl.exe --cd /mnt/e/dev -e bash -lc "make\; ls\; exec bash""#
        );
        let tab = terminal_tab(&Shell::Cmd.interactive("dir", Some(r"C:\a b")));
        assert_eq!(tab, r#"wt.exe -w 0 nt -d "C:\a b" cmd.exe /d /s /k "dir""#);
        let tab = terminal_tab(&Shell::Pwsh.interactive("Get-Date", None));
        assert_eq!(
            tab,
            "wt.exe -w 0 nt pwsh.exe -NoLogo -NoExit -Command Get-Date"
        );
    }

    #[test]
    fn windows_folders_become_mnt_paths() {
        assert_eq!(
            wsl_path(r"C:\Users\Admin\").as_deref(),
            Some("/mnt/c/Users/Admin")
        );
        assert_eq!(wsl_path(r"e:\").as_deref(), Some("/mnt/e"));
        assert_eq!(wsl_path(r"D:\a b\c").as_deref(), Some("/mnt/d/a b/c"));
        assert_eq!(wsl_path(r"\\nas\share\"), None);
        assert_eq!(wsl_path(""), None);
    }
}
