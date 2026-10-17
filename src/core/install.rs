//! `wincraft.exe --install`: the one-time move into a per-user Programs
//! folder, `%LOCALAPPDATA%\Programs\WinCraft`. It is the only copy the
//! in-app updater replaces, so a build in a repo or a download folder is
//! never overwritten. `--uninstall` takes the Start menu shortcut and the
//! logon task away again and leaves the settings and the program alone.

use std::ffi::{c_void, OsString};
use std::fs;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use windows_sys::core::{IUnknown_Vtbl, BOOL, GUID, HRESULT, PCWSTR};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
use windows_sys::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB;

use crate::core::autostart::{self, Installed};
use crate::core::com::{self, ComPtr};
use crate::core::config::{self, Config};
use crate::core::{instance, logging, package, quit, wide};

pub const EXE_NAME: &str = "wincraft.exe";
/// A test instance's install folder, so a live test of `--install` or of an
/// update never touches the real one. Ignored without `WINCRAFT_INSTANCE`.
pub const DIR_ENV: &str = "WINCRAFT_INSTALL_DIR";
/// The copy waits here until the running WinCraft has closed, so a failed
/// copy changes nothing.
const STAGED: &str = "wincraft.install.tmp";

/// Exit codes of `--install` and `--uninstall`.
pub const DONE: i32 = 0;
pub const FAILED: i32 = 1;
pub const REFUSED: i32 = 2;

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

#[repr(C)]
struct IShellLinkWVtbl {
    _base: IUnknown_Vtbl,
    _get_path: unsafe extern "system" fn(),
    _get_id_list: unsafe extern "system" fn(),
    _set_id_list: unsafe extern "system" fn(),
    _get_description: unsafe extern "system" fn(),
    set_description: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    _get_working_directory: unsafe extern "system" fn(),
    set_working_directory: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    _get_arguments: unsafe extern "system" fn(),
    _set_arguments: unsafe extern "system" fn(),
    _get_hotkey: unsafe extern "system" fn(),
    _set_hotkey: unsafe extern "system" fn(),
    _get_show_cmd: unsafe extern "system" fn(),
    _set_show_cmd: unsafe extern "system" fn(),
    _get_icon_location: unsafe extern "system" fn(),
    _set_icon_location: unsafe extern "system" fn(),
    _set_relative_path: unsafe extern "system" fn(),
    _resolve: unsafe extern "system" fn(),
    set_path: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
}

#[repr(C)]
struct IPersistFileVtbl {
    _base: IUnknown_Vtbl,
    _get_class_id: unsafe extern "system" fn(),
    _is_dirty: unsafe extern "system" fn(),
    _load: unsafe extern "system" fn(),
    save: unsafe extern "system" fn(*mut c_void, PCWSTR, BOOL) -> HRESULT,
}

pub fn dir() -> PathBuf {
    dir_from(
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os(DIR_ENV),
        instance::is_test(),
    )
}

