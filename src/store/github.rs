use std::time::Duration;

const RAW_BASE: &str = "https://raw.githubusercontent.com/OliverD25/wincraft/main";
const RELEASES_LATEST: &str = "https://api.github.com/repos/OliverD25/wincraft/releases/latest";
const TIMEOUT: Duration = Duration::from_secs(5);

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

pub fn releases_url() -> &'static str {
    "https://github.com/OliverD25/wincraft/releases"
}

pub fn get(url: &str) -> Result<String, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        // GitHub's API rejects requests without one.
        .user_agent(concat!("WinCraft/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent();
    agent
        .get(url)
        .call()
        .map_err(|err| format!("{err}"))?
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("{err}"))
}

/// Reads `tag_name` out of the releases API without a JSON model, because one
/// field is not worth a struct that has to track GitHub's schema.
pub fn latest_release_tag() -> Result<String, String> {
    let body = get(RELEASES_LATEST)?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|err| format!("unexpected reply: {err}"))?;
    value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .map(|tag| tag.to_string())
        .ok_or_else(|| "the reply had no tag_name".to_string())
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
    fn the_index_url_can_be_pointed_somewhere_else() {
        assert!(index_url().ends_with("plugins.json"));
    }
}
