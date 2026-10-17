//! Updates from GitHub releases. WinCraft asks once a day whether a newer
//! release exists and says so; nothing is downloaded until the user clicks
//! Update now.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::core::instance;
use crate::store::github::{self, Release};

pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether the daily check should ask GitHub now. A clock that went back
/// since the last check counts as due, or the check would stop for as long.
pub fn check_due(now: u64, last: u64, daily: bool) -> bool {
    daily && (last == 0 || now < last || now - last >= CHECK_EVERY.as_secs())
}

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

/// Asks GitHub; slow, so never on the host or UI thread.
pub fn check(current: &str) -> Result<Checked, String> {
    github::latest_release().map(|release| classify(release, current))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60;

    #[test]
    fn the_check_is_due_once_a_day_and_never_when_switched_off() {
        let now = 1_800_000_000;
        assert!(check_due(now, 0, true), "never checked");
        assert!(!check_due(now, now - 60, true));
        assert!(!check_due(now, now - DAY + 1, true));
        assert!(check_due(now, now - DAY, true));
        assert!(check_due(now, now - 3 * DAY, true));
        assert!(check_due(now, now + 3600, true), "the clock went back");
        assert!(!check_due(now, 0, false));
        assert!(!check_due(now, now - 3 * DAY, false));
    }

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
}
