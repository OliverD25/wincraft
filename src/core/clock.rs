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

/// A moment as "2026-09-24T21:40:11Z", for files other programs may read.
pub fn utc_iso(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rest = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Howard Hinnant's days-to-date algorithm for the proleptic Gregorian
/// calendar; the same one build.rs uses for the build date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
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
    fn utc_moments_format_as_iso_8601() {
        let at = |secs| UNIX_EPOCH + std::time::Duration::from_secs(secs);
        assert_eq!(utc_iso(at(1_790_000_000)), "2026-09-21T14:13:20Z");
        assert_eq!(utc_iso(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(utc_iso(at(0)), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn the_clock_reads_as_hours_and_minutes() {
        let text = now_hours_minutes();
        assert_eq!(text.len(), 5);
        assert_eq!(&text[2..3], ":");
    }
}