fn dir_from(local: Option<OsString>, test_dir: Option<OsString>, test: bool) -> PathBuf {
    if let Some(dir) = test_dir.filter(|dir| test && !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    local
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Programs")
        .join("WinCraft")
}

/// Windows paths compare without case, and `current_exe` may carry the
/// `\\?\` prefix or forward slashes from whoever started the program.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let plain = |path: &Path| {
        let text = path.to_string_lossy().replace('/', "\\");
        text.trim_start_matches(r"\\?\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    plain(a) == plain(b)
}

/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\WinCraft.lnk`; a test
/// instance's carries its suffix, so it can never replace the real one.
pub fn shortcut_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").filter(|dir| !dir.is_empty())?;
    Some(shortcut_in(
        Path::new(&appdata),
        &instance::name("WinCraft"),
    ))
}

fn shortcut_in(appdata: &Path, name: &str) -> PathBuf {
    appdata
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join(format!("{name}.lnk"))
}

/// Starts `exe` outside the job object this process may belong to, so it is
/// not ended together with that job; where breaking away is not allowed, as
/// a plain child.
pub fn start_detached(exe: &Path, args: &[String]) -> std::io::Result<Child> {
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    match command.creation_flags(CREATE_BREAKAWAY_FROM_JOB).spawn() {
        Ok(child) => Ok(child),
        Err(_) => command.creation_flags(0).spawn(),
    }
}

/// Replaces `to` with `from` in one step. std::fs::rename asks for POSIX
/// semantics first, which some filesystems refuse even inside one folder.
pub fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    let moved = unsafe {
        MoveFileExW(
            wide(&from.to_string_lossy()).as_ptr(),
            wide(&to.to_string_lossy()).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) != 0
    };
    if moved {
        Ok(())
    } else {
        Err(format!(
            "cannot move {} to {}: {}",
            from.display(),
            to.display(),
            std::io::Error::last_os_error()
        ))
    }
}

pub fn write_shortcut(link: &Path, target: &Path) -> Result<(), String> {
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let shell_link = ComPtr::create(&CLSID_SHELL_LINK, &IID_ISHELL_LINK_W, CLSCTX_INPROC_SERVER)
        .map_err(|hr| format!("cannot create a shell link ({})", com::hex(hr)))?;
    let calls = unsafe { shell_link.vtable::<IShellLinkWVtbl>() };
    let set = |what: &str,
               call: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
               value: &str| {
        let value = wide(value);
        com::check(unsafe { call(shell_link.as_raw(), value.as_ptr()) })
            .map_err(|hr| format!("cannot set the shortcut's {what} ({})", com::hex(hr)))
    };
    set("target", calls.set_path, &target.to_string_lossy())?;
    if let Some(dir) = target.parent() {
        set(
            "folder",
            calls.set_working_directory,
            &dir.to_string_lossy(),
        )?;
    }
    set(
        "description",
        calls.set_description,
        "Tray utilities, command palette and window layouts",
    )?;
    let file = shell_link
        .query(&IID_IPERSIST_FILE)
        .ok_or_else(|| "the shell link cannot be saved".to_string())?;
    let path = wide(&link.to_string_lossy());
    let saved =
        unsafe { (file.vtable::<IPersistFileVtbl>().save)(file.as_raw(), path.as_ptr(), 1) };
    com::check(saved).map_err(|hr| format!("cannot save {} ({})", link.display(), com::hex(hr)))
}

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Refused(String),
    Failed(String),
}

/// What `--install` does besides copying the exe, so a test can run it on
/// temp folders without touching Windows.
trait System {
    /// The Windows package this process runs inside, if any.
    fn package(&self) -> Option<String>;
    fn quit_running(&mut self) -> quit::Outcome;
    /// Writes the Start menu shortcut and says where.
    fn shortcut(&mut self, target: &Path) -> Result<PathBuf, String>;
    /// Points start at sign-in at `exe` when the user wants it; None when
    /// they do not.
    fn logon_task(&mut self, exe: &Path) -> Result<Option<Installed>, String>;
    /// Starts the installed copy and says how.
    fn start(&mut self, exe: &Path, by_task: bool) -> Result<String, String>;
}

fn refusal(package: &str) -> String {
    format!(
        "--install refused: this terminal runs inside {package}, which redirects the files it \
         writes, so Windows would never see the copy or the shortcut. Run it from a terminal \
         that was not started from {package}, such as Windows Terminal opened from the Start menu."
    )
}

fn install_with(
    source: &Path,
    dir: &Path,
    system: &mut dyn System,
    say: &mut dyn FnMut(&str),
) -> Result<(), Failure> {
    if let Some(package) = system.package() {
        return Err(Failure::Refused(refusal(&package)));
    }
    let target = dir.join(EXE_NAME);
    let staged = dir.join(STAGED);
    let copy = !same_path(source, &target);
    if copy {
        fs::create_dir_all(dir)
            .map_err(|e| Failure::Failed(format!("cannot create {}: {e}", dir.display())))?;
        fs::copy(source, &staged).map_err(|e| {
            Failure::Failed(format!(
                "cannot copy {} to {}: {e}",
                source.display(),
                staged.display()
            ))
        })?;
    } else {
        say(&format!(
            "already running from {}, nothing to copy",
            dir.display()
        ));
    }

    match system.quit_running() {
        quit::Outcome::Closed => say("closed the running WinCraft"),
        quit::Outcome::NotRunning => say("no WinCraft was running"),
        quit::Outcome::StillRunning => {
            let _ = fs::remove_file(&staged);
            return Err(Failure::Failed(
                "the running WinCraft did not close within 10 seconds, so nothing was changed"
                    .to_string(),
            ));
        }
    }

    if copy {
        replace_file(&staged, &target).map_err(|err| {
            let _ = fs::remove_file(&staged);
            Failure::Failed(format!(
                "{err}; WinCraft is closed, start it again from {}",
                source.display()
            ))
        })?;
        say(&format!(
            "copied {} to {}",
            source.display(),
            target.display()
        ));
    }

    match system.shortcut(&target) {
        Ok(link) => say(&format!("Start menu shortcut: {}", link.display())),
        Err(err) => say(&format!("warning: no Start menu shortcut: {err}")),
    }

    let by_task = match system.logon_task(&target) {
        Ok(Some(Installed::Task)) => {
            say(&format!(
                "logon task {} now starts {}",
                autostart::task_label(),
                target.display()
            ));
            true
        }
        Ok(Some(Installed::RunKey)) => {
            say("no logon task could be made; the Run registry value starts it at sign-in");
            false
        }
        Ok(None) => {
            say("Start with Windows is off, so no logon task was made");
            false
        }
        Err(err) => {
            say(&format!("warning: start at sign-in not changed: {err}"));
            false
        }
    };

    let how = system
        .start(&target, by_task)
        .map_err(|err| Failure::Failed(format!("installed, but it did not start: {err}")))?;
    say(&how);
    say(&format!("done: WinCraft is installed in {}", dir.display()));
    Ok(())
}

/// The real thing: Windows' package check, `--quit`, a shell link, Task
/// Scheduler.
struct Windows;

impl Windows {
    /// A file written through `%LOCALAPPDATA%` and where it really landed is
    /// the only sign of a package's redirection: the process has no package
    /// identity of its own.
    fn package_probe() -> Option<String> {
        let probe = config::data_dir().join("install-probe.tmp");
        fs::create_dir_all(config::data_dir()).ok()?;
        fs::write(&probe, b"probe").ok()?;
        let found = package::current(&probe);
        let _ = fs::remove_file(&probe);
        found
    }
}

impl System for Windows {
    fn package(&self) -> Option<String> {
        Self::package_probe()
    }

    fn quit_running(&mut self) -> quit::Outcome {
        quit::run(quit::TIMEOUT)
    }

    fn shortcut(&mut self, target: &Path) -> Result<PathBuf, String> {
        let link = shortcut_path().ok_or_else(|| "APPDATA is not set".to_string())?;
        write_shortcut(&link, target)?;
        Ok(link)
    }

    fn logon_task(&mut self, exe: &Path) -> Result<Option<Installed>, String> {
        if !Config::load().start_with_windows && !autostart::task_exists_now() {
            return Ok(None);
        }
        autostart::point_at(exe).map(Some)
    }

    fn start(&mut self, exe: &Path, by_task: bool) -> Result<String, String> {
        // The task starts WinCraft without this process's environment, so a
        // test instance would come up as the user's own WinCraft.
        if by_task && !instance::is_test() {
            autostart::run_task()?;
            return Ok("started WinCraft through the logon task".to_string());
        }
        start_detached(exe, &[]).map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        Ok(format!("started {}", exe.display()))
    }
}

fn print(line: &str) {
    // Nobody may be reading: a closed pipe must not turn into a panic.
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// Prints each step and writes it to `install.log` next to the settings.
fn reporter() -> impl FnMut(&str) {
    logging::init_to(&config::data_dir().join("install.log"));
    |line: &str| {
        log::info!("{line}");
        print(line);
    }
}

/// Inside a package even the log would land in the hidden copy, so the
/// refusal is only printed.
fn refused_inside_package(flag: &str) -> bool {
    match Windows::package_probe() {
        Some(package) => {
            print(&format!("WinCraft {} {flag}", env!("CARGO_PKG_VERSION")));
            print(&refusal(&package).replace("--install", flag));
            true
        }
        None => false,
    }
}

struct ComApartment(bool);

impl ComApartment {
    fn start() -> Self {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        Self(hr >= 0)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn finish(result: Result<(), Failure>, say: &mut dyn FnMut(&str)) -> i32 {
    match result {
        Ok(()) => DONE,
        Err(Failure::Refused(text)) => {
            say(&text);
            REFUSED
        }
        Err(Failure::Failed(text)) => {
            say(&format!("--install failed: {text}"));
            FAILED
        }
    }
}

/// `wincraft.exe --install`. Safe to run again: from the installed copy
/// it only repairs the shortcut and the task and starts it.
pub fn run() -> i32 {
    if refused_inside_package("--install") {
        return REFUSED;
    }
    let mut say = reporter();
    say(&format!("WinCraft {} --install", env!("CARGO_PKG_VERSION")));
    let source = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            say(&format!("--install failed: cannot find own path: {err}"));
            return FAILED;
        }
    };
    let _com = ComApartment::start();
    let result = install_with(&source, &dir(), &mut Windows, &mut say);
    finish(result, &mut say)
}

/// `wincraft.exe --uninstall`: closes WinCraft, switches start at sign-in
/// off and removes the shortcut. The program and the settings stay.
pub fn uninstall() -> i32 {
    if refused_inside_package("--uninstall") {
        return REFUSED;
    }
    let mut say = reporter();
    say(&format!(
        "WinCraft {} --uninstall",
        env!("CARGO_PKG_VERSION")
    ));
    match quit::run(quit::TIMEOUT) {
        quit::Outcome::StillRunning => {
            say("--uninstall failed: the running WinCraft did not close within 10 seconds");
            return FAILED;
        }
        outcome => say(&outcome.message()),
    }
    let mut failed = false;
    // Off in config.json too, or the next start would make the task again.
    let mut config = Config::load();
    if config.start_with_windows {
        config.start_with_windows = false;
        if let Err(err) = config.save() {
            say(&format!("warning: {err}"));
        }
    }
    match autostart::set(false) {
        Ok(()) => say("start at sign-in removed"),
        Err(err) => {
            say(&format!("could not remove start at sign-in: {err}"));
            failed = true;
        }
    }
    if let Some(link) = shortcut_path().filter(|link| link.exists()) {
        match fs::remove_file(&link) {
            Ok(()) => say(&format!("removed {}", link.display())),
            Err(err) => {
                say(&format!("could not remove {}: {err}", link.display()));
                failed = true;
            }
        }
    }
    say(&format!(
        "left in place: the program in {} and the settings in {}",
        dir().display(),
        config::data_dir().display()
    ));
    if failed {
        FAILED
    } else {
        DONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[derive(Default)]
    struct Fake {
        package: Option<String>,
        quit: Option<quit::Outcome>,
        task: Option<Installed>,
        shortcuts: Vec<PathBuf>,
        tasks: Vec<PathBuf>,
        started: Vec<(PathBuf, bool)>,
    }

    impl System for Fake {
        fn package(&self) -> Option<String> {
            self.package.clone()
        }

        fn quit_running(&mut self) -> quit::Outcome {
            self.quit.unwrap_or(quit::Outcome::Closed)
        }

        fn shortcut(&mut self, target: &Path) -> Result<PathBuf, String> {
            self.shortcuts.push(target.to_path_buf());
            Ok(PathBuf::from("WinCraft.lnk"))
        }

        fn logon_task(&mut self, exe: &Path) -> Result<Option<Installed>, String> {
            self.tasks.push(exe.to_path_buf());
            Ok(self.task.take())
        }

        fn start(&mut self, exe: &Path, by_task: bool) -> Result<String, String> {
            self.started.push((exe.to_path_buf(), by_task));
            Ok("started".to_string())
        }
    }

    #[test]
    fn the_install_folder_is_under_local_app_data_programs() {
        let local = Some(OsString::from(r"C:\Users\Ann\AppData\Local"));
        assert_eq!(
            dir_from(local.clone(), None, false),
            PathBuf::from(r"C:\Users\Ann\AppData\Local\Programs\WinCraft")
        );
        assert_eq!(
            dir_from(local.clone(), Some(OsString::from(r"D:\scratch")), true),
            PathBuf::from(r"D:\scratch")
        );
        // Only a test instance may point it elsewhere.
        assert_eq!(
            dir_from(local.clone(), Some(OsString::from(r"D:\scratch")), false),
            PathBuf::from(r"C:\Users\Ann\AppData\Local\Programs\WinCraft")
        );
        assert_eq!(
            dir_from(local, Some(OsString::new()), true),
            PathBuf::from(r"C:\Users\Ann\AppData\Local\Programs\WinCraft")
        );
    }

    #[test]
    fn paths_match_without_case_prefix_or_slash_style() {
        let installed = Path::new(r"C:\Users\Ann\AppData\Local\Programs\WinCraft\wincraft.exe");
        assert!(same_path(
            Path::new(r"\\?\c:\users\ann\appdata\local\programs\wincraft\WinCraft.exe"),
            installed
        ));
        assert!(same_path(
            Path::new("C:/Users/Ann/AppData/Local/Programs/WinCraft/wincraft.exe"),
            installed
        ));
        assert!(!same_path(
            Path::new(r"E:\code\wincraft\target\release\wincraft.exe"),
            installed
        ));
    }

    #[test]
    fn a_test_instance_shortcut_never_has_the_real_name() {
        let appdata = Path::new(r"C:\Users\Ann\AppData\Roaming");
        assert_eq!(
            shortcut_in(appdata, &instance::with_suffix("WinCraft", "")),
            PathBuf::from(
                r"C:\Users\Ann\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\WinCraft.lnk"
            )
        );
        assert!(
            shortcut_in(appdata, &instance::with_suffix("WinCraft", "t1"))
                .ends_with(r"Programs\WinCraft.t1.lnk")
        );
    }

    #[test]
    fn install_copies_the_exe_and_running_it_again_changes_nothing() {
        let root = scratch("twice");
        let source = root.join("repo").join(EXE_NAME);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"build 1").unwrap();
        let dir = root.join("Programs").join("WinCraft");
        let mut lines = Vec::new();

        let mut first = Fake {
            task: Some(Installed::Task),
            ..Fake::default()
        };
        let result = install_with(&source, &dir, &mut first, &mut |line| {
            lines.push(line.to_string())
        });
        assert_eq!(result, Ok(()));
        let target = dir.join(EXE_NAME);
        assert_eq!(fs::read(&target).unwrap(), b"build 1");
        assert!(!dir.join(STAGED).exists());
        assert_eq!(first.shortcuts, std::slice::from_ref(&target));
        assert_eq!(first.tasks, std::slice::from_ref(&target));
        assert_eq!(first.started, [(target.clone(), true)]);

        // Again from the same build: same files, same shortcut and task.
        let mut again = Fake {
            task: Some(Installed::Task),
            ..Fake::default()
        };
        assert_eq!(install_with(&source, &dir, &mut again, &mut |_| {}), Ok(()));
        assert_eq!(fs::read(&target).unwrap(), b"build 1");
        assert_eq!(again.started, [(target.clone(), true)]);

        // From the installed copy itself: nothing to copy, the rest repaired.
        let mut from_installed = Fake::default();
        lines.clear();
        assert_eq!(
            install_with(&target, &dir, &mut from_installed, &mut |line| {
                lines.push(line.to_string())
            }),
            Ok(())
        );
        assert_eq!(fs::read(&target).unwrap(), b"build 1");
        assert!(lines[0].starts_with("already running from"), "{lines:?}");
        assert_eq!(from_installed.started, [(target.clone(), false)]);
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "only wincraft.exe is left in the folder"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_newer_build_replaces_the_installed_copy() {
        let root = scratch("newer");
        let source = root.join(EXE_NAME);
        let dir = root.join("Programs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(EXE_NAME), b"old").unwrap();
        fs::write(&source, b"new").unwrap();
        assert_eq!(
            install_with(&source, &dir, &mut Fake::default(), &mut |_| {}),
            Ok(())
        );
        assert_eq!(fs::read(dir.join(EXE_NAME)).unwrap(), b"new");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn inside_a_package_install_refuses_and_touches_nothing() {
        let root = scratch("package");
        let source = root.join(EXE_NAME);
        fs::write(&source, b"build").unwrap();
        let dir = root.join("Programs");
        let mut system = Fake {
            package: Some("Claude".to_string()),
            ..Fake::default()
        };
        let result = install_with(&source, &dir, &mut system, &mut |_| {});
        let Err(Failure::Refused(text)) = result else {
            panic!("expected a refusal, got {result:?}");
        };
        assert!(text.contains("runs inside Claude"), "{text}");
        assert!(!dir.exists());
        assert!(system.shortcuts.is_empty() && system.tasks.is_empty());
        assert!(system.started.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_wincraft_that_will_not_close_leaves_the_install_as_it_was() {
        let root = scratch("busy");
        let source = root.join(EXE_NAME);
        fs::write(&source, b"new").unwrap();
        let dir = root.join("Programs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(EXE_NAME), b"old").unwrap();
        let mut system = Fake {
            quit: Some(quit::Outcome::StillRunning),
            ..Fake::default()
        };
        let result = install_with(&source, &dir, &mut system, &mut |_| {});
        assert!(matches!(result, Err(Failure::Failed(_))), "{result:?}");
        assert_eq!(fs::read(dir.join(EXE_NAME)).unwrap(), b"old");
        assert!(!dir.join(STAGED).exists());
        assert!(system.started.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shortcut_is_written_where_it_is_asked_for() {
        let root = scratch("lnk");
        let _com = ComApartment::start();
        let link = root.join("Start Menu").join("WinCraft.test.lnk");
        let target = Path::new(r"C:\Tools\wincraft.exe");
        write_shortcut(&link, target).unwrap();
        let bytes = fs::read(&link).unwrap();
        // A shell link starts with its header size, 0x4C.
        assert_eq!(bytes[..4], [0x4C, 0, 0, 0]);
        let _ = fs::remove_dir_all(&root);
    }
}
