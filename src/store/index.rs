use serde::{Deserialize, Serialize};

use crate::modules;

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
