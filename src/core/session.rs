//! How long ago the user signed in to Windows, so LayoutKeeper restores the
//! saved layout only for a start that follows a sign-in.
//!
//! The time is the logon time of this process's own logon session, read with
//! `LsaGetLogonSessionData`. It is the moment Windows made the user's
//! session, so it is right in the cases the boot time gets wrong: with fast
//! startup a shutdown only hibernates the kernel and the tick counter keeps
//! running across it, but the user session is still made again at every
//! sign-in. Locking the PC or resuming it from sleep does not make a new
//! session either, so they do not count as signing in.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
use windows_sys::Win32::Security::Authentication::Identity::{
    LsaFreeReturnBuffer, LsaGetLogonSessionData, SECURITY_LOGON_SESSION_DATA,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenStatistics, TOKEN_QUERY, TOKEN_STATISTICS,
};
use windows_sys::Win32::System::SystemInformation::{GetSystemTimeAsFileTime, GetTickCount64};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// A start this long after sign-in or less is the start that follows it:
/// the task that starts WinCraft at sign-in runs within seconds, and a slow
/// PC may take minutes to get there.
pub const SIGN_IN_WINDOW: Duration = Duration::from_secs(15 * 60);

/// Where the age came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The logon time of this process's session.
    Session,
    /// The session could not be read, so the time since the PC started.
    /// With fast startup that can be much more than the sign-in age, never
    /// less, so it only ever errs towards not restoring.
    Boot,
}

/// Time since the user signed in.
pub fn sign_in_age() -> (Duration, Source) {
    match logon_time().and_then(age_from_filetime_ticks) {
        Some(age) => (age, Source::Session),
        None => (
            Duration::from_millis(unsafe { GetTickCount64() }),
            Source::Boot,
        ),
    }
}

/// How long ago `ticks` (100 ns since 1601, as FILETIME counts) was. A time
/// in the future, from a clock that was set back, is no time ago.
fn age_from_filetime_ticks(ticks: i64) -> Option<Duration> {
    if ticks <= 0 {
        return None;
    }
    let mut now = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    unsafe { GetSystemTimeAsFileTime(&mut now) };
    let now = (u64::from(now.dwHighDateTime) << 32) | u64::from(now.dwLowDateTime);
    Some(ticks_between(ticks as u64, now))
}

fn ticks_between(then: u64, now: u64) -> Duration {
    Duration::from_nanos(now.saturating_sub(then).saturating_mul(100))
}

/// The logon time of this process's logon session, in FILETIME ticks.
fn logon_time() -> Option<i64> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let mut statistics: TOKEN_STATISTICS = unsafe { std::mem::zeroed() };
    let mut needed = 0u32;
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenStatistics,
            (&raw mut statistics).cast(),
            std::mem::size_of::<TOKEN_STATISTICS>() as u32,
            &mut needed,
        )
    };
    unsafe { CloseHandle(token) };
    if read == 0 {
        return None;
    }
    let mut data: *mut SECURITY_LOGON_SESSION_DATA = std::ptr::null_mut();
    let status = unsafe { LsaGetLogonSessionData(&statistics.AuthenticationId, &mut data) };
    if status != 0 || data.is_null() {
        return None;
    }
    let time = unsafe { (*data).LogonTime };
    unsafe { LsaFreeReturnBuffer(data.cast()) };
    Some(time)
}

/// How this WinCraft was started, read once in main.rs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartKind {
    /// Started with `--from-task`: by the logon task, or the Run key that
    /// stands in for it.
    pub from_task: bool,
    /// Started by an update, a rollback, or Windows after an installer
    /// closed it.
    pub started_again: bool,
    /// The first WinCraft start since the user signed in; None when the
    /// sign-in time cannot be read.
    pub first_in_session: Option<bool>,
}

static START: OnceLock<StartKind> = OnceLock::new();

/// Reads and then updates the marker in the data folder that holds the
/// sign-in this WinCraft last started in, and keeps what it found for
/// `start_kind`. Call once, after the single-instance mutex is held, so a
/// second copy that quits at once does not count as a start.
pub fn note_start(from_task: bool, started_again: bool) -> StartKind {
    let first_in_session = logon_time().map(|logon| {
        let path = marker_path();
        let first = is_first_start(std::fs::read_to_string(&path).ok().as_deref(), logon);
        if let Err(err) = write_marker(&path, logon) {
            log::warn!("cannot write {}: {err}", path.display());
        }
        first
    });
    let kind = StartKind {
        from_task,
        started_again,
        first_in_session,
    };
    let _ = START.set(kind);
    kind
}

/// What `note_start` found; all false and unknown before it ran (in tests).
pub fn start_kind() -> StartKind {
    START.get().copied().unwrap_or_default()
}

fn marker_path() -> PathBuf {
    crate::core::config::data_dir().join(format!(
        "{}.txt",
        crate::core::instance::name("last-sign-in")
    ))
}

fn write_marker(path: &Path, logon: i64) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::core::config::write_atomic(path, &logon.to_string())
}

/// A start is the first of the session when the marker is missing, cannot
/// be read, or holds another sign-in time.
fn is_first_start(marker: Option<&str>, logon: i64) -> bool {
    marker.and_then(|text| text.trim().parse::<i64>().ok()) != Some(logon)
}

/// What LayoutKeeper does when WinCraft starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtStart {
    /// Put the saved layout back by itself.
    Restore,
    /// Move nothing; ask with a Put back button.
    Offer,
    Nothing,
}

