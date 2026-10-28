//! Updates from GitHub releases. WinCraft asks every hour (and after a wake,
//! and when the palette opens after a quiet while; see `update_timing`)
//! whether a newer release exists and says so. Nothing is installed until
//! the user clicks Update now.
//!
//! Update now downloads `wincraft.exe` and `SHA256SUMS.txt` from the
//! release, checks the checksum, and swaps the files: Windows lets a running
//! exe be renamed, never replaced, so the running one becomes
//! `wincraft.old.exe` and the download takes its name. The old process then
//! shuts down like Quit, starts the new one and waits for it to say it is
//! healthy. If it does not, the old process kills it, swaps the files back
//! and starts the old version again.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, WaitForMultipleObjects, EVENT_MODIFY_STATE,
};

use crate::core::install::{self, EXE_NAME};
use crate::core::{config, instance, logging, package, release_notes, sha256, wide};
use crate::store::github::{self, Release};
use crate::store::release_lookup::CheckError;

pub const EXE_ASSET: &str = "wincraft.exe";
pub const SUMS_ASSET: &str = "SHA256SUMS.txt";
/// Next to the installed exe while an update is under way.
pub const NEW_EXE: &str = "wincraft.new.exe";
pub const OLD_EXE: &str = "wincraft.old.exe";
pub const FAILED_EXE: &str = "wincraft.failed.exe";
const DOWNLOADS: &str = "https://github.com/OliverD25/wincraft/releases/download/";
/// Far above the real exe; a server that sends more is not sending WinCraft.
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The new process starts with `--updated-from <old version>` and the old
/// one, after a rollback, with `--update-failed <new version>`.
pub const UPDATED_FROM: &str = "--updated-from";
pub const UPDATE_FAILED: &str = "--update-failed";
/// Given after either of those, only when the user started the update from
/// the palette or from About: `--reopen palette` or `--reopen about`.
pub const REOPEN: &str = "--reopen";

/// What the user was looking at when they started an update, so the next
/// start can open it again. An unattended update or the tray menu has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reopen {
    Palette,
    About,
}

impl Reopen {
    pub fn word(self) -> &'static str {
        match self {
            Reopen::Palette => "palette",
            Reopen::About => "about",
        }
    }

    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "palette" => Some(Reopen::Palette),
            "about" => Some(Reopen::About),
            _ => None,
        }
    }
}

/// What a starting process reopens: only when an update (or its rollback)
/// started it, so a normal start never opens anything by itself.
pub fn reopen_at_start(started_by_update: bool, word: Option<&str>) -> Option<Reopen> {
    word.filter(|_| started_by_update)
        .and_then(Reopen::from_word)
}

