use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::config;
use crate::plugins;
use crate::store::github;

/// The committed copy is the one the plugin store downloads. A unit test
/// rebuilds this in memory and compares, so a plugin added without running
/// `wincraft --write-plugin-index` fails CI instead of going missing from the
/// store.
pub const COMMITTED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/plugins.json"));

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginIndex {
    pub wincraft_version: String,
    pub plugins: Vec<IndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub readme: String,
    /// Missing in an index written before categories existed.
    #[serde(default)]
    pub category: String,
    /// The plugin's own icon name; missing in older indexes and for a plugin
    /// that uses its category's icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Path of the tile picture, when the plugin has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
}

pub fn generate() -> PluginIndex {
    PluginIndex {
        wincraft_version: env!("CARGO_PKG_VERSION").to_string(),
        plugins: plugins::load_active_plugins()
            .iter()
            .map(|plugin| {
                let meta = plugin.metadata();
                IndexEntry {
                    id: meta.id.to_string(),
                    name: meta.name.to_string(),
                    version: meta.version.to_string(),
                    author: meta.author.to_string(),
                    description: meta.description.to_string(),
                    readme: format!("src/plugins/{}/README.md", meta.id),
                    category: meta.category.to_string(),
                    icon: meta.icon.map(str::to_string),
                    thumbnail: meta.thumbnail.map(str::to_string),
                }
            })
            .collect(),
    }
}

pub fn to_json(index: &PluginIndex) -> String {
    let mut text = serde_json::to_string_pretty(index).unwrap_or_default();
    text.push('\n');
    text
}

pub fn parse(text: &str) -> Result<PluginIndex, String> {
    serde_json::from_str(text).map_err(|err| format!("plugins.json is not valid JSON: {err}"))
}

/// Where a fetched index or README is kept so the store still has something to
/// show when the machine is offline.
fn cache_path(name: &str) -> PathBuf {
    config::cache_dir().join(name)
}

/// Keyed by what the file is, not by the URL it came from. Keying by URL would
/// silently throw away every user's cache the day the repository moves, and it
/// makes the offline path impossible to test by pointing the URL elsewhere.
fn cache_name_for(what: &str) -> String {
    let safe: String = what
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{safe}.cache")
}

fn write_cache(name: &str, text: &str) {
    let path = cache_path(name);
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let _ = std::fs::write(path, text);
}

fn read_cache(name: &str) -> Option<(String, std::time::SystemTime)> {
    let path = cache_path(name);
    let text = std::fs::read_to_string(&path).ok()?;
    let written = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());
    Some((text, written))
}

/// Where the answer came from, so the status line can say "updated 14:02",
/// "the cached copy from 21 Sep" or "the plugins built into this copy".
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Live,
    Cache(std::time::SystemTime),
    BuiltIn,
}

pub struct Fetched<T> {
    pub value: T,
    pub source: Source,
}

pub fn fetch_index() -> Result<Fetched<PluginIndex>, String> {
    let url = github::index_url();
    let name = cache_name_for("plugins.json");
    match github::get(&url) {
        Ok(body) => match parse(&body) {
            Ok(index) => {
                write_cache(&name, &body);
                log::info!("plugin index fetched ({} plugins)", index.plugins.len());
                Ok(Fetched {
                    value: index,
                    source: Source::Live,
                })
            }
            Err(err) => Err(err),
        },
        Err(err) => {
            if let Some((cached, written)) = read_cache(&name) {
                let index = parse(&cached)?;
                log::info!("offline, using cache ({} plugins)", index.plugins.len());
                return Ok(Fetched {
                    value: index,
                    source: Source::Cache(written),
                });
            }
            // Never nothing to show: the index this exe was built from is
            // compiled in, and it is exactly right about the plugins it has.
            let index = parse(COMMITTED)?;
            log::info!("offline with no cache, using the built-in index ({err})");
            Ok(Fetched {
                value: index,
                source: Source::BuiltIn,
            })
        }
    }
}

pub fn fetch_readme(path: &str) -> Result<Fetched<String>, String> {
    let url = github::readme_url(path);
    let name = cache_name_for(path);
    match github::get(&url) {
        Ok(body) => {
            write_cache(&name, &body);
            Ok(Fetched {
                value: body,
                source: Source::Live,
            })
        }
        Err(err) => match read_cache(&name) {
            Some((cached, written)) => Ok(Fetched {
                value: cached,
                source: Source::Cache(written),
            }),
            None => Err(format!("offline, and nothing cached yet ({err})")),
        },
    }
}

pub fn write_committed_copy() -> Result<std::path::PathBuf, String> {
    let path = std::path::PathBuf::from("plugins.json");
    std::fs::write(&path, to_json(&generate()))
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_index_matches_the_plugins_in_this_build() {
        let generated = generate();
        let committed = parse(COMMITTED).expect("the committed plugins.json parses");
        assert_eq!(
            committed, generated,
            "plugins.json is out of date; run: wincraft --write-plugin-index"
        );
    }

    #[test]
    fn an_index_with_a_category_and_a_thumbnail_survives_a_round_trip() {
        let index = PluginIndex {
            wincraft_version: "1.2.3".to_string(),
            plugins: vec![
                IndexEntry {
                    id: "a".to_string(),
                    name: "A".to_string(),
                    version: "1.0.0".to_string(),
                    author: "community".to_string(),
                    description: "First.".to_string(),
                    readme: "src/plugins/a/README.md".to_string(),
                    category: "Windows".to_string(),
                    icon: Some("tag".to_string()),
                    thumbnail: Some("src/plugins/a/thumbnail.png".to_string()),
                },
                IndexEntry {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    version: "1.0.0".to_string(),
                    author: "community".to_string(),
                    description: "Second.".to_string(),
                    readme: "src/plugins/b/README.md".to_string(),
                    category: "Input".to_string(),
                    icon: None,
                    thumbnail: None,
                },
            ],
        };
        let text = to_json(&index);
        assert!(text.contains("\"category\": \"Windows\""));
        assert!(text.contains("\"icon\": \"tag\""));
        assert_eq!(text.matches("\"icon\":").count(), 1, "none is left out");
        assert_eq!(
            text.matches("\"thumbnail\":").count(),
            1,
            "none is left out"
        );
        assert_eq!(parse(&text).unwrap(), index);
    }

    #[test]
    fn an_index_from_before_categories_still_reads() {
        let old = r#"{"wincraft_version":"0.19.0","plugins":[{"id":"a","name":"A","version":"1.0.0","author":"x","description":"d","readme":"r"}]}"#;
        let index = parse(old).expect("an old index parses");
        assert_eq!(index.plugins[0].category, "");
        assert_eq!(index.plugins[0].thumbnail, None);
        assert_eq!(index.plugins[0].icon, None);
    }

    #[test]
    fn every_plugin_has_a_category() {
        for plugin in plugins::load_active_plugins() {
            let meta = plugin.metadata();
            assert!(
                !meta.category.trim().is_empty(),
                "{} has no category",
                meta.id
            );
        }
    }

    #[test]
    fn every_plugin_has_a_readme_and_a_description() {
        for plugin in plugins::load_active_plugins() {
            let meta = plugin.metadata();
            assert!(
                !meta.readme.trim().is_empty(),
                "{} has an empty README",
                meta.id
            );
            assert!(
                meta.readme.starts_with("# "),
                "{} README should open with a heading",
                meta.id
            );
            assert!(
                !meta.description.trim().is_empty(),
                "{} has no description",
                meta.id
            );
        }
    }
}
