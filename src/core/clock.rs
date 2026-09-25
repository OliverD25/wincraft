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

/// A moment as local time with its UTC offset, "2026-09-25T02:48:11+03:00",
/// for files a person may open: it reads as the clock on the wall and is
/// still one exact moment.
pub fn local_iso(time: SystemTime) -> String {
    let secs = unix_seconds(time);
    let offset = local_system_time(time)
        .map(|local| {
            let local_secs = days_from_civil(
                i64::from(local.wYear),
                u32::from(local.wMonth),
                u32::from(local.wDay),
            ) * 86_400
                + i64::from(local.wHour) * 3600
                + i64::from(local.wMinute) * 60
                + i64::from(local.wSecond);
            // Time zones are whole minutes; rounding absorbs the second
            // that can tick over between the two readings.
            (local_secs - secs + 30).div_euclid(60) * 60
        })
        .unwrap_or(0);
    iso_with_offset(secs, offset)
}

fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

fn iso_with_offset(secs: i64, offset: i64) -> String {
    let local = secs + offset;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let rest = local.rem_euclid(86_400);
    let sign = if offset < 0 { '-' } else { '+' };
    let zone = offset.abs() / 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        zone / 60,
        zone % 60
    )
}

/// Reads a time written by `local_iso`, or the UTC form with `Z` that older
/// files have, as seconds since 1970.
pub fn parse_iso(text: &str) -> Option<i64> {
    let text = text.trim();
    let stamp = text.get(..19)?;
    let zone = text.get(19..)?;
    let bytes = stamp.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' || bytes[13] != b':' {
        return None;
    }
    if bytes[16] != b':' {
        return None;
    }
    let number = |text: &str| -> Option<i64> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    };
    let year = number(&stamp[0..4])?;
    let month = number(&stamp[5..7])?;
    let day = number(&stamp[8..10])?;
    let hour = number(&stamp[11..13])?;
    let minute = number(&stamp[14..16])?;
    let second = number(&stamp[17..19])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let offset = match zone {
        "Z" => 0,
        _ if zone.len() == 6 && zone.as_bytes()[3] == b':' => {
            let sign = match zone.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            sign * (number(zone.get(1..3)?)? * 3600 + number(zone.get(4..6)?)? * 60)
        }
        _ => return None,
    };
    Some(
        days_from_civil(year, month as u32, day as u32) * 86_400
            + hour * 3600
            + minute * 60
            + second
            - offset,
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

/// The inverse of `civil_from_days`, from the same source.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month = i64::from(month);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
    fn moments_format_as_iso_8601_with_their_offset() {
        assert_eq!(
            iso_with_offset(1_790_000_000, 3 * 3600),
            "2026-09-21T17:13:20+03:00"
        );
        assert_eq!(
            iso_with_offset(1_790_000_000, -(5 * 3600 + 30 * 60)),
            "2026-09-21T08:43:20-05:30"
        );
        assert_eq!(iso_with_offset(951_782_400, 0), "2000-02-29T00:00:00+00:00");
        // Crossing midnight moves the date too.
        assert_eq!(iso_with_offset(0, 2 * 3600), "1970-01-01T02:00:00+02:00");
        assert_eq!(iso_with_offset(0, -3600), "1969-12-31T23:00:00-01:00");
    }

    #[test]
    fn both_the_new_and_the_old_form_read_back_as_the_same_moment() {
        let moment = 1_790_000_000;
        assert_eq!(parse_iso("2026-09-21T14:13:20Z"), Some(moment));
        assert_eq!(parse_iso("2026-09-21T17:13:20+03:00"), Some(moment));
        assert_eq!(parse_iso("2026-09-21T08:43:20-05:30"), Some(moment));
        assert_eq!(parse_iso("2000-02-29T00:00:00+00:00"), Some(951_782_400));
        for offset in [0, 3 * 3600, -8 * 3600, 5 * 3600 + 45 * 60] {
            assert_eq!(parse_iso(&iso_with_offset(moment, offset)), Some(moment));
        }
        for broken in [
            "",
            "t1",
            "2026-09-21 14:13:20Z",
            "2026-13-21T14:13:20Z",
            "2026-09-21T14:13:20",
            "2026-09-21T14:13:20+0300",
            "2026-09-21T14:13:20*03:00",
            "2026-09-2xT14:13:20Z",
        ] {
            assert_eq!(parse_iso(broken), None, "{broken:?}");
        }
    }

    #[test]
    fn the_local_time_now_reads_back_as_now() {
        let now = SystemTime::now();
        let text = local_iso(now);
        assert_eq!(text.len(), 25, "{text}");
        assert_eq!(parse_iso(&text), Some(unix_seconds(now)));
    }

    #[test]
    fn the_clock_reads_as_hours_and_minutes() {
        let text = now_hours_minutes();
        assert_eq!(text.len(), 5);
        assert_eq!(&text[2..3], ":");
    }
}
