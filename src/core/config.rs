use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use crate::core::wide;

pub const DEFAULT_PALETTE_HOTKEY: &str = "Win+Alt+P";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default = "default_palette_hotkey")]
    pub palette_hotkey: String,
    #[serde(default)]
    pub search: SearchConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            theme: ThemeChoice::System,
            palette_hotkey: DEFAULT_PALETTE_HOTKEY.to_string(),
            search: SearchConfig::default(),
        }
    }
}

/// The palette's search. Read at startup only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Provider id → the prefix that sends a query to it. A provider left
    /// out keeps its built-in prefix; an empty string switches it off.
    #[serde(default = "default_prefixes")]
    pub prefixes: BTreeMap<String, String>,
    /// Web search address, with `{query}` where the words go.
    #[serde(default = "default_web_url")]
    pub web_url: String,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            prefixes: default_prefixes(),
            web_url: default_web_url(),
        }
    }
}

/// Written out in full so the file shows every prefix that can be changed.
fn default_prefixes() -> BTreeMap<String, String> {
    [
        ("windows", "<"),
        ("paths", "/"),
        ("calc", "="),
        ("web", "?"),
    ]
    .into_iter()
    .map(|(id, prefix)| (id.to_string(), prefix.to_string()))
    .collect()
}

fn default_web_url() -> String {
    crate::search::providers::web::DEFAULT_URL.to_string()
}

