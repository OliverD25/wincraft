//! Asking GitHub for the newest release when its API cannot answer.
//!
//! The API at api.github.com allows 60 requests an hour without a login,
//! counted per internet address: every program and PC behind the same
//! router shares them. Past that it answers 403 or 429. The web page
//! `github.com/OliverD25/wincraft/releases/latest` is not counted: it
//! redirects to `/releases/tag/vX.Y.Z`, so the tag can be read from the
//! redirect without the API. The files then come from
//! `releases/download/<tag>/` as always, and the checksum is still checked.

use std::time::Duration;

use crate::core::instance;
use crate::core::release_cache;
use crate::store::github::{self, Asset, Release};

const RELEASES_PAGE_LATEST: &str = "https://github.com/OliverD25/wincraft/releases/latest";
const TIMEOUT: Duration = Duration::from_secs(5);
/// Points a test instance's fallback at a local fake of the releases page.
/// Ignored without `WINCRAFT_INSTANCE`.
pub const RELEASES_PAGE_ENV: &str = "WINCRAFT_RELEASES_PAGE_URL";
/// Waiting for GitHub's limit never counts longer than this, whatever a
/// header says: the limit lifts within the hour.
const LONGEST_WAIT: u64 = 60 * 60;

/// Why a check got no answer, kept apart so the user hears the true reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckError {
    /// GitHub's limit for requests without a login. `wait` is how many
    /// seconds until it lifts, when GitHub said.
    Limited { wait: Option<u64> },
    /// GitHub could not be reached at all.
    Offline(String),
    /// Anything else, in short.
    Other(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckError::Limited { wait: Some(wait) } => {
                write!(f, "GitHub's request limit, lifts in {wait} s")
            }
            CheckError::Limited { wait: None } => write!(f, "GitHub's request limit"),
            CheckError::Offline(err) | CheckError::Other(err) => write!(f, "{err}"),
        }
    }
}

impl From<ureq::Error> for CheckError {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed => CheckError::Offline(format!("{err}")),
            other => CheckError::Other(format!("{other}")),
        }
    }
}

/// Whole minutes until the limit lifts, at least one; 60 when GitHub did
/// not say.
pub fn minutes(wait: Option<u64>) -> u64 {
    wait.map_or(60, |seconds| seconds.div_ceil(60).max(1))
}

/// Seconds until GitHub's limit lifts, from `retry-after` (seconds) or else
/// `x-ratelimit-reset` (unix seconds). None when neither makes sense.
pub fn limit_wait(retry_after: Option<&str>, reset: Option<&str>, now: u64) -> Option<u64> {
    let number = |text: Option<&str>| text.and_then(|text| text.trim().parse::<u64>().ok());
    number(retry_after)
        .or_else(|| number(reset).map(|reset| reset.saturating_sub(now)))
        .map(|wait| wait.min(LONGEST_WAIT))
}

/// The releases page that redirects to the newest tag: GitHub's, or a test
/// instance's fake. None in a test instance without a fake, so a test never
/// reaches the real GitHub.
pub fn page_url() -> Option<String> {
    if !instance::is_test() {
        return Some(RELEASES_PAGE_LATEST.to_string());
    }
    let url = std::env::var(RELEASES_PAGE_ENV).ok()?;
    let url = url.trim();
    (!url.is_empty()).then(|| url.to_string())
}

/// The tag a `/releases/latest` redirect points at: `/releases/tag/v0.20.68`
/// gives `v0.20.68`. A redirect to `/releases` means no release is
/// published yet. Anything else is an error.
pub fn tag_from_location(location: &str) -> Result<Option<String>, String> {
    let path = location
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    const TAG: &str = "/releases/tag/";
    if let Some(at) = path.find(TAG) {
        let tag = &path[at + TAG.len()..];
        let sensible = tag.len() <= 64
            && tag.starts_with(['v', 'V'])
            && tag[1..].starts_with(|c: char| c.is_ascii_digit())
            && tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
        return if sensible {
            Ok(Some(tag.to_string()))
        } else {
            Err(format!("the redirect named no sensible tag: {location}"))
        };
    }
    if path.ends_with("/releases") {
        return Ok(None);
    }
    Err(format!("unexpected redirect to {location}"))
}

/// Asks `page` where the newest release is, without following the
/// redirect: a HEAD request, so no page is downloaded.
pub fn latest_tag_by_redirect(page: &str) -> Result<Option<String>, CheckError> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("WinCraft/", env!("CARGO_PKG_VERSION")))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let response = agent.head(page).call()?;
    let status = response.status().as_u16();
    if !(300..400).contains(&status) {
        return Err(CheckError::Other(format!(
            "the releases page answered {status} instead of a redirect"
        )));
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| CheckError::Other("the redirect had no location".to_string()))?;
    tag_from_location(location).map_err(CheckError::Other)
}

/// A release known only by its tag. Its two files sit at fixed addresses
/// next to `page` (`releases/download/<tag>/`). The notes come from the
/// last API answer when it was about the same tag; else they are empty and
/// About says so.
pub fn release_from_tag(tag: &str, page: &str, kept_notes: Option<&str>) -> Release {
    let base = page.trim_end_matches('/').trim_end_matches("latest");
    let asset = |name: &str| Asset {
        name: name.to_string(),
        url: format!("{base}download/{tag}/{name}"),
    };
    Release {
        tag: tag.to_string(),
        notes: kept_notes.unwrap_or_default().to_string(),
        page: github::release_url(tag),
        assets: vec![
            asset(crate::core::update::EXE_ASSET),
            asset(crate::core::update::SUMS_ASSET),
        ],
    }
}

/// The notes the last full API answer gave for `tag`, when it was that tag.
pub fn kept_notes(tag: &str) -> Option<String> {
    let cached = release_cache::load_at(&release_cache::path())?;
    let release = github::parse_release(&cached.body).ok()?;
    (release.tag == tag).then_some(release.notes)
}

/// The newest release by the releases page's redirect, when the API cannot
/// answer.
pub fn latest_by_page() -> Result<Option<Release>, CheckError> {
    let page = page_url().ok_or_else(|| {
        CheckError::Other("a test instance has no fake releases page".to_string())
    })?;
    let tag = latest_tag_by_redirect(&page)?;
    Ok(tag.map(|tag| {
        let notes = kept_notes(&tag);
        release_from_tag(&tag, &page, notes.as_deref())
    }))
}

#[cfg(test)]
mod tests;