/// The arguments the next process starts with: what started it, the version
/// that goes with it, and where to reopen.
fn start_args(flag: &str, version: &str, reopen: Option<Reopen>) -> Vec<String> {
    let mut args = vec![flag.to_string(), version.to_string()];
    if let Some(reopen) = reopen {
        args.push(REOPEN.to_string());
        args.push(reopen.word().to_string());
    }
    args
}
/// How long the new version must run before it counts as healthy: by then
/// it has started its plugins and registered its hotkeys, and the usual
/// crash at start would have happened.
pub const HEALTHY_AFTER: Duration = Duration::from_secs(30);
/// How long the old version waits for that signal; twice the above, for a
/// slow start on a busy PC.
pub const HANDOVER_WAIT: Duration = Duration::from_secs(60);
const HEALTHY_EVENT: &str = r"Local\WinCraft.Healthy";

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// A test instance checks on its own only against a fake releases API, so
/// a test run never pops up the real "update available" notice.
pub fn automatic_checks_allowed() -> bool {
    !instance::is_test() || github::releases_override().is_some()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Checked {
    NoRelease,
    UpToDate(String),
    Newer(Release),
}

pub fn classify(release: Option<Release>, current: &str) -> Checked {
    match release {
        None => Checked::NoRelease,
        Some(release) if github::is_newer(&release.tag, current) => Checked::Newer(release),
        Some(release) => Checked::UpToDate(release.tag),
    }
}

/// What one update check found, and what it learned about GitHub's limit.
#[derive(Debug)]
pub struct Asked {
    pub result: Result<Checked, CheckError>,
    /// The API said it is limiting requests: seconds until that lifts, when
    /// it said.
    pub limited: Option<Option<u64>>,
}

/// Asks GitHub; slow, so never on the host or UI thread. `use_api` false
/// asks only the releases page, which GitHub's API limit does not cover.
pub fn check(current: &str, use_api: bool) -> Asked {
    let lookup = github::latest_release(use_api);
    Asked {
        result: lookup.result.map(|release| {
            if let Some(release) = &release {
                keep_notes_if_installed(release, current);
            }
            classify(release, current)
        }),
        limited: lookup.limited,
    }
}

/// A release that is the running version has the notes About shows under
/// "What's new"; they are kept for the next time it is opened.
pub fn keep_notes_if_installed(release: &Release, current: &str) {
    if release.tag.trim_start_matches(['v', 'V']) == current {
        release_notes::store(&release.tag, &release.notes);
    }
}

/// Why Update now cannot run in this copy, or None when it can. Only the
/// installed copy is replaced, and never from inside a package, whose file
/// redirection would put the new exe where Windows does not look.
pub fn blocked(exe: &Path, package: Option<&str>) -> Option<String> {
    if let Some(package) = package {
        return Some(format!(
            "WinCraft runs inside {package}, so it cannot update itself. \
             Start it from the Start menu."
        ));
    }
    if !install::is_installed_copy(exe) {
        return Some(format!(
            "Updates install into {}. Run wincraft.exe --install once first.",
            short_path(&install::dir())
        ));
    }
    None
}

/// "%LOCALAPPDATA%\Programs\WinCraft" says the same as the full path in a
/// third of the width.
fn short_path(path: &Path) -> String {
    let full = path.display().to_string();
    match std::env::var("LOCALAPPDATA") {
        Ok(base) if !base.is_empty() && full.starts_with(&base) => {
            format!("%LOCALAPPDATA%{}", &full[base.len()..])
        }
        _ => full,
    }
}

/// The package this process runs in, for `blocked`; the log is a file it
/// wrote through `%LOCALAPPDATA%`.
pub fn current_package() -> Option<String> {
    package::current(&config::log_path())
}

/// Only WinCraft's own release downloads, over HTTPS. A test instance with
/// a fake releases API may also fetch from that fake on this PC.
pub fn allowed_asset(url: &str, local_fake: bool) -> bool {
    url.starts_with(DOWNLOADS) || (local_fake && url.starts_with("http://127.0.0.1:"))
}

/// The error of a download the user cancelled; not a failure to report.
pub const CANCELLED: &str = "cancelled";

pub trait Fetch {
    fn text(&self, url: &str) -> Result<String, String>;
    /// Writes the body to `dest`, telling `progress` the bytes so far and
    /// the total when the server gave one. `progress` answers false to stop
    /// the download: the result is then `Err(CANCELLED)`.
    fn to_file(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<(), String>;
}

/// The network, through ureq.
pub struct Web;

impl Fetch for Web {
    fn text(&self, url: &str) -> Result<String, String> {
        github::get(url)
    }

    fn to_file(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<(), String> {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .user_agent(concat!("WinCraft/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        let mut response = agent.get(url).call().map_err(|err| format!("{err}"))?;
        let total = response.body().content_length();
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD)
            .reader();
        let mut file =
            File::create(dest).map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|e| format!("the download stopped: {e}"))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
            done += read as u64;
            if !progress(done, total) {
                return Err(CANCELLED.to_string());
            }
        }
        file.sync_all()
            .map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
        match total {
            Some(total) if total != done => Err(format!(
                "the download stopped after {done} of {total} bytes"
            )),
            _ => Ok(()),
        }
    }
}

/// Downloads the release's exe as `wincraft.new.exe` in `dir` and checks it
/// against the release's checksum. A file that does not match is deleted.
pub fn download(
    release: &Release,
    dir: &Path,
    fetch: &dyn Fetch,
    local_fake: bool,
    progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<PathBuf, String> {
    download_as(release, dir, NEW_EXE, fetch, local_fake, progress).map(|(path, _)| path)
}

/// `download` into `dir/name`, giving back the checksum it matched too.
pub fn download_as(
    release: &Release,
    dir: &Path,
    name: &str,
    fetch: &dyn Fetch,
    local_fake: bool,
    progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<(PathBuf, String), String> {
    let asset = |name: &str| {
        release
            .assets
            .iter()
            .find(|asset| asset.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("release {} has no {name}", release.tag))
    };
    let exe = asset(EXE_ASSET)?;
    let sums = asset(SUMS_ASSET)?;
    for asset in [exe, sums] {
        if !allowed_asset(&asset.url, local_fake) {
            return Err(format!(
                "refused to download {}: it is not a WinCraft release download on GitHub",
                asset.url
            ));
        }
    }
    let expected = sha256::expected(&fetch.text(&sums.url)?, EXE_ASSET).ok_or_else(|| {
        format!(
            "{SUMS_ASSET} of {} has no line for {EXE_ASSET}",
            release.tag
        )
    })?;
    let dest = dir.join(name);
    let _ = fs::remove_file(&dest);
    let checked = fetch
        .to_file(&exe.url, &dest, progress)
        .and_then(|()| sha256::of_file(&dest))
        .and_then(|actual| {
            if actual == expected {
                Ok(())
            } else {
                Err(format!(
                    "the download does not match its checksum \
                     (expected {expected}, got {actual}), so it was deleted"
                ))
            }
        });
    match checked {
        Ok(()) => Ok((dest, expected)),
        Err(err) => {
            let _ = fs::remove_file(&dest);
            Err(err)
        }
    }
}

/// File moves, so the swap and the rollback can be tested without touching
/// a real exe.
pub trait Files {
    fn exists(&self, path: &Path) -> bool;
    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), String>;
    fn remove(&mut self, path: &Path) -> Result<(), String>;
}

pub struct Disk;

impl Disk {
    /// A virus scanner reading the new exe, or a process that has not quite
    /// ended, keeps a file open for a moment.
    fn patiently(mut step: impl FnMut() -> Result<(), String>) -> Result<(), String> {
        let mut tries = 0;
        loop {
            match step() {
                Ok(()) => return Ok(()),
                Err(_) if tries < 20 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(250));
                }
                Err(err) => return Err(err),
            }
        }
    }
}

impl Files for Disk {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        Self::patiently(|| install::replace_file(from, to))
    }

    fn remove(&mut self, path: &Path) -> Result<(), String> {
        Self::patiently(|| {
            fs::remove_file(path).map_err(|e| format!("cannot delete {}: {e}", path.display()))
        })
    }
}

/// Puts the downloaded exe in the running one's place. When the second
/// rename fails, the first is undone, so the folder never lacks an exe.
pub fn swap_in(dir: &Path, files: &mut dyn Files) -> Result<(), String> {
    let exe = dir.join(EXE_NAME);
    let new = dir.join(NEW_EXE);
    let old = dir.join(OLD_EXE);
    if files.exists(&old) {
        files.remove(&old)?;
    }
    files.rename(&exe, &old)?;
    if let Err(err) = files.rename(&new, &exe) {
        return match files.rename(&old, &exe) {
            Ok(()) => Err(err),
            Err(back) => Err(format!(
                "{err}; putting the old exe back failed too: {back}"
            )),
        };
    }
    Ok(())
}