fn default_palette_hotkey() -> String {
    DEFAULT_PALETTE_HOTKEY.to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginConfig {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub hotkeys: BTreeMap<String, String>,
    #[serde(default = "empty_object")]
    pub settings: Value,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkeys: BTreeMap::new(),
            settings: empty_object(),
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("WinCraft")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn log_path() -> PathBuf {
    data_dir().join("wincraft.log")
}

pub fn plugins_dir() -> PathBuf {
    data_dir().join("plugins")
}

pub fn plugin_path(id: &str) -> PathBuf {
    plugins_dir().join(format!("{id}.json"))
}

pub fn cache_dir() -> PathBuf {
    data_dir().join("cache")
}

/// Files that existed but could not be read at start. Saving over one would
/// replace everything the user wrote with defaults, so they are left alone
/// and the defaults live in memory only.
static UNREADABLE: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn mark_unreadable(path: &Path) {
    if let Ok(mut unreadable) = UNREADABLE.lock() {
        unreadable.insert(path.to_path_buf());
    }
}

fn is_unreadable(path: &Path) -> bool {
    UNREADABLE
        .lock()
        .map(|unreadable| unreadable.contains(path))
        .unwrap_or(false)
}

/// Notepad and Windows PowerShell 5 write UTF-8 with a byte order mark, which
/// serde_json rejects as "expected value at line 1 column 1".
pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Like [`write_atomic`], but refuses a file that could not be read at start.
fn save_readable(path: &Path, text: &str) -> Result<(), String> {
    if is_unreadable(path) {
        return Err(format!(
            "{} could not be read at start, so it is kept as it is; \
             fix or delete it and restart WinCraft to save changes",
            path.display()
        ));
    }
    write_atomic(path, text)
}

/// Writes through a temporary file so a crash mid-write cannot leave a
/// half-written file that the next start refuses to read.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;

    // std::fs::rename asks for POSIX rename semantics first and reports
    // ERROR_NOT_SAME_DEVICE on filesystems that refuse them, even inside one
    // folder. MoveFileExW is the plain Win32 replace and always works here.
    let replaced = unsafe {
        MoveFileExW(
            wide(&tmp.to_string_lossy()).as_ptr(),
            wide(&path.to_string_lossy()).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) != 0
    };
    if !replaced {
        let _ = fs::remove_file(&tmp);
        fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        let Ok(text) = fs::read_to_string(&path) else {
            log::info!("no config at {}, using defaults", path.display());
            return Self::default();
        };
        let (config, legacy) = match Self::parse(&text) {
            Ok(parsed) => parsed,
            Err(err) => {
                log::warn!(
                    "config.json cannot be read ({err}); using defaults and leaving the file as it is"
                );
                mark_unreadable(&path);
                return Self::default();
            }
        };

        if !legacy.is_empty() {
            for (id, plugin) in legacy {
                let target = plugin_path(&id);
                if target.exists() {
                    log::info!("{id} already has {}, leaving it alone", target.display());
                    continue;
                }
                match plugin.save(&id) {
                    Ok(()) => log::info!("migrated {id} settings to {}", target.display()),
                    Err(err) => log::error!("could not migrate {id}: {err}"),
                }
            }
            // Saving now rewrites config.json without the "modules" key, because
            // Config has no field for it.
            if let Err(err) = config.save() {
                log::error!("could not rewrite config.json after migration: {err}");
            }
        }
        config
    }

    /// A field of the wrong type fails the whole file instead of quietly
    /// becoming a default, because the file would then be saved without it.
    fn parse(text: &str) -> Result<(Self, Vec<(String, PluginConfig)>), String> {
        let mut raw: Value =
            serde_json::from_str(strip_bom(text)).map_err(|e| format!("not valid JSON: {e}"))?;
        let legacy = take_legacy_plugins(&mut raw);
        let config = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        Ok((config, legacy))
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialise config: {e}"))?;
        save_readable(&config_path(), &text)
    }
}

/// Pulls the 0.2-shaped `modules` map out of a parsed config.json. Left as a
/// plain function on a Value so the migration can be tested without touching
/// the user's real files.
///
/// The key keeps its old name on purpose: it is what 0.2 wrote to disk, so
/// renaming it with the rest of the code would strand every 0.2 user's settings.
pub fn take_legacy_plugins(raw: &mut Value) -> Vec<(String, PluginConfig)> {
    let Some(object) = raw.as_object_mut() else {
        return Vec::new();
    };
    let Some(legacy) = object.remove("modules") else {
        return Vec::new();
    };
    let Some(legacy) = legacy.as_object() else {
        return Vec::new();
    };
    legacy
        .iter()
        .filter_map(|(id, value)| match serde_json::from_value(value.clone()) {
            Ok(plugin) => Some((id.clone(), plugin)),
            Err(err) => {
                log::warn!("cannot read the old settings for {id}: {err}");
                None
            }
        })
        .collect()
}

impl PluginConfig {
    pub fn load(id: &str) -> Self {
        let path = plugin_path(id);
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str(strip_bom(&text)) {
            Ok(plugin) => plugin,
            Err(err) => {
                log::warn!(
                    "{} cannot be read ({err}); using defaults and leaving the file as it is",
                    path.display()
                );
                mark_unreadable(&path);
                Self::default()
            }
        }
    }

    pub fn save(&self, id: &str) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialise the settings for {id}: {e}"))?;
        save_readable(&plugin_path(id), &text)
    }

    pub fn reset(id: &str) -> Result<(), String> {
        let path = plugin_path(id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("cannot delete {}: {err}", path.display())),
        }
    }

    /// Fills in keys the file does not have yet, so every editable option shows
    /// up on disk even when the plugin gained it after the file was written.
    pub fn merge_defaults(&mut self, hotkeys: &[(&str, String)], settings: &Value) {
        for (name, default) in hotkeys {
            self.hotkeys
                .entry((*name).to_string())
                .or_insert_with(|| default.clone());
        }
        if let Some(defaults) = settings.as_object() {
            if !self.settings.is_object() {
                self.settings = empty_object();
            }
            let target = self.settings.as_object_mut().expect("just made an object");
            for (key, value) in defaults {
                target.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V02_CONFIG: &str = r#"{
      "start_with_windows": true,
      "modules": {
        "screen_dimmer": {
          "enabled": true,
          "hotkeys": { "toggle_monitor_1": "Win+Alt+F1" },
          "settings": { "idle_opacity": 1.0, "hover_opacity": 0.4 }
        },
        "shortcut_detector": {
          "enabled": false,
          "hotkeys": { "open_window": "Win+Alt+Q" },
          "settings": { "show_free_by_default": true }
        }
      }
    }"#;

    #[test]
    fn migration_moves_every_plugin_out_of_the_host_file() {
        let mut raw: Value = serde_json::from_str(V02_CONFIG).unwrap();
        let mut legacy = take_legacy_plugins(&mut raw);
        legacy.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].0, "screen_dimmer");
        assert!(legacy[0].1.enabled);
        assert_eq!(
            legacy[0]
                .1
                .hotkeys
                .get("toggle_monitor_1")
                .map(String::as_str),
            Some("Win+Alt+F1")
        );
        assert_eq!(legacy[0].1.settings["hover_opacity"], 0.4);
        assert_eq!(legacy[1].0, "shortcut_detector");
        assert!(!legacy[1].1.enabled);
    }

    #[test]
    fn migration_leaves_the_host_settings_behind_and_drops_the_modules_key() {
        let mut raw: Value = serde_json::from_str(V02_CONFIG).unwrap();
        take_legacy_plugins(&mut raw);
        assert!(raw.get("modules").is_none());

        let config: Config = serde_json::from_value(raw).unwrap();
        assert!(config.start_with_windows);
        assert_eq!(config.theme, ThemeChoice::System);
        assert_eq!(config.palette_hotkey, DEFAULT_PALETTE_HOTKEY);

        let written = serde_json::to_string(&config).unwrap();
        assert!(!written.contains("modules"));
    }

    #[test]
    fn a_config_without_modules_migrates_nothing() {
        let mut raw: Value = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert!(take_legacy_plugins(&mut raw).is_empty());
        let config: Config = serde_json::from_value(raw).unwrap();
        assert_eq!(config.theme, ThemeChoice::Dark);
    }

    #[test]
    fn missing_files_fall_back_to_defaults() {
        let config: Config = serde_json::from_str("{}").unwrap();
        assert!(!config.start_with_windows);
        assert_eq!(config.theme, ThemeChoice::System);
        assert_eq!(config.palette_hotkey, DEFAULT_PALETTE_HOTKEY);

        assert_eq!(config.search, SearchConfig::default());
        assert_eq!(config.search.prefixes["paths"], "/");

        let plugin: PluginConfig = serde_json::from_str("{}").unwrap();
        assert!(plugin.enabled);
        assert!(plugin.hotkeys.is_empty());
        assert!(plugin.settings.is_object());
    }

    #[test]
    fn a_byte_order_mark_does_not_stop_a_file_from_parsing() {
        let (config, _) = Config::parse("\u{feff}{\"theme\":\"light\"}").unwrap();
        assert_eq!(config.theme, ThemeChoice::Light);

        let plugin: PluginConfig =
            serde_json::from_str(strip_bom("\u{feff}{\"enabled\":false}")).unwrap();
        assert!(!plugin.enabled);
        assert_eq!(strip_bom("{}"), "{}");
    }

    #[test]
    fn a_search_section_with_only_some_keys_keeps_the_other_defaults() {
        let (config, _) = Config::parse(r#"{"search":{"prefixes":{"web":"g "}}}"#).unwrap();
        assert_eq!(config.search.prefixes.len(), 1);
        assert_eq!(config.search.prefixes["web"], "g ");
        assert_eq!(config.search.web_url, default_web_url());
    }

    #[test]
    fn a_field_of_the_wrong_type_fails_the_file_instead_of_defaulting() {
        assert!(Config::parse(r#"{"theme":"blue"}"#).is_err());
        assert!(Config::parse("not json").is_err());
    }

    #[test]
    fn a_file_that_could_not_be_read_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("wincraft-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.json");
        fs::write(&path, "{ broken").unwrap();

        mark_unreadable(&path);
        assert!(save_readable(&path, "{}").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");

        let other = dir.join("fine.json");
        save_readable(&other, "{}").unwrap();
        assert_eq!(fs::read_to_string(&other).unwrap(), "{}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_defaults_never_overwrites_what_the_user_wrote() {
        let mut plugin: PluginConfig =
            serde_json::from_str(r#"{"hotkeys":{"a":"Win+Alt+Z"},"settings":{"x":5}}"#).unwrap();
        plugin.merge_defaults(
            &[
                ("a", "Win+Alt+A".to_string()),
                ("b", "Win+Alt+B".to_string()),
            ],
            &serde_json::json!({ "x": 1, "y": 2 }),
        );
        assert_eq!(plugin.hotkeys["a"], "Win+Alt+Z");
        assert_eq!(plugin.hotkeys["b"], "Win+Alt+B");
        assert_eq!(plugin.settings["x"], 5);
        assert_eq!(plugin.settings["y"], 2);
    }
}
