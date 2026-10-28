use std::time::Duration;

use crate::core::instance;
use crate::core::release_cache::{self, Answer};

const RAW_BASE: &str = "https://raw.githubusercontent.com/OliverD25/wincraft/main";
const RELEASES_LATEST: &str = "https://api.github.com/repos/OliverD25/wincraft/releases/latest";
const RELEASES_BY_TAG: &str = "https://api.github.com/repos/OliverD25/wincraft/releases/tags/";
const TIMEOUT: Duration = Duration::from_secs(5);
/// Points a test instance's update check at a local fake of the releases
/// API. Ignored without `WINCRAFT_INSTANCE`.
pub const RELEASES_ENV: &str = "WINCRAFT_RELEASES_URL";

pub fn index_url() -> String {
    // Pointing this at a host that does not resolve is how the offline path
    // gets tested without unplugging anything.
    std::env::var("WINCRAFT_INDEX_URL").unwrap_or_else(|_| format!("{RAW_BASE}/plugins.json"))
}

pub fn readme_url(path: &str) -> String {
    format!("{RAW_BASE}/{path}")
}

pub fn repo_url() -> &'static str {
    "https://github.com/OliverD25/wincraft"
}

pub fn issues_url() -> &'static str {
    "https://github.com/OliverD25/wincraft/issues"
}

pub fn release_url(tag: &str) -> String {
    format!("https://github.com/OliverD25/wincraft/releases/tag/{tag}")
}

pub fn releases_url() -> &'static str {
    "https://github.com/OliverD25/wincraft/releases"
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        // GitHub's API rejects requests without one.
        .user_agent(concat!("WinCraft/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

pub fn get(url: &str) -> Result<String, String> {
    agent()
        .get(url)
        .call()
        .map_err(|err| format!("{err}"))?
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("{err}"))
}

/// A GET that sends `etag` back as `If-None-Match`, so GitHub can answer
/// "304 Not Modified" when nothing changed.
pub fn get_if_changed(url: &str, etag: Option<&str>) -> Result<Answer, String> {
    let mut request = agent().get(url);
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    let mut response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => return Ok(Answer::NotFound),
        Err(err) => return Err(format!("{err}")),
    };
    if response.status().as_u16() == 304 {
        return Ok(Answer::NotModified);
    }
    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("{err}"))?;
    Ok(Answer::Fresh { body, etag })
}

/// A test instance's fake releases API, when it names one.
pub fn releases_override() -> Option<String> {
    let url = std::env::var(RELEASES_ENV).ok()?;
    (instance::is_test() && !url.trim().is_empty()).then(|| url.trim().to_string())
}

/// The newest published release, as much of it as the updater uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub tag: String,
    /// The release notes, Markdown.
    pub notes: String,
    /// The release's page on GitHub.
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// None while the repository has no release: GitHub answers 404 until the
/// first one is published, which is a different fact from being offline.
/// The last answer is kept with its ETag (see `release_cache`), so an
/// unchanged release costs GitHub a "304 Not Modified" only.
pub fn latest_release() -> Result<Option<Release>, String> {
    let url = releases_override().unwrap_or_else(|| RELEASES_LATEST.to_string());
    let path = release_cache::path();
    let cached = release_cache::load_at(&path);
    let answer = get_if_changed(&url, release_cache::etag_for(cached.as_ref(), &url))?;
    if answer == Answer::NotModified {
        log::info!("update check: GitHub says the latest release has not changed");
    }
    let (release, change) = release_cache::resolve(answer, cached.as_ref(), &url)?;
    release_cache::apply_at(&path, change);
    Ok(release)
}

/// One release by its tag, such as `v0.16.2`; None when there is no such
/// release.
pub fn release_by_tag(tag: &str) -> Result<Option<Release>, String> {
    let url = releases_override().unwrap_or_else(|| format!("{RELEASES_BY_TAG}{tag}"));
    match get(&url) {
        Ok(body) => parse_release(&body).map(Some),
        Err(err) if err.contains("404") => Ok(None),
        Err(err) => Err(err),
    }
}

