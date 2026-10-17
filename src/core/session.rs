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

/// Whether a start `age` after sign-in restores the saved layout. A start
/// that something else caused (an update, Windows restarting WinCraft after
/// an installer closed it) is never the start after sign-in, however young
/// the session is.
pub fn restores_at_start(age: Duration, started_again: bool) -> bool {
    !started_again && age <= SIGN_IN_WINDOW
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

    #[test]
    fn a_start_just_after_sign_in_restores() {
        assert!(restores_at_start(Duration::from_secs(8), false));
        assert!(restores_at_start(minutes(2), false));
    }

    #[test]
    fn the_window_is_fifteen_minutes() {
        assert!(restores_at_start(minutes(14), false));
        assert!(restores_at_start(minutes(15), false));
        assert!(!restores_at_start(minutes(16), false));
        assert!(!restores_at_start(Duration::from_secs(15 * 60 + 1), false));
    }

    #[test]
    fn a_start_caused_by_an_update_or_an_installer_never_restores() {
        // The update restarts WinCraft seconds into a session, or hours.
        assert!(!restores_at_start(minutes(1), true));
        assert!(!restores_at_start(minutes(300), true));
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
