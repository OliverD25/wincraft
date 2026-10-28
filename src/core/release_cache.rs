//! GitHub's last full answer to "which release is the latest", kept in
//! `latest_release.json` with the ETag GitHub gave it. The next check sends
//! that tag back (`If-None-Match`); "304 Not Modified" then means the same
//! release as last time, and it is read from here. Without a login a 304
//! still counts toward GitHub's 60 requests an hour, but the answer is a few
//! bytes instead of the whole release.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config;
use crate::store::github::{self, Release};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cached {
    /// The address that answered, so a test instance's fake and GitHub
    /// never share an answer.
    pub url: String,
    pub etag: String,
    /// The answer as GitHub sent it.
    pub body: String,
}

/// What GitHub said to a conditional request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// A full answer, with the ETag when GitHub sent one.
    Fresh { body: String, etag: Option<String> },
    /// 304: the same as the answer kept here.
    NotModified,
    /// 404: no release is published yet.
    NotFound,
}

/// What to do with the kept answer afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Keep,
    Store(Cached),
    Clear,
}

pub fn path() -> PathBuf {
    config::data_dir().join("latest_release.json")
}

pub fn load_at(path: &Path) -> Option<Cached> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

pub fn store_at(path: &Path, cached: &Cached) {
    let result = serde_json::to_string(cached)
        .map_err(|err| err.to_string())
        .and_then(|text| config::write_atomic(path, &text));
    if let Err(err) = result {
        log::info!("the latest release answer could not be kept: {err}");
    }
}

/// The ETag to send to `url`, when an answer from it is kept.
pub fn etag_for<'a>(cached: Option<&'a Cached>, url: &str) -> Option<&'a str> {
    cached
        .filter(|cached| cached.url == url && !cached.etag.is_empty())
        .map(|cached| cached.etag.as_str())
}

/// The release GitHub's answer stands for, and what happens to the kept
/// answer. A 304 with nothing kept for this address is an error, so the
/// check fails as if offline and the next one asks in full.
pub fn resolve(
    answer: Answer,
    cached: Option<&Cached>,
    url: &str,
) -> Result<(Option<Release>, Change), String> {
    match answer {
        Answer::NotFound => Ok((None, Change::Clear)),
        Answer::NotModified => {
            let kept = cached
                .filter(|cached| cached.url == url)
                .ok_or_else(|| "GitHub said \"not modified\" but no answer is kept".to_string())?;
            github::parse_release(&kept.body).map(|release| (Some(release), Change::Keep))
        }
        Answer::Fresh { body, etag } => {
            let release = github::parse_release(&body)?;
            let change = match etag.filter(|etag| !etag.is_empty()) {
                Some(etag) => Change::Store(Cached {
                    url: url.to_string(),
                    etag,
                    body,
                }),
                None => Change::Clear,
            };
            Ok((Some(release), change))
        }
    }
}

/// Carries out `change` on the file at `path`.
pub fn apply_at(path: &Path, change: Change) {
    match change {
        Change::Keep => {}
        Change::Store(cached) => store_at(path, &cached),
        Change::Clear => {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://api.github.com/repos/OliverD25/wincraft/releases/latest";

    fn body(tag: &str) -> String {
        format!(r#"{{"tag_name": "{tag}", "body": "notes of {tag}"}}"#)
    }

    fn kept(tag: &str) -> Cached {
        Cached {
            url: URL.to_string(),
            etag: "\"abc\"".to_string(),
            body: body(tag),
        }
    }

    #[test]
    fn not_modified_reuses_the_kept_release() {
        let cached = kept("v0.21.0");
        let (release, change) = resolve(Answer::NotModified, Some(&cached), URL).unwrap();
        let release = release.expect("the kept release");
        assert_eq!(release.tag, "v0.21.0");
        assert_eq!(release.notes, "notes of v0.21.0");
        assert_eq!(change, Change::Keep);
    }

    #[test]
    fn not_modified_without_a_kept_answer_fails_like_offline() {
        assert!(resolve(Answer::NotModified, None, URL).is_err());
        let mut other = kept("v1.0.0");
        other.url = "http://127.0.0.1:9/latest".to_string();
        assert!(resolve(Answer::NotModified, Some(&other), URL).is_err());
    }

    #[test]
    fn a_fresh_answer_is_kept_with_its_etag_and_one_without_clears() {
        let fresh = Answer::Fresh {
            body: body("v0.22.0"),
            etag: Some("\"new\"".to_string()),
        };
        let (release, change) = resolve(fresh, Some(&kept("v0.21.0")), URL).unwrap();
        assert_eq!(release.unwrap().tag, "v0.22.0");
        assert_eq!(
            change,
            Change::Store(Cached {
                url: URL.to_string(),
                etag: "\"new\"".to_string(),
                body: body("v0.22.0"),
            })
        );
        let bare = Answer::Fresh {
            body: body("v0.22.0"),
            etag: None,
        };
        assert_eq!(resolve(bare, None, URL).unwrap().1, Change::Clear);
    }

    #[test]
    fn no_release_clears_and_a_broken_answer_is_an_error() {
        assert_eq!(
            resolve(Answer::NotFound, Some(&kept("v1.0.0")), URL).unwrap(),
            (None, Change::Clear)
        );
        let broken = Answer::Fresh {
            body: "not json".to_string(),
            etag: Some("x".to_string()),
        };
        assert!(resolve(broken, None, URL).is_err());
    }

    #[test]
    fn the_etag_goes_only_to_the_address_that_gave_it() {
        let cached = kept("v1.0.0");
        assert_eq!(etag_for(Some(&cached), URL), Some("\"abc\""));
        assert_eq!(etag_for(Some(&cached), "http://127.0.0.1:9/x"), None);
        assert_eq!(etag_for(None, URL), None);
    }

    #[test]
    fn the_kept_answer_round_trips_and_clears_on_disk() {
        let dir = std::env::temp_dir().join(format!("wincraft-latest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest_release.json");
        apply_at(&path, Change::Store(kept("v2.0.0")));
        assert_eq!(load_at(&path), Some(kept("v2.0.0")));
        apply_at(&path, Change::Keep);
        assert!(path.exists());
        apply_at(&path, Change::Clear);
        assert_eq!(load_at(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
