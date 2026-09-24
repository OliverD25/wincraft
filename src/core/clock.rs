use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Time::FileTimeToSystemTime;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Local wall-clock time as "14:02", for "updated 14:02" in the store.
pub fn now_hours_minutes() -> String {
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    format!("{:02}:{:02}", now.wHour, now.wMinute)
}

/// A moment as the local date "21 Sep", for "the cached copy from 21 Sep".
pub fn day_month(time: SystemTime) -> String {
    let Some(local) = local_system_time(time) else {
        return String::from("an earlier visit");
    };
    let month = MONTHS
        .get(usize::from(local.wMonth).saturating_sub(1))
        .copied()
        .unwrap_or("");
    format!("{} {month}", local.wDay)
}

/// FILETIME counts 100 ns steps since 1601; the Unix epoch is 11,644,473,600
/// seconds after that.
fn local_system_time(time: SystemTime) -> Option<SYSTEMTIME> {
    let since_unix = time.duration_since(UNIX_EPOCH).ok()?;
    let ticks = (since_unix.as_secs() + 11_644_473_600) * 10_000_000
        + u64::from(since_unix.subsec_nanos() / 100);
    let utc = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local: FILETIME = unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } == 0 {
        return None;
    }
    let mut out: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToSystemTime(&local, &mut out) } == 0 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_moment_formats_as_day_and_month() {
        // 2026-09-21 14:13 UTC is in September in every time zone.
        let moment = UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
        let text = day_month(moment);
        assert!(text.ends_with("Sep"), "{text}");
    }

    #[test]
    fn the_clock_reads_as_hours_and_minutes() {
        let text = now_hours_minutes();
        assert_eq!(text.len(), 5);
        assert_eq!(&text[2..3], ":");
    }
}