/// LayoutKeeper restores by itself only for the start the sign-in task
/// makes right after sign-in (Principle 1). The first WinCraft of a session
/// that came later or by hand asks first. Any other start (an update,
/// Windows starting WinCraft again after an installer closed it, a second
/// start in the session) does nothing, however young the session is.
pub fn at_start(start: StartKind, age: Duration) -> AtStart {
    if start.started_again {
        AtStart::Nothing
    } else if start.from_task && age <= SIGN_IN_WINDOW {
        AtStart::Restore
    } else if start.first_in_session == Some(true) {
        AtStart::Offer
    } else {
        AtStart::Nothing
    }
}

/// "3 min" or "2 h 5 min", for the log.
pub fn describe(age: Duration) -> String {
    let minutes = age.as_secs() / 60;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(count: u64) -> Duration {
        Duration::from_secs(count * 60)
    }

    const BY_TASK: StartKind = StartKind {
        from_task: true,
        started_again: false,
        first_in_session: Some(true),
    };
    const BY_HAND: StartKind = StartKind {
        from_task: false,
        started_again: false,
        first_in_session: Some(true),
    };

    #[test]
    fn the_task_start_just_after_sign_in_restores() {
        assert_eq!(at_start(BY_TASK, Duration::from_secs(8)), AtStart::Restore);
        assert_eq!(at_start(BY_TASK, minutes(2)), AtStart::Restore);
        let unknown = StartKind {
            first_in_session: None,
            ..BY_TASK
        };
        assert_eq!(at_start(unknown, minutes(2)), AtStart::Restore);
    }

    #[test]
    fn the_window_is_fifteen_minutes_and_a_late_first_start_asks() {
        assert_eq!(at_start(BY_TASK, minutes(14)), AtStart::Restore);
        assert_eq!(at_start(BY_TASK, minutes(15)), AtStart::Restore);
        assert_eq!(at_start(BY_TASK, minutes(16)), AtStart::Offer);
        assert_eq!(
            at_start(BY_TASK, Duration::from_secs(15 * 60 + 1)),
            AtStart::Offer
        );
    }

    /// 2026-11-15: the task did not run, the user started WinCraft by hand
    /// 12 minutes after sign-in, and it moved windows by itself.
    #[test]
    fn a_first_start_by_hand_asks_at_any_age() {
        assert_eq!(at_start(BY_HAND, Duration::from_secs(8)), AtStart::Offer);
        assert_eq!(at_start(BY_HAND, minutes(12)), AtStart::Offer);
        assert_eq!(at_start(BY_HAND, minutes(300)), AtStart::Offer);
    }

    #[test]
    fn a_second_start_or_an_unknown_session_by_hand_does_nothing() {
        let second = StartKind {
            first_in_session: Some(false),
            ..BY_HAND
        };
        assert_eq!(at_start(second, minutes(1)), AtStart::Nothing);
        let unknown = StartKind {
            first_in_session: None,
            ..BY_HAND
        };
        assert_eq!(at_start(unknown, minutes(1)), AtStart::Nothing);
        let second_by_task = StartKind {
            first_in_session: Some(false),
            ..BY_TASK
        };
        assert_eq!(at_start(second_by_task, minutes(30)), AtStart::Nothing);
    }

    #[test]
    fn a_start_caused_by_an_update_or_an_installer_never_restores() {
        // The update restarts WinCraft seconds into a session, or hours.
        for start in [BY_TASK, BY_HAND] {
            let again = StartKind {
                started_again: true,
                ..start
            };
            assert_eq!(at_start(again, minutes(1)), AtStart::Nothing);
            assert_eq!(at_start(again, minutes(300)), AtStart::Nothing);
        }
    }

    #[test]
    fn only_a_marker_with_this_sign_in_time_makes_a_later_start() {
        let logon = 134_073_450_000_000_000;
        assert!(is_first_start(None, logon), "no marker yet");
        assert!(is_first_start(Some(""), logon));
        assert!(is_first_start(Some("not a time"), logon));
        assert!(
            is_first_start(Some("134073449000000000"), logon),
            "an earlier sign-in"
        );
        assert!(!is_first_start(Some("134073450000000000"), logon));
        assert!(!is_first_start(Some("134073450000000000\r\n"), logon));
    }

    #[test]
    fn the_marker_is_written_and_read_back() {
        let dir = std::env::temp_dir().join(format!("wincraft-marker-{}", std::process::id()));
        let path = dir.join("last-sign-in.txt");
        write_marker(&path, 42).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!is_first_start(Some(&text), 42));
        assert!(is_first_start(Some(&text), 43));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ages_read_for_the_log() {
        assert_eq!(describe(Duration::from_secs(30)), "0 min");
        assert_eq!(describe(minutes(16)), "16 min");
        assert_eq!(describe(minutes(125)), "2 h 5 min");
    }

    #[test]
    fn filetime_ticks_become_a_duration_and_a_future_time_is_no_time() {
        // 100 ns ticks: 600 000 000 of them are 60 s.
        assert_eq!(ticks_between(1_000, 600_001_000), Duration::from_secs(60));
        assert_eq!(ticks_between(5_000, 1_000), Duration::ZERO);
        assert_eq!(age_from_filetime_ticks(0), None);
        assert_eq!(age_from_filetime_ticks(-1), None);
    }

    #[test]
    fn this_session_has_a_sign_in_time_in_the_past() {
        let (age, source) = sign_in_age();
        assert_eq!(
            source,
            Source::Session,
            "the session's logon time is readable"
        );
        assert!(age > Duration::from_secs(1), "a test runs after signing in");
        // Nobody stays signed in for years; a wild value means a wrong unit.
        assert!(age < Duration::from_secs(5 * 365 * 24 * 3600), "{age:?}");
    }
}
