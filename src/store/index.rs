use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::config;
use crate::modules;
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
}

pub fn generate() -> PluginIndex {
    PluginIndex {
        wincraft_version: env!("CARGO_PKG_VERSION").to_string(),
        plugins: modules::load_active_modules()
            .iter()
            .map(|module| {
                let meta = module.metadata();
                IndexEntry {
                    id: meta.id.to_string(),
                    name: meta.name.to_string(),
                    version: meta.version.to_string(),
                    author: meta.author.to_string(),
                    description: meta.description.to_string(),
                    readme: format!("src/modules/{}/README.md", meta.id),
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

fn cache_name_for(url: &str) -> String {
    let safe: String = url
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

fn read_cache(name: &str) -> Option<String> {
    std::fs::read_to_string(cache_path(name)).ok()
}

pub struct Fetched<T> {
    pub value: T,
    pub note: String,
}

pub fn fetch_index() -> Result<Fetched<PluginIndex>, String> {
    let url = github::index_url();
    let name = cache_name_for(&url);
    match github::get(&url) {
        Ok(body) => match parse(&body) {
            Ok(index) => {
                write_cache(&name, &body);
                log::info!("plugin index fetched ({} plugins)", index.plugins.len());
                Ok(Fetched {
                    value: index,
                    note: format!("{} plugins listed", index_count(&body)),
                })
            }
            Err(err) => Err(err),
        },
        Err(err) => {
            if let Some(cached) = read_cache(&name) {
                let index = parse(&cached)?;
                log::info!("offline, using cache ({} plugins)", index.plugins.len());
                return Ok(Fetched {
                    value: index,
                    note: "offline, showing the cached copy".to_string(),
                });
            }
            // Never nothing to show: the index this exe was built from is
            // compiled in, and it is exactly right about the plugins it has.
            let index = parse(COMMITTED)?;
            log::info!("offline with no cache, using the built-in index ({err})");
            Ok(Fetched {
                value: index,
                note: "offline, showing the plugins built into this copy".to_string(),
            })
        }
    }
}

pub fn fetch_readme(path: &str) -> Result<Fetched<String>, String> {
    let url = github::readme_url(path);
    let name = cache_name_for(&url);
    match github::get(&url) {
        Ok(body) => {
            write_cache(&name, &body);
            Ok(Fetched {
                value: body,
                note: String::new(),
            })
        }
        Err(err) => match read_cache(&name) {
            Some(cached) => Ok(Fetched {
                value: cached,
                note: "offline, showing the cached copy".to_string(),
            }),
            None => Err(format!("offline, and nothing cached yet ({err})")),
        },
    }
}

fn index_count(body: &str) -> usize {
    parse(body).map(|index| index.plugins.len()).unwrap_or(0)
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
    fn every_plugin_has_a_readme_and_a_description() {
        for module in modules::load_active_modules() {
            let meta = module.metadata();
            assert!(!meta.readme.trim().is_empty(), "{} has an empty README", meta.id);
            assert!(
                meta.readme.starts_with("# "),
                "{} README should open with a heading",
                meta.id
            );
            assert!(!meta.description.trim().is_empty(), "{} has no description", meta.id);
        }
    }
}