/// The other way: the old exe back in place, the new one deleted.
fn swap_back(dir: &Path, files: &mut dyn Files) -> Result<(), String> {
    let exe = dir.join(EXE_NAME);
    let old = dir.join(OLD_EXE);
    let failed = dir.join(FAILED_EXE);
    if !files.exists(&old) {
        return Err(format!(
            "{OLD_EXE} is gone, so there is nothing to go back to"
        ));
    }
    if files.exists(&failed) {
        files.remove(&failed)?;
    }
    if files.exists(&exe) {
        files.rename(&exe, &failed)?;
    }
    files.rename(&old, &exe)?;
    let _ = files.remove(&failed);
    Ok(())
}

/// The files an update leaves next to the installed exe: the old version
/// once the new one runs, a download that never finished, a version that
/// was rolled back. Returns the ones still there.
pub fn remove_leftovers(dir: &Path, files: &mut dyn Files) -> Vec<String> {
    let mut left = Vec::new();
    for name in [OLD_EXE, NEW_EXE, FAILED_EXE] {
        let path = dir.join(name);
        if !files.exists(&path) {
            continue;
        }
        match files.remove(&path) {
            Ok(()) => log::info!("deleted {name}, left over from an update"),
            Err(_) => left.push(name.to_string()),
        }
    }
    left
}

/// After the new version reported healthy: the old process ends a moment
/// later, and only then can its exe be deleted.
pub fn remove_leftovers_soon(dir: PathBuf) {
    let started = std::thread::Builder::new()
        .name("wincraft-update-cleanup".to_string())
        .spawn(move || {
            for _ in 0..6 {
                let left = remove_leftovers(&dir, &mut Disk);
                if left.is_empty() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(5));
            }
            log::warn!("could not delete what the update left over; the next start tries again");
        });
    if let Err(err) = started {
        log::warn!("could not start the update cleanup: {err}");
    }
}

/// What the old process does after its host stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handover {
    pub dir: PathBuf,
    pub from: String,
    pub to: String,
    /// Opened again by whichever version runs next, the new one or, after a
    /// rollback, the old one: the user is waiting for it either way.
    pub reopen: Option<Reopen>,
}

static PENDING: Mutex<Option<Handover>> = Mutex::new(None);

/// Set by the host right before it shuts down for an update.
pub fn hand_over_at_exit(plan: Handover) {
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some(plan);
    }
}

