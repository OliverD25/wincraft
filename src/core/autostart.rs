//! Starting WinCraft when the user signs in, and coming back when Windows
//! closes it for an update.
//!
//! Sign-in start is a per-user logon task in Task Scheduler. The registry
//! Run key is only the fallback for when a task cannot be created: on one PC
//! Explorer ran every other Run entry and never attempted WinCraft's.

use std::path::Path;
use std::process::{Command, Stdio};

use std::os::windows::process::CommandExt;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
use windows_sys::Win32::System::Recovery::{
    RegisterApplicationRestart, RESTART_NO_CRASH, RESTART_NO_HANG,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    KEY_WRITE, REG_SZ,
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::core::{decode_console, instance, wide};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "WinCraft";
const TASK_NAME: &str = "WinCraft";

/// The command line Windows gives WinCraft when it starts it again after an
/// update. The program's own name is not part of it.
pub const RESTARTED_FLAG: &str = "--restarted";

fn open(write: bool) -> Option<HKEY> {
    let access = if write {
        KEY_READ | KEY_WRITE
    } else {
        KEY_READ
    };
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            access,
            &mut key,
        )
    };
    if status == ERROR_SUCCESS {
        Some(key)
    } else {
        log::warn!("cannot open the Run registry key (error {status})");
        None
    }
}

fn write_run_value() -> Result<(), String> {
    let key = open(true).ok_or_else(|| "cannot open the Run registry key".to_string())?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own path: {e}"))?;
    let command = wide(&format!("\"{}\"", exe.display()));
    let status = unsafe {
        RegSetValueExW(
            key,
            wide(&instance::name(VALUE_NAME)).as_ptr(),
            0,
            REG_SZ,
            command.as_ptr() as *const u8,
            (command.len() * 2) as u32,
        )
    };
    unsafe { RegCloseKey(key) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("registry write failed (error {status})"))
    }
}

/// Whether there was a value to remove.
fn remove_run_value() -> Result<bool, String> {
    let key = open(true).ok_or_else(|| "cannot open the Run registry key".to_string())?;
    let status = unsafe { RegDeleteValueW(key, wide(&instance::name(VALUE_NAME)).as_ptr()) };
    unsafe { RegCloseKey(key) };
    match status {
        ERROR_SUCCESS => Ok(true),
        // ERROR_FILE_NOT_FOUND: already the state the caller asked for.
        2 => Ok(false),
        _ => Err(format!("registry write failed (error {status})")),
    }
}

fn task_name() -> String {
    instance::name(TASK_NAME)
}

struct Ran {
    code: i32,
    text: String,
}

fn run_schtasks(args: &[&str]) -> Result<Ran, String> {
    let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let exe = Path::new(&system).join("System32").join("schtasks.exe");
    let output = Command::new(&exe)
        .args(args)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", exe.display()))?;
    let mut bytes = output.stdout;
    bytes.extend(output.stderr);
    Ok(Ran {
        code: output.status.code().unwrap_or(-1),
        text: decode_output(&bytes),
    })
}

/// schtasks writes an XML declaration that says UTF-16 over bytes in the
/// console's code page, but a UTF-16 stream with a byte order mark is
/// handled too.
fn decode_output(bytes: &[u8]) -> String {
    match bytes.strip_prefix(&[0xFF, 0xFE]) {
        Some(rest) => {
            let (units, _) = rest.as_chunks::<2>();
            let units: Vec<u16> = units.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
            String::from_utf16_lossy(&units)
        }
        None => decode_console(bytes),
    }
}

/// The file `schtasks /XML` accepts: UTF-16LE with a byte order mark.
fn utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The task's definition. The trigger and the principal both name the user:
/// without that a non-admin user gets "Access is denied" for a logon task.
fn task_xml(user: &str, exe: &Path) -> String {
    let user = xml_escape(user);
    let command = xml_escape(&exe.to_string_lossy());
    let folder = exe.parent().map(|dir| xml_escape(&dir.to_string_lossy()));
    let folder = folder.map_or_else(String::new, |dir| {
        format!("<WorkingDirectory>{dir}</WorkingDirectory>")
    });
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Starts WinCraft when you sign in.</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>PT5S</Delay></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings><Enabled>true</Enabled><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy></Settings>
  <Actions Context="Author"><Exec><Command>{command}</Command>{folder}</Exec></Actions>
</Task>
"#
    )
}