/// Read without a full JSON model, because a handful of fields is not worth
/// a struct that has to track GitHub's schema.
pub fn parse_release(body: &str) -> Result<Release, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|err| format!("unexpected reply: {err}"))?;
    let text = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(|field| field.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let tag = text(&value, "tag_name");
    if tag.is_empty() {
        return Err("the reply had no tag_name".to_string());
    }
    let assets = value
        .get("assets")
        .and_then(|assets| assets.as_array())
        .map(|assets| {
            assets
                .iter()
                .map(|asset| Asset {
                    name: text(asset, "name"),
                    url: text(asset, "browser_download_url"),
                })
                .filter(|asset| !asset.name.is_empty() && !asset.url.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let page = Some(text(&value, "html_url"))
        .filter(|page| !page.is_empty())
        .unwrap_or_else(|| release_url(&tag));
    Ok(Release {
        notes: text(&value, "body"),
        page,
        assets,
        tag,
    })
}

/// True when `latest` is a higher version than `current`. Both may carry a
/// leading `v`; anything unparseable counts as "no newer version", because
/// nagging about an update that does not exist is worse than missing one.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |text: &str| -> Option<(u64, u64, u64)> {
        let text = text.trim().trim_start_matches(['v', 'V']);
        let mut parts = text.split('.').map(|part| {
            part.split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("")
                .parse::<u64>()
                .ok()
        });
        Some((parts.next()??, parts.next()??, parts.next()??))
    };
    match (parse(latest), parse(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_higher_version_is_newer() {
        assert!(is_newer("v0.4.0", "0.3.0"));
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
    }

    #[test]
    fn the_same_or_older_version_is_not_newer() {
        assert!(!is_newer("v0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
    }

    #[test]
    fn a_tag_that_makes_no_sense_never_nags() {
        assert!(!is_newer("nightly", "0.3.0"));
        assert!(!is_newer("", "0.3.0"));
        assert!(!is_newer("v1", "0.3.0"));
    }

    #[test]
    fn a_suffix_on_the_patch_number_still_compares() {
        assert!(is_newer("v0.4.0-beta.1", "0.3.0"));
    }

    #[test]
    fn a_release_reply_gives_its_tag_notes_page_and_assets() {
        let body = r#"{
            "tag_name": "v0.16.0",
            "html_url": "https://github.com/OliverD25/wincraft/releases/tag/v0.16.0",
            "body": "**Changes**\n* Updates",
            "assets": [
                {"name": "wincraft.exe", "browser_download_url": "https://github.com/OliverD25/wincraft/releases/download/v0.16.0/wincraft.exe"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/OliverD25/wincraft/releases/download/v0.16.0/SHA256SUMS.txt"},
                {"name": "no-url.txt"}
            ]
        }"#;
        let release = parse_release(body).unwrap();
        assert_eq!(release.tag, "v0.16.0");
        assert_eq!(release.notes, "**Changes**\n* Updates");
        assert!(release.page.ends_with("/releases/tag/v0.16.0"));
        let names: Vec<&str> = release.assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["wincraft.exe", "SHA256SUMS.txt"]);
    }

    #[test]
    fn a_reply_without_a_tag_is_an_error_and_missing_parts_are_empty() {
        assert!(parse_release(r#"{"name": "x"}"#).is_err());
        assert!(parse_release("not json").is_err());
        let release = parse_release(r#"{"tag_name": "v1.0.0"}"#).unwrap();
        assert!(release.notes.is_empty() && release.assets.is_empty());
        assert_eq!(release.page, release_url("v1.0.0"));
    }

    #[test]
    fn the_index_url_can_be_pointed_somewhere_else() {
        assert!(index_url().ends_with("plugins.json"));
    }
}