pub fn take_handover() -> Option<Handover> {
    PENDING.lock().ok().and_then(|mut pending| pending.take())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waited {
    Healthy,
    Exited(Option<u32>),
    TimedOut,
}

pub trait Launcher {
    fn start(&mut self, exe: &Path, args: &[String]) -> Result<Box<dyn Started>, String>;
}

pub trait Started {
    /// Until the new version says it is healthy or ends, whichever is first.
    fn wait(&mut self, timeout: Duration) -> Waited;
    /// Ends it and waits until it is gone, so its exe can be moved.
    fn kill(&mut self);
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Healthy,
    /// The old version is back and running; the reason says what went wrong.
    RolledBack(String),
    /// Going back did not work either.
    Broken(String),
}

pub fn hand_over(
    plan: &Handover,
    launcher: &mut dyn Launcher,
    files: &mut dyn Files,
    wait: Duration,
) -> Outcome {
    let exe = plan.dir.join(EXE_NAME);
    let reason = match launcher.start(&exe, &start_args(UPDATED_FROM, &plan.from, plan.reopen)) {
        Err(err) => format!("{} did not start: {err}", plan.to),
        Ok(mut started) => match started.wait(wait) {
            Waited::Healthy => return Outcome::Healthy,
            Waited::Exited(code) => format!(
                "{} closed{} before it reported healthy",
                plan.to,
                code.map(|code| format!(" with exit code {code}"))
                    .unwrap_or_default()
            ),
            Waited::TimedOut => {
                started.kill();
                format!(
                    "{} did not report healthy within {} seconds and was ended",
                    plan.to,
                    wait.as_secs()
                )
            }
        },
    };
    let again = start_args(UPDATE_FAILED, &plan.to, plan.reopen);
    if let Err(err) = swap_back(&plan.dir, files) {
        // Better the old version from its odd name than no WinCraft at all.
        let old = plan.dir.join(OLD_EXE);
        let started = files.exists(&old) && launcher.start(&old, &again).is_ok();
        return Outcome::Broken(format!(
            "{reason}; putting {} back failed: {err}{}",
            plan.from,
            if started {
                format!("; started it from {OLD_EXE}")
            } else {
                String::new()
            }
        ));
    }
    match launcher.start(&exe, &again) {
        Ok(_) => Outcome::RolledBack(reason),
        Err(err) => Outcome::Broken(format!(
            "{reason}; {} is back in place but did not start: {err}",
            plan.from
        )),
    }
}

/// The named event the new version sets once healthy. Created by the old
/// process before it starts the new one, so the signal cannot come too
/// early to be seen.
pub struct HealthSignal(HANDLE);

impl HealthSignal {
    fn named(name: &str) -> Result<Self, String> {
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide(name).as_ptr()) };
        if handle.is_null() {
            Err(format!(
                "cannot create {name}: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for HealthSignal {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

impl Launcher for HealthSignal {
    fn start(&mut self, exe: &Path, args: &[String]) -> Result<Box<dyn Started>, String> {
        let child = install::start_detached(exe, args)
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        Ok(Box::new(Process {
            child,
            event: self.0,
        }))
    }
}

struct Process {
    child: Child,
    /// Borrowed from the HealthSignal, which outlives every Process.
    event: HANDLE,
}

impl Started for Process {
    fn wait(&mut self, timeout: Duration) -> Waited {
        let handles = [self.event, self.child.as_raw_handle() as HANDLE];
        let millis = timeout.as_millis().min(u32::MAX as u128 - 1) as u32;
        // The event comes first, so a version that said it is healthy and
        // then quit counts as healthy.
        let waited = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, millis) };
        if waited == WAIT_OBJECT_0 {
            Waited::Healthy
        } else if waited == WAIT_OBJECT_0 + 1 {
            let code = self.child.try_wait().ok().flatten().and_then(|s| s.code());
            Waited::Exited(code.map(|code| code as u32))
        } else {
            Waited::TimedOut
        }
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn signal(name: &str) -> bool {
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, wide(name).as_ptr()) };
    if handle.is_null() {
        return false;
    }
    let set = unsafe { SetEvent(handle) } != 0;
    unsafe { CloseHandle(handle) };
    set
}

/// The new version's half: tells the old one it is healthy. False when no
/// old version is waiting.
pub fn report_healthy() -> bool {
    signal(&instance::name(HEALTHY_EVENT))
}

/// The old process, after its host stopped and its single-instance mutex
/// was freed: starts the new version and waits for it. The exit code is 0
/// when the new version took over, 1 after a rollback, 2 when that failed
/// too.
pub fn run_handover(plan: &Handover) -> i32 {
    // The new version writes the same log from now on.
    logging::reopen_appending();
    log::info!(
        "starting {} and waiting up to {} seconds for it to report healthy",
        plan.to,
        HANDOVER_WAIT.as_secs()
    );
    let outcome = match HealthSignal::named(&instance::name(HEALTHY_EVENT)) {
        Ok(mut signal) => hand_over(plan, &mut signal, &mut Disk, HANDOVER_WAIT),
        Err(err) => {
            // Without the event no signal could ever arrive, so the new
            // version is not started at all.
            log::error!("{err}");
            let mut never = NoSignal;
            hand_over(plan, &mut never, &mut Disk, HANDOVER_WAIT)
        }
    };
    match outcome {
        Outcome::Healthy => {
            log::info!("{} reported healthy; the old version ends here", plan.to);
            0
        }
        Outcome::RolledBack(reason) => {
            log::warn!("update failed: {reason}; {} is running again", plan.from);
            1
        }
        Outcome::Broken(reason) => {
            log::error!("update failed and could not be undone: {reason}");
            2
        }
    }
}

/// Starts only the old version: used when the healthy event cannot exist.
struct NoSignal;

impl Launcher for NoSignal {
    fn start(&mut self, exe: &Path, args: &[String]) -> Result<Box<dyn Started>, String> {
        if args.first().map(String::as_str) == Some(UPDATED_FROM) {
            return Err("no healthy signal could be set up".to_string());
        }
        install::start_detached(exe, args)
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        Ok(Box::new(Gone))
    }
}

struct Gone;

impl Started for Gone {
    fn wait(&mut self, _timeout: Duration) -> Waited {
        Waited::Exited(None)
    }

    fn kill(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::github::Asset;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::rc::Rc;

    fn release(tag: &str) -> Release {
        Release {
            tag: tag.to_string(),
            ..Release::default()
        }
    }

    #[test]
    fn a_reply_is_no_release_up_to_date_or_newer() {
        assert_eq!(classify(None, "0.16.0"), Checked::NoRelease);
        assert_eq!(
            classify(Some(release("v0.16.0")), "0.16.0"),
            Checked::UpToDate("v0.16.0".to_string())
        );
        assert_eq!(
            classify(Some(release("v0.15.0")), "0.16.0"),
            Checked::UpToDate("v0.15.0".to_string())
        );
        assert_eq!(
            classify(Some(release("v0.16.1")), "0.16.0"),
            Checked::Newer(release("v0.16.1"))
        );
    }

    #[test]
    fn only_the_installed_copy_outside_a_package_may_update() {
        let installed = install::dir().join(EXE_NAME);
        assert_eq!(blocked(&installed, None), None);
        let repo = Path::new(r"E:\code\wincraft\target\release\wincraft.exe");
        let text = blocked(repo, None).unwrap();
        assert!(text.starts_with("Updates install into "), "{text}");
        assert!(
            text.ends_with("Run wincraft.exe --install once first."),
            "{text}"
        );
        let text = blocked(&installed, Some("Claude")).unwrap();
        assert!(text.starts_with("WinCraft runs inside Claude"), "{text}");
    }

    #[test]
    fn only_wincraft_release_downloads_over_https_are_fetched() {
        let good = "https://github.com/OliverD25/wincraft/releases/download/v0.16.0/wincraft.exe";
        assert!(allowed_asset(good, false));
        for bad in [
            "http://github.com/OliverD25/wincraft/releases/download/v0.16.0/wincraft.exe",
            "https://github.com/someone/wincraft/releases/download/v0.16.0/wincraft.exe",
            "https://github.com.evil.example/OliverD25/wincraft/releases/download/x",
            "https://objects.githubusercontent.com/wincraft.exe",
            "http://127.0.0.1:5000/download/wincraft.exe",
            "",
        ] {
            assert!(!allowed_asset(bad, false), "{bad}");
        }
        assert!(allowed_asset(
            "http://127.0.0.1:5000/download/wincraft.exe",
            true
        ));
        assert!(!allowed_asset("http://192.168.1.2:5000/wincraft.exe", true));
    }

    struct FakeFetch {
        files: BTreeMap<String, Vec<u8>>,
    }

    impl FakeFetch {
        fn new(exe: &[u8], sums: &str) -> Self {
            let mut files = BTreeMap::new();
            files.insert(url(EXE_ASSET), exe.to_vec());
            files.insert(url(SUMS_ASSET), sums.as_bytes().to_vec());
            Self { files }
        }
    }

    impl Fetch for FakeFetch {
        fn text(&self, url: &str) -> Result<String, String> {
            self.files
                .get(url)
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .ok_or_else(|| "http status: 404".to_string())
        }

        fn to_file(
            &self,
            url: &str,
            dest: &Path,
            progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
        ) -> Result<(), String> {
            let bytes = self.files.get(url).ok_or("http status: 404")?;
            fs::write(dest, bytes).map_err(|e| e.to_string())?;
            if !progress(bytes.len() as u64, Some(bytes.len() as u64)) {
                return Err(CANCELLED.to_string());
            }
            Ok(())
        }
    }

    fn url(name: &str) -> String {
        format!("{DOWNLOADS}v9.9.9/{name}")
    }

    fn published() -> Release {
        Release {
            tag: "v9.9.9".to_string(),
            assets: [EXE_ASSET, SUMS_ASSET]
                .iter()
                .map(|name| Asset {
                    name: name.to_string(),
                    url: url(name),
                })
                .collect(),
            ..Release::default()
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_download_that_matches_its_checksum_is_kept() {
        let dir = scratch("good");
        let sums = format!("{}  wincraft.exe\n", sha256::of_bytes(b"new exe").unwrap());
        let mut seen = Vec::new();
        let path = download(
            &published(),
            &dir,
            &FakeFetch::new(b"new exe", &sums),
            false,
            &mut |done, total| {
                seen.push((done, total));
                true
            },
        )
        .unwrap();
        assert_eq!(path, dir.join(NEW_EXE));
        assert_eq!(fs::read(&path).unwrap(), b"new exe");
        assert_eq!(seen, [(7, Some(7))]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_download_leaves_no_partial_file() {
        let dir = scratch("cancel");
        let sums = format!(
            "{}  wincraft.exe
",
            sha256::of_bytes(b"new exe").unwrap()
        );
        let err = download(
            &published(),
            &dir,
            &FakeFetch::new(b"new exe", &sums),
            false,
            &mut |_, _| false,
        )
        .unwrap_err();
        assert_eq!(err, CANCELLED);
        assert!(!dir.join(NEW_EXE).exists(), "the partial file is deleted");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_download_that_does_not_match_is_deleted() {
        let dir = scratch("bad");
        let sums = format!("{}  wincraft.exe\n", sha256::of_bytes(b"other").unwrap());
        let err = download(
            &published(),
            &dir,
            &FakeFetch::new(b"tampered", &sums),
            false,
            &mut |_, _| true,
        )
        .unwrap_err();
        assert!(err.contains("does not match its checksum"), "{err}");
        assert!(!dir.join(NEW_EXE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_release_without_both_files_or_a_checksum_line_downloads_nothing() {
        let dir = scratch("missing");
        let fetch = FakeFetch::new(b"exe", "0000  other.txt\n");
        let err = download(&published(), &dir, &fetch, false, &mut |_, _| true).unwrap_err();
        assert!(err.contains("has no line for wincraft.exe"), "{err}");
        let mut only_exe = published();
        only_exe.assets.retain(|asset| asset.name == EXE_ASSET);
        let err = download(&only_exe, &dir, &fetch, false, &mut |_, _| true).unwrap_err();
        assert_eq!(err, "release v9.9.9 has no SHA256SUMS.txt");
        let mut elsewhere = published();
        elsewhere.assets[0].url = "https://example.com/wincraft.exe".to_string();
        let err = download(&elsewhere, &dir, &fetch, false, &mut |_, _| true).unwrap_err();
        assert!(
            err.starts_with("refused to download https://example.com/"),
            "{err}"
        );
        assert!(!dir.join(NEW_EXE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Files by lower-case name; renames from a locked name fail.
    #[derive(Default)]
    struct FakeFiles {
        present: BTreeSet<String>,
        locked: BTreeSet<String>,
        moves: Vec<String>,
    }

    fn name(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().to_lowercase()
    }

    impl FakeFiles {
        fn with(names: &[&str]) -> Self {
            Self {
                present: names.iter().map(|name| name.to_string()).collect(),
                ..Self::default()
            }
        }

        fn names(&self) -> Vec<&str> {
            self.present.iter().map(String::as_str).collect()
        }
    }

    impl Files for FakeFiles {
        fn exists(&self, path: &Path) -> bool {
            self.present.contains(&name(path))
        }

        fn rename(&mut self, from: &Path, to: &Path) -> Result<(), String> {
            let (from, to) = (name(from), name(to));
            if self.locked.contains(&from) || !self.present.remove(&from) {
                return Err(format!("cannot move {from}"));
            }
            self.present.insert(to.clone());
            self.moves.push(format!("{from} -> {to}"));
            Ok(())
        }

        fn remove(&mut self, path: &Path) -> Result<(), String> {
            let name = name(path);
            if self.locked.contains(&name) || !self.present.remove(&name) {
                return Err(format!("cannot delete {name}"));
            }
            self.moves.push(format!("delete {name}"));
            Ok(())
        }
    }

    const DIR: &str = r"C:\Programs\WinCraft";

    #[test]
    fn the_swap_renames_the_running_exe_and_puts_the_download_in_its_place() {
        let mut files = FakeFiles::with(&[EXE_NAME, NEW_EXE, OLD_EXE]);
        swap_in(Path::new(DIR), &mut files).unwrap();
        assert_eq!(files.names(), [EXE_NAME, OLD_EXE]);
        assert_eq!(
            files.moves,
            [
                "delete wincraft.old.exe",
                "wincraft.exe -> wincraft.old.exe",
                "wincraft.new.exe -> wincraft.exe"
            ]
        );
    }

    #[test]
    fn a_swap_that_fails_halfway_puts_the_running_exe_back() {
        let mut files = FakeFiles::with(&[EXE_NAME, NEW_EXE]);
        files.locked.insert(NEW_EXE.to_string());
        assert!(swap_in(Path::new(DIR), &mut files).is_err());
        assert_eq!(files.names(), [EXE_NAME, NEW_EXE]);
    }

    struct FakeRun {
        healthy_after: Option<Duration>,
        exits_after: Option<Duration>,
        kills: Rc<RefCell<u32>>,
    }

    impl Started for FakeRun {
        fn wait(&mut self, timeout: Duration) -> Waited {
            let healthy = self.healthy_after.filter(|after| *after <= timeout);
            let exits = self.exits_after.filter(|after| *after <= timeout);
            match (healthy, exits) {
                (Some(healthy), Some(exits)) if exits < healthy => Waited::Exited(Some(101)),
                (Some(_), _) => Waited::Healthy,
                (None, Some(_)) => Waited::Exited(Some(101)),
                (None, None) => Waited::TimedOut,
            }
        }

        fn kill(&mut self) {
            *self.kills.borrow_mut() += 1;
        }
    }

    /// How each started process behaves, in start order: Err fails to
    /// start; (healthy after, exits after) otherwise.
    type Script = Result<(Option<u64>, Option<u64>), String>;

    #[derive(Default)]
    struct FakeLauncher {
        script: VecDeque<Script>,
        started: Vec<String>,
        kills: Rc<RefCell<u32>>,
    }

    impl FakeLauncher {
        fn new(script: Vec<Script>) -> Self {
            Self {
                script: script.into(),
                ..Self::default()
            }
        }
    }

    impl Launcher for FakeLauncher {
        fn start(&mut self, exe: &Path, args: &[String]) -> Result<Box<dyn Started>, String> {
            self.started
                .push(format!("{} {}", name(exe), args.join(" ")));
            let (healthy, exits) = self.script.pop_front().unwrap_or(Ok((None, None)))?;
            Ok(Box::new(FakeRun {
                healthy_after: healthy.map(Duration::from_secs),
                exits_after: exits.map(Duration::from_secs),
                kills: Rc::clone(&self.kills),
            }))
        }
    }

    fn plan() -> Handover {
        Handover {
            dir: PathBuf::from(DIR),
            from: "0.16.0".to_string(),
            to: "v0.17.0".to_string(),
            reopen: None,
        }
    }

    #[test]
    fn the_reopen_word_round_trips_and_unknown_words_mean_nothing() {
        for reopen in [Reopen::Palette, Reopen::About] {
            assert_eq!(Reopen::from_word(reopen.word()), Some(reopen));
        }
        assert_eq!(Reopen::from_word(""), None);
        assert_eq!(Reopen::from_word("tray"), None);
    }

    #[test]
    fn a_start_reopens_only_when_an_update_started_it() {
        assert_eq!(
            reopen_at_start(true, Some("palette")),
            Some(Reopen::Palette)
        );
        assert_eq!(reopen_at_start(true, Some("about")), Some(Reopen::About));
        assert_eq!(reopen_at_start(true, None), None);
        assert_eq!(reopen_at_start(true, Some("nonsense")), None);
        assert_eq!(
            reopen_at_start(false, Some("palette")),
            None,
            "a normal start"
        );
    }

    #[test]
    fn the_new_version_is_told_where_to_reopen_only_when_the_user_asked_for_the_update() {
        for (reopen, tail) in [
            (None, ""),
            (Some(Reopen::Palette), " --reopen palette"),
            (Some(Reopen::About), " --reopen about"),
        ] {
            let mut launcher = FakeLauncher::new(vec![Ok((Some(30), None))]);
            let plan = Handover { reopen, ..plan() };
            hand_over(&plan, &mut launcher, &mut swapped(), HANDOVER_WAIT);
            assert_eq!(
                launcher.started,
                [format!("wincraft.exe --updated-from 0.16.0{tail}")]
            );
        }
    }

    #[test]
    fn a_rollback_starts_the_old_version_with_the_same_place_to_reopen() {
        let mut launcher = FakeLauncher::new(vec![Ok((None, Some(2))), Ok((None, None))]);
        let plan = Handover {
            reopen: Some(Reopen::About),
            ..plan()
        };
        let outcome = hand_over(&plan, &mut launcher, &mut swapped(), HANDOVER_WAIT);
        assert!(matches!(outcome, Outcome::RolledBack(_)), "{outcome:?}");
        assert_eq!(
            launcher.started,
            [
                "wincraft.exe --updated-from 0.16.0 --reopen about",
                "wincraft.exe --update-failed v0.17.0 --reopen about"
            ]
        );
    }

    fn swapped() -> FakeFiles {
        FakeFiles::with(&[EXE_NAME, OLD_EXE])
    }

    #[test]
    fn a_healthy_new_version_keeps_the_swap() {
        let mut launcher = FakeLauncher::new(vec![Ok((Some(30), None))]);
        let mut files = swapped();
        let outcome = hand_over(&plan(), &mut launcher, &mut files, HANDOVER_WAIT);
        assert_eq!(outcome, Outcome::Healthy);
        assert_eq!(launcher.started, ["wincraft.exe --updated-from 0.16.0"]);
        assert!(files.moves.is_empty());
        assert_eq!(*launcher.kills.borrow(), 0);
    }

    #[test]
    fn a_new_version_that_crashes_is_rolled_back_and_the_old_one_restarted() {
        let mut launcher = FakeLauncher::new(vec![Ok((None, Some(2))), Ok((None, None))]);
        let mut files = swapped();
        let outcome = hand_over(&plan(), &mut launcher, &mut files, HANDOVER_WAIT);
        let Outcome::RolledBack(reason) = outcome else {
            panic!("expected a rollback, got {outcome:?}");
        };
        assert_eq!(
            reason,
            "v0.17.0 closed with exit code 101 before it reported healthy"
        );
        assert_eq!(
            files.moves,
            [
                "wincraft.exe -> wincraft.failed.exe",
                "wincraft.old.exe -> wincraft.exe",
                "delete wincraft.failed.exe"
            ]
        );
        assert_eq!(files.names(), [EXE_NAME]);
        assert_eq!(
            launcher.started,
            [
                "wincraft.exe --updated-from 0.16.0",
                "wincraft.exe --update-failed v0.17.0"
            ]
        );
        assert_eq!(
            *launcher.kills.borrow(),
            0,
            "a process that ended is not killed"
        );
    }

    #[test]
    fn a_new_version_that_hangs_is_ended_before_the_rollback() {
        let mut launcher = FakeLauncher::new(vec![Ok((None, None))]);
        let mut files = swapped();
        let outcome = hand_over(&plan(), &mut launcher, &mut files, HANDOVER_WAIT);
        assert_eq!(
            outcome,
            Outcome::RolledBack(
                "v0.17.0 did not report healthy within 60 seconds and was ended".to_string()
            )
        );
        assert_eq!(*launcher.kills.borrow(), 1);
        assert_eq!(files.names(), [EXE_NAME]);
    }

    #[test]
    fn a_healthy_signal_after_the_wait_is_too_late() {
        let mut launcher = FakeLauncher::new(vec![Ok((Some(75), None))]);
        let mut files = swapped();
        let outcome = hand_over(&plan(), &mut launcher, &mut files, HANDOVER_WAIT);
        assert!(matches!(outcome, Outcome::RolledBack(_)), "{outcome:?}");
        assert_eq!(*launcher.kills.borrow(), 1);
        assert_eq!(launcher.started.len(), 2);
    }

    #[test]
    fn a_healthy_version_that_quits_at_once_still_counts_as_healthy() {
        let mut launcher = FakeLauncher::new(vec![Ok((Some(30), Some(31)))]);
        let outcome = hand_over(&plan(), &mut launcher, &mut swapped(), HANDOVER_WAIT);
        assert_eq!(outcome, Outcome::Healthy);
    }

    #[test]
    fn a_new_version_that_cannot_start_is_rolled_back() {
        let mut launcher = FakeLauncher::new(vec![Err("Access is denied".to_string())]);
        let mut files = swapped();
        let outcome = hand_over(&plan(), &mut launcher, &mut files, HANDOVER_WAIT);
        assert_eq!(
            outcome,
            Outcome::RolledBack("v0.17.0 did not start: Access is denied".to_string())
        );
        assert_eq!(files.names(), [EXE_NAME]);
        assert_eq!(launcher.started[1], "wincraft.exe --update-failed v0.17.0");
    }

    #[test]
    fn when_the_files_cannot_go_back_the_old_version_starts_from_its_odd_name() {
        let mut launcher = FakeLauncher::new(vec![Ok((None, Some(1))), Ok((None, None))]);
        let mut files = swapped();
        files.locked.insert(EXE_NAME.to_string());
        let outcome = hand_over(&plan(), &mut launcher, &mut files, HANDOVER_WAIT);
        let Outcome::Broken(reason) = outcome else {
            panic!("expected Broken, got {outcome:?}");
        };
        assert!(
            reason.ends_with("started it from wincraft.old.exe"),
            "{reason}"
        );
        assert_eq!(
            launcher.started[1],
            "wincraft.old.exe --update-failed v0.17.0"
        );
    }

    #[test]
    fn an_old_version_that_will_not_start_again_is_reported() {
        let mut launcher =
            FakeLauncher::new(vec![Ok((None, Some(1))), Err("no such file".to_string())]);
        let outcome = hand_over(&plan(), &mut launcher, &mut swapped(), HANDOVER_WAIT);
        let Outcome::Broken(reason) = outcome else {
            panic!("expected Broken, got {outcome:?}");
        };
        assert!(
            reason.ends_with("0.16.0 is back in place but did not start: no such file"),
            "{reason}"
        );
    }

    #[test]
    fn leftovers_are_deleted_and_a_locked_one_is_reported() {
        let mut files = FakeFiles::with(&[EXE_NAME, OLD_EXE, NEW_EXE, FAILED_EXE]);
        files.locked.insert(OLD_EXE.to_string());
        let left = remove_leftovers(Path::new(DIR), &mut files);
        assert_eq!(left, [OLD_EXE]);
        assert_eq!(files.names(), [EXE_NAME, OLD_EXE]);
        files.locked.clear();
        assert!(remove_leftovers(Path::new(DIR), &mut files).is_empty());
        assert_eq!(files.names(), [EXE_NAME]);
    }

    #[test]
    fn a_handover_is_taken_once() {
        hand_over_at_exit(plan());
        assert_eq!(take_handover(), Some(plan()));
        assert_eq!(take_handover(), None);
    }

    fn test_event() -> String {
        format!(r"Local\WinCraft.Healthy.test-{}", std::process::id())
    }

    #[test]
    fn the_healthy_signal_reaches_the_waiting_event_by_name() {
        let name = test_event();
        assert!(!signal(&name), "nobody is waiting yet");
        let waiting = HealthSignal::named(&name).unwrap();
        assert!(signal(&name));
        let waited =
            unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(waiting.0, 0) };
        assert_eq!(waited, WAIT_OBJECT_0);
    }

    fn cmd(args: &str) -> Vec<String> {
        ["/d", "/c"]
            .iter()
            .map(|part| part.to_string())
            .chain(std::iter::once(args.to_string()))
            .collect()
    }

    fn system32(exe: &str) -> PathBuf {
        let system = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        Path::new(&system).join("System32").join(exe)
    }

    fn cmd_exe() -> PathBuf {
        system32("cmd.exe")
    }

    #[test]
    fn a_real_process_that_ends_is_seen_with_its_exit_code() {
        let mut signal = HealthSignal::named(&format!("{}.exit", test_event())).unwrap();
        let mut started = signal.start(&cmd_exe(), &cmd("exit 3")).unwrap();
        assert_eq!(
            started.wait(Duration::from_secs(20)),
            Waited::Exited(Some(3))
        );
    }

    #[test]
    fn a_real_process_that_hangs_times_out_and_is_ended() {
        let mut signal = HealthSignal::named(&format!("{}.hang", test_event())).unwrap();
        // ping itself, not through cmd: kill ends only the process it
        // started, and a ping left behind by a killed cmd ran on for 30 s.
        let args = ["-n", "30", "127.0.0.1"].map(str::to_string);
        let mut started = signal.start(&system32("PING.EXE"), &args).unwrap();
        assert_eq!(started.wait(Duration::from_millis(300)), Waited::TimedOut);
        started.kill();
        assert!(matches!(
            started.wait(Duration::from_secs(5)),
            Waited::Exited(_)
        ));
    }

    /// The real ureq download against a server on this PC: a port the
    /// system picks, never one in use, and nothing on the internet.
    #[test]
    fn the_web_fetch_writes_the_body_to_a_file_with_progress() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body: Vec<u8> = (0..150_000u32).map(|n| (n % 253) as u8).collect();
        let sent = body.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                sent.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(&sent).unwrap();
        });
        let dir = scratch("web");
        let dest = dir.join(NEW_EXE);
        let mut last = (0, None);
        Web.to_file(
            &format!("http://127.0.0.1:{port}/download/wincraft.exe"),
            &dest,
            &mut |done, total| {
                last = (done, total);
                true
            },
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        assert_eq!(last, (body.len() as u64, Some(body.len() as u64)));
        let _ = fs::remove_dir_all(&dir);
    }

    fn update_folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn text_of(dir: &Path, name: &str) -> Option<String> {
        fs::read_to_string(dir.join(name)).ok()
    }

    #[test]
    fn on_a_real_disk_the_new_exe_takes_the_place_of_the_old_one() {
        let dir = update_folder(
            "swap",
            &[(EXE_NAME, "v1"), (NEW_EXE, "v2"), (OLD_EXE, "v0")],
        );
        swap_in(&dir, &mut Disk).unwrap();
        assert_eq!(text_of(&dir, EXE_NAME).as_deref(), Some("v2"));
        assert_eq!(text_of(&dir, OLD_EXE).as_deref(), Some("v1"));
        assert_eq!(text_of(&dir, NEW_EXE), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn on_a_real_disk_a_swap_goes_back_and_the_failed_exe_is_deleted() {
        let dir = update_folder(
            "back",
            &[(EXE_NAME, "v2"), (OLD_EXE, "v1"), (FAILED_EXE, "x")],
        );
        swap_back(&dir, &mut Disk).unwrap();
        assert_eq!(text_of(&dir, EXE_NAME).as_deref(), Some("v1"));
        assert_eq!(text_of(&dir, OLD_EXE), None);
        assert_eq!(text_of(&dir, FAILED_EXE), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn going_back_without_the_old_exe_says_there_is_nothing_to_go_back_to() {
        let dir = update_folder("noback", &[(EXE_NAME, "v2")]);
        let err = swap_back(&dir, &mut Disk).unwrap_err();
        assert!(err.contains("nothing to go back to"), "{err}");
        assert_eq!(text_of(&dir, EXE_NAME).as_deref(), Some("v2"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn on_a_real_disk_the_leftovers_go_and_the_installed_exe_stays() {
        let dir = update_folder(
            "left",
            &[
                (EXE_NAME, "v2"),
                (OLD_EXE, "v1"),
                (NEW_EXE, "n"),
                (FAILED_EXE, "f"),
            ],
        );
        assert!(remove_leftovers(&dir, &mut Disk).is_empty());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(text_of(&dir, EXE_NAME).as_deref(), Some("v2"));
        assert!(Disk.exists(&dir.join(EXE_NAME)));
        assert!(!Disk.exists(&dir.join(OLD_EXE)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_step_that_works_on_the_third_try_is_waited_for() {
        let mut calls = 0;
        let result = Disk::patiently(|| {
            calls += 1;
            if calls < 3 {
                Err("busy".to_string())
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Ok(()));
        assert_eq!(calls, 3);
    }

    #[test]
    fn with_no_healthy_signal_only_the_old_version_is_started() {
        let mut launcher = NoSignal;
        let refused = launcher.start(Path::new("wincraft.exe"), &[UPDATED_FROM.to_string()]);
        assert!(matches!(refused, Err(err) if err.contains("no healthy signal")));
        let mut gone = Gone;
        assert_eq!(gone.wait(Duration::from_secs(1)), Waited::Exited(None));
        gone.kill();
    }

    #[test]
    fn a_path_under_local_app_data_is_shortened() {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        if local.is_empty() {
            return;
        }
        let inside = Path::new(&local).join("Programs").join("WinCraft");
        let short = short_path(&inside);
        assert!(short.starts_with("%LOCALAPPDATA%"), "{short}");
        assert!(short.ends_with("WinCraft"), "{short}");
        assert_eq!(short_path(Path::new(r"D:\elsewhere")), r"D:\elsewhere");
    }

    #[test]
    fn an_update_is_blocked_inside_a_package_and_outside_the_install_folder() {
        let inside = blocked(Path::new("wincraft.exe"), Some("Claude")).unwrap();
        assert!(inside.contains("runs inside Claude"), "{inside}");
        let elsewhere = blocked(Path::new(r"D:\build\wincraft.exe"), None).unwrap();
        assert!(elsewhere.contains("wincraft.exe --install"), "{elsewhere}");
        let installed = install::dir().join(EXE_NAME);
        assert_eq!(blocked(&installed, None), None);
    }

    #[test]
    fn only_wincrafts_own_downloads_are_allowed_unless_a_local_fake_is_on() {
        assert!(allowed_asset(&format!("{DOWNLOADS}v1/wincraft.exe"), false));
        assert!(!allowed_asset("https://example.com/wincraft.exe", false));
        assert!(!allowed_asset("http://127.0.0.1:9/wincraft.exe", false));
        assert!(allowed_asset("http://127.0.0.1:9/wincraft.exe", true));
        assert!(!allowed_asset("http://evil.test/wincraft.exe", true));
    }

    #[test]
    fn now_is_after_2023() {
        assert!(now_secs() > 1_700_000_000);
    }
}