/// The program a task starts, from the XML `schtasks /Query /XML` prints.
fn command_in_xml(xml: &str) -> Option<String> {
    let rest = xml.split_once("<Command>")?.1;
    let command = xml_unescape(rest.split_once("</Command>")?.0);
    let command = command.trim().trim_matches('"');
    (!command.is_empty()).then(|| command.to_string())
}

fn user_name() -> Result<String, String> {
    let mut buffer = [0u16; 256];
    let mut size = buffer.len() as u32;
    let found = unsafe { GetUserNameExW(NameSamCompatible, buffer.as_mut_ptr(), &mut size) };
    if found && size > 0 {
        return Ok(String::from_utf16_lossy(&buffer[..size as usize]));
    }
    match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(domain), Ok(user)) if !user.is_empty() => Ok(format!("{domain}\\{user}")),
        _ => Err("cannot find the user name".to_string()),
    }
}

fn create_task(name: &str, exe: &Path) -> Result<(), String> {
    let xml = task_xml(&user_name()?, exe);
    let file =
        std::env::temp_dir().join(format!("wincraft-task-{}-{name}.xml", std::process::id()));
    std::fs::write(&file, utf16le_with_bom(&xml))
        .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    let ran = run_schtasks(&[
        "/Create",
        "/TN",
        name,
        "/XML",
        &file.to_string_lossy(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&file);
    let ran = ran?;
    if ran.code == 0 {
        Ok(())
    } else {
        Err(format!(
            "schtasks /Create exited with {}: {}",
            ran.code,
            ran.text.trim()
        ))
    }
}

fn task_exists(name: &str) -> bool {
    matches!(run_schtasks(&["/Query", "/TN", name]), Ok(ran) if ran.code == 0)
}

/// `None` when the task is missing or its program cannot be read.
fn queried_command(name: &str) -> Option<String> {
    let ran = run_schtasks(&["/Query", "/TN", name, "/XML"]).ok()?;
    if ran.code != 0 {
        return None;
    }
    command_in_xml(&ran.text)
}

fn delete_task(name: &str) -> Result<(), String> {
    let ran = run_schtasks(&["/Delete", "/TN", name, "/F"])?;
    if ran.code == 0 {
        Ok(())
    } else {
        Err(format!(
            "schtasks /Delete exited with {}: {}",
            ran.code,
            ran.text.trim()
        ))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Check {
    Ok,
    Missing,
    MovedFrom(String),
}

/// What the task found by a query says about the exe that is running now.
fn check(queried: Option<&str>, exe: &Path) -> Check {
    let Some(found) = queried else {
        return Check::Missing;
    };
    if found.to_lowercase() == exe.to_string_lossy().to_lowercase() {
        Check::Ok
    } else {
        Check::MovedFrom(found.to_string())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Installed {
    Task,
    RunKey,
}

/// A task when one can be made, else the Run key. Once the task exists the
/// old Run value goes, or WinCraft would be started twice at sign-in.
fn install_with(
    create: impl FnOnce() -> Result<(), String>,
    write_run: impl FnOnce() -> Result<(), String>,
    remove_run: impl FnOnce() -> bool,
) -> Result<Installed, String> {
    match create() {
        Ok(()) => {
            if remove_run() {
                log::info!("removed the old Run key value, the logon task replaces it");
            }
            Ok(Installed::Task)
        }
        Err(reason) => {
            log::warn!("cannot create the logon task, using the Run key: {reason}");
            write_run()?;
            Ok(Installed::RunKey)
        }
    }
}

fn install() -> Result<Installed, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own path: {e}"))?;
    let name = task_name();
    install_with(
        || create_task(&name, &exe),
        write_run_value,
        || remove_run_value().unwrap_or(false),
    )
}

fn remove() -> Result<(), String> {
    let name = task_name();
    let task = if task_exists(&name) {
        delete_task(&name)
    } else {
        Ok(())
    };
    let run = remove_run_value().map(|_| ());
    task.and(run)
}

/// Turns start-at-sign-in on or off. On, it is the logon task, or the Run
/// key when the task cannot be made; off, both are removed.
pub fn set(enabled: bool) -> Result<(), String> {
    if enabled {
        install().map(|_| ())
    } else {
        remove()
    }
}

/// Makes what Windows will do at sign-in match `start_with_windows`, on its
/// own thread because starting schtasks takes a moment. With the switch on,
/// this repairs a task that was deleted, whose exe was moved, or that a
/// version without tasks never had.
pub fn sync_in_background(wanted: bool) {
    let started = std::thread::Builder::new()
        .name("wincraft-autostart".to_string())
        .spawn(move || sync(wanted));
    if let Err(err) = started {
        log::warn!("could not start the autostart thread: {err}");
    }
}

fn sync(wanted: bool) {
    let name = task_name();
    if !wanted {
        // Only the switch removes the task. A config.json that reads "off"
        // by mistake, such as an old copy in the real profile next to the
        // one a virtualized process wrote, must not silently end autostart.
        if remove_run_value().unwrap_or(false) {
            log::info!("removed the old Run key value, start_with_windows is off");
        }
        if task_exists(&name) {
            log::warn!("the logon task exists but start_with_windows is off; the switch in Settings removes it");
        }
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            log::warn!("cannot find own path: {err}");
            return;
        }
    };
    let why = match check(queried_command(&name).as_deref(), &exe) {
        Check::Ok => {
            let old = remove_run_value().unwrap_or(false);
            log::info!(
                "logon task ok{}",
                if old {
                    ", removed the old Run key value"
                } else {
                    ""
                }
            );
            return;
        }
        Check::Missing => "it was missing".to_string(),
        Check::MovedFrom(path) => format!("it started {path}"),
    };
    match install() {
        Ok(Installed::Task) => log::info!("logon task recreated ({why})"),
        Ok(Installed::RunKey) => log::info!("fell back to the Run key"),
        Err(err) => log::error!("could not set up start at sign-in: {err}"),
    }
}

/// Asks Windows to start WinCraft again when an installer or Windows Update
/// closes it, and after an update restart. Not after a crash or a hang, so a
/// bug can never turn into a restart loop.
pub fn register_restart() {
    let hr = unsafe {
        RegisterApplicationRestart(
            wide(RESTARTED_FLAG).as_ptr(),
            RESTART_NO_CRASH | RESTART_NO_HANG,
        )
    };
    if hr < 0 {
        log::warn!("RegisterApplicationRestart failed (0x{:08X})", hr as u32);
    } else {
        log::info!("registered with Windows to be started again after an update closes it");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::PathBuf;

    const EXE: &str = r"C:\Tools\Win & Co\wincraft.exe";

    #[test]
    fn the_task_starts_the_exe_for_the_user_five_seconds_after_sign_in() {
        let xml = task_xml(r"PC\Ann", Path::new(r"C:\Tools\wincraft.exe"));
        assert!(xml.contains(r"<Command>C:\Tools\wincraft.exe</Command>"));
        assert!(xml.contains(r"<WorkingDirectory>C:\Tools</WorkingDirectory>"));
        assert!(xml.contains(r"<Delay>PT5S</Delay>"));
        assert!(xml.contains(r"<LogonTrigger><Enabled>true</Enabled><UserId>PC\Ann</UserId>"));
        assert!(xml.contains(r"<UserId>PC\Ann</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel>"));
    }

    #[test]
    fn characters_that_mean_something_in_xml_are_escaped() {
        let xml = task_xml(r"PC\A<n>&'x", Path::new(EXE));
        assert!(xml.contains(r"<UserId>PC\A&lt;n&gt;&amp;&apos;x</UserId>"));
        assert!(xml.contains(r"<Command>C:\Tools\Win &amp; Co\wincraft.exe</Command>"));
        assert!(xml.contains(r"<WorkingDirectory>C:\Tools\Win &amp; Co</WorkingDirectory>"));
    }

    #[test]
    fn a_test_instance_gets_its_own_task_name() {
        assert_eq!(instance::with_suffix(TASK_NAME, ""), "WinCraft");
        assert_eq!(
            instance::with_suffix(TASK_NAME, "rmtest"),
            "WinCraft.rmtest"
        );
    }

    #[test]
    fn the_command_comes_back_out_of_the_xml_unescaped() {
        let xml = task_xml(r"PC\Ann", Path::new(EXE));
        assert_eq!(command_in_xml(&xml).as_deref(), Some(EXE));
        assert_eq!(
            command_in_xml("<Exec>\r\r\n<Command>\"C:\\a b\\x.exe\"</Command></Exec>").as_deref(),
            Some(r"C:\a b\x.exe")
        );
        assert_eq!(command_in_xml("<Task></Task>"), None);
        assert_eq!(command_in_xml("<Command> </Command>"), None);
    }

    #[test]
    fn the_task_file_is_utf16_little_endian_with_a_byte_order_mark() {
        assert_eq!(
            utf16le_with_bom("A\u{436}"),
            [0xFF, 0xFE, 0x41, 0x00, 0x36, 0x04]
        );
    }

    #[test]
    fn schtasks_output_is_read_as_utf16_or_as_the_console_code_page() {
        assert_eq!(decode_output(b"<Task/>\r\r\n"), "<Task/>\r\r\n");
        assert_eq!(
            decode_output(&[0xFF, 0xFE, 0x41, 0x00, 0x36, 0x04]),
            "A\u{436}"
        );
        assert_eq!(decode_output(&[]), "");
    }

    #[test]
    fn a_task_for_another_exe_or_no_task_needs_repair() {
        let exe = PathBuf::from(EXE);
        assert_eq!(check(Some(EXE), &exe), Check::Ok);
        assert_eq!(check(Some(&EXE.to_uppercase()), &exe), Check::Ok);
        assert_eq!(check(None, &exe), Check::Missing);
        assert_eq!(
            check(Some(r"D:\old\wincraft.exe"), &exe),
            Check::MovedFrom(r"D:\old\wincraft.exe".to_string())
        );
    }

    #[test]
    fn a_created_task_replaces_the_run_value() {
        let removed = Cell::new(false);
        let wrote = Cell::new(false);
        let result = install_with(
            || Ok(()),
            || {
                wrote.set(true);
                Ok(())
            },
            || {
                removed.set(true);
                true
            },
        );
        assert_eq!(result, Ok(Installed::Task));
        assert!(removed.get());
        assert!(!wrote.get());
    }

    #[test]
    fn a_task_that_cannot_be_created_falls_back_to_the_run_value() {
        let removed = Cell::new(false);
        let wrote = Cell::new(false);
        let result = install_with(
            || Err("Access is denied".to_string()),
            || {
                wrote.set(true);
                Ok(())
            },
            || {
                removed.set(true);
                true
            },
        );
        assert_eq!(result, Ok(Installed::RunKey));
        assert!(wrote.get());
        assert!(!removed.get());
    }

    #[test]
    fn with_no_task_and_no_run_value_the_error_is_the_registry_one() {
        let result = install_with(
            || Err("no schtasks".to_string()),
            || Err("registry write failed".to_string()),
            || false,
        );
        assert_eq!(result, Err("registry write failed".to_string()));
    }

    #[test]
    fn windows_keeps_the_restart_registration_without_crash_and_hang_restarts() {
        use windows_sys::Win32::System::Recovery::GetApplicationRestartSettings;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        register_restart();
        let mut line = [0u16; 64];
        let mut size = line.len() as u32;
        let mut flags = 0u32;
        let hr = unsafe {
            GetApplicationRestartSettings(
                GetCurrentProcess(),
                line.as_mut_ptr(),
                &mut size,
                &mut flags,
            )
        };
        assert!(hr >= 0, "0x{:08X}", hr as u32);
        let text = String::from_utf16_lossy(&line[..size as usize - 1]);
        assert_eq!(text, RESTARTED_FLAG);
        assert_eq!(flags, RESTART_NO_CRASH | RESTART_NO_HANG);
    }
}
