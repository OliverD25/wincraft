use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use crate::core::tips::TipsConfig;
use crate::core::wide;

pub const DEFAULT_PALETTE_HOTKEY: &str = "Win+Alt+P";

/// The mode: dark, light, or whatever Windows uses for apps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
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

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "system" => Some(ThemeChoice::System),
            "light" => Some(ThemeChoice::Light),
            "dark" => Some(ThemeChoice::Dark),
            _ => None,
        }
    }
}

/// An unknown word falls back to the default instead of failing the whole
/// file, so a config written by a newer WinCraft still opens.
impl<'de> Deserialize<'de> for ThemeChoice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Ok(Self::from_key(&key).unwrap_or_else(|| {
            log::warn!("unknown mode \"{key}\" in config.json, using System");
            ThemeChoice::System
        }))
    }
}

/// The look: colours, corners, row height and fonts. Each has a dark and a
/// light set of tokens in `core::theme`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    #[default]
    Classic,
    Fluent,
    Terminal,
    Paper,
    HighContrast,
}

impl ThemeName {
    /// In the order the theme picker shows them.
    pub const ALL: [ThemeName; 5] = [
        ThemeName::Classic,
        ThemeName::Fluent,
        ThemeName::Terminal,
        ThemeName::Paper,
        ThemeName::HighContrast,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ThemeName::Classic => "classic",
            ThemeName::Fluent => "fluent",
            ThemeName::Terminal => "terminal",
            ThemeName::Paper => "paper",
            ThemeName::HighContrast => "high-contrast",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|name| name.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Classic => "Classic",
            ThemeName::Fluent => "Fluent",
            ThemeName::Terminal => "Terminal",
            ThemeName::Paper => "Paper",
            ThemeName::HighContrast => "High contrast",
        }
    }

    /// The line under the name on the theme's tile.
    pub fn blurb(self) -> &'static str {
        match self {
            ThemeName::Classic => "Today's look",
            ThemeName::Fluent => "Windows 11 glass",
            ThemeName::Terminal => "Dense, monospace",
            ThemeName::Paper => "Serif, like a book",
            ThemeName::HighContrast => "Black and white, strong borders",
        }
    }
}

impl<'de> Deserialize<'de> for ThemeName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let key = String::deserialize(deserializer)?;
        Ok(Self::from_key(&key).unwrap_or_else(|| {
            log::warn!("unknown theme \"{key}\" in config.json, using Classic");
            ThemeName::Classic
        }))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub theme: ThemeName,
    #[serde(default)]
    pub mode: ThemeChoice,
    #[serde(default = "default_palette_hotkey")]
    pub palette_hotkey: String,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub updates: UpdateConfig,
    #[serde(default)]
    pub tips: TipsConfig,
    /// Shortcuts of palette commands that have no hotkey of their own, by
    /// command id: "Win+Alt+K, S". Only what the user set is here; a
    /// plugin's own hotkeys stay in its file under `hotkeys`.
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,
    /// The detailed log (`core::detailed`): off by default. Kept here so it
    /// stays on after a restart and an update, until the user turns it off.
    #[serde(default)]
    pub detailed_log: bool,
    /// Where the user dragged the palette to, by monitor device name
    /// (for example `DISPLAY1`). A monitor with no entry opens it centred.
    #[serde(default)]
    pub palette_spots: BTreeMap<String, PaletteSpot>,
}

/// A dragged palette panel's top-left corner, in physical pixels from the top
/// left of its monitor's work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteSpot {
    pub dx: i32,
    pub dy: i32,
}

/// A monitor's work area in physical pixels: left, top, right, bottom.
pub type WorkArea = (i32, i32, i32, i32);

impl PaletteSpot {
    /// The spot of a panel whose top-left corner is at `(x, y)` on the screen.
    pub fn from_screen(work: WorkArea, x: i32, y: i32) -> Self {
        Self {
            dx: x - work.0,
            dy: y - work.1,
        }
    }

    /// The panel's top-left corner on the screen. The whole `size` panel is
    /// kept inside the work area, so a monitor that got smaller never hides it.
    pub fn to_screen(self, work: WorkArea, size: (i32, i32)) -> (i32, i32) {
        let x = (work.0 + self.dx).min(work.2 - size.0).max(work.0);
        let y = (work.1 + self.dy).min(work.3 - size.1).max(work.1);
        (x, y)
    }
}

/// The default panel spot: centred, with the top edge 18 % down the work area.
pub fn centred_spot(work: WorkArea, width: i32) -> (i32, i32) {
    let x = work.0 + ((work.2 - work.0) - width) / 2;
    let y = work.1 + ((work.3 - work.1) as f32 * 0.18) as i32;
    (x, y)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            theme: ThemeName::Classic,
            mode: ThemeChoice::System,
            palette_hotkey: DEFAULT_PALETTE_HOTKEY.to_string(),
            search: SearchConfig::default(),
            updates: UpdateConfig::default(),
            tips: TipsConfig::default(),
            shortcuts: BTreeMap::new(),
            detailed_log: false,
            palette_spots: BTreeMap::new(),
        }
    }
}

/// The update check. Nothing is installed until the user clicks Update now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Whether GitHub is asked on its own for a newer release: every hour,
    /// after a wake and when the palette opens. The key keeps its old name
    /// so older config files still load.
    #[serde(default = "enabled_by_default")]
    pub check_daily: bool,
    /// When GitHub last answered, in seconds since 1970; 0 for never.
    #[serde(default)]
    pub last_check: u64,
    /// The newest release tag seen. The tray says once per new tag that an
    /// update is there.
    #[serde(default)]
    pub latest: String,
    /// Whether a newer release is downloaded and checked in the background,
    /// so Update now takes seconds. It never installs on its own.
    #[serde(default = "enabled_by_default")]
    pub download_ahead: bool,
    /// The release tag the user hid from the palette with Ctrl+Del; a newer
    /// tag shows again. Empty for none.
    #[serde(default)]
    pub hidden_version: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_daily: true,
            last_check: 0,
            latest: String::new(),
            download_ahead: true,
            hidden_version: String::new(),
        }
    }
}

/// The palette's search. The prefixes are read at startup; the rest is
/// changed from the settings window and applies at once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Provider id → the prefix that sends a query to it. A provider left
    /// out keeps its built-in prefix; an empty string switches it off.
    #[serde(default = "default_prefixes")]
    pub prefixes: BTreeMap<String, String>,
    /// Web search address, with `{query}` where the words go.
    #[serde(default = "default_web_url")]
    pub web_url: String,
    /// What the `>` prefix runs commands in: one of
    /// `search::providers::shell::SHELL_NAMES`.
    #[serde(default = "default_terminal_shell")]
    pub terminal_shell: String,
    /// The command template for the "Custom" shell, with `{cmd}` and
    /// optionally `{cwd}`.
    #[serde(default)]
    pub terminal_custom: String,
    /// Whether the commands run from the palette are remembered in
    /// terminal_history.json.
    #[serde(default = "enabled_by_default")]
    pub terminal_history: bool,
    /// Whether rows picked before rank higher, and picks are recorded in
    /// palette_usage.json.
    #[serde(default = "enabled_by_default")]
    pub rank_by_use: bool,
    /// Whether files and folders are searched at all, in the blended list and
    /// after the `.` prefix: with Everything when it is running. The key kept
    /// its first name.
    #[serde(default = "enabled_by_default")]
    pub everything: bool,
    /// Whether the Windows Search index answers file searches while
    /// Everything is not running.
    #[serde(default = "enabled_by_default")]
    pub file_search_fallback: bool,
    /// The AI website Ask AI opens with the question: one of
    /// `search::ask_ai::SITE_NAMES`.
    #[serde(default = "default_ask_ai_site")]
    pub ask_ai_site: String,
    /// The address for the "Custom URL" site, with `{query}` where the
    /// question goes.
    #[serde(default)]
    pub ask_ai_custom: String,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            prefixes: default_prefixes(),
            web_url: default_web_url(),
            terminal_shell: default_terminal_shell(),
            terminal_custom: String::new(),
            terminal_history: true,
            rank_by_use: true,
            everything: true,
            file_search_fallback: true,
            ask_ai_site: default_ask_ai_site(),
            ask_ai_custom: String::new(),
        }
    }
}

/// Written out in full so the file shows every prefix that can be changed.
fn default_prefixes() -> BTreeMap<String, String> {
    [
        ("windows", "<"),
        ("paths", "/"),
        ("files", "."),
        ("bookmarks", "'"),
        ("calc", "="),
        ("terminal", ">"),
        ("web", "?"),
    ]
    .into_iter()
    .map(|(id, prefix)| (id.to_string(), prefix.to_string()))
    .collect()
}

fn default_terminal_shell() -> String {
    crate::search::providers::shell::DEFAULT_SHELL.to_string()
}

fn default_web_url() -> String {
    crate::search::providers::web::DEFAULT_URL.to_string()
}

fn default_ask_ai_site() -> String {
    crate::search::ask_ai::DEFAULT_SITE.to_string()
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
    config_in(&data_dir())
}

/// The loaders and savers below take the data folder, so a test can point
/// them at a scratch one without changing LOCALAPPDATA under the other tests.
fn config_in(data: &Path) -> PathBuf {
    data.join("config.json")
}

pub fn log_path() -> PathBuf {
    data_dir().join("wincraft.log")
}

pub fn plugins_dir() -> PathBuf {
    data_dir().join("plugins")
}

pub fn plugin_path(id: &str) -> PathBuf {
    plugin_in(&data_dir(), id)
}

fn plugin_in(data: &Path, id: &str) -> PathBuf {
    data.join("plugins").join(format!("{id}.json"))
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
        Self::load_in(&data_dir())
    }

    pub fn load_in(data: &Path) -> Self {
        let path = config_in(data);
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
                let target = plugin_in(data, &id);
                if target.exists() {
                    log::info!("{id} already has {}, leaving it alone", target.display());
                    continue;
                }
                match plugin.save_in(data, &id) {
                    Ok(()) => log::info!("migrated {id} settings to {}", target.display()),
                    Err(err) => log::error!("could not migrate {id}: {err}"),
                }
            }
            // Saving now rewrites config.json without the "modules" key, because
            // Config has no field for it.
            if let Err(err) = config.save_in(data) {
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
        move_old_theme_to_mode(&mut raw);
        let config = serde_json::from_value(raw).map_err(|e| e.to_string())?;
        Ok((config, legacy))
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_in(&data_dir())
    }

    pub fn save_in(&self, data: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialise config: {e}"))?;
        save_readable(&config_in(data), &text)
    }
}

/// Up to 0.20.9 `theme` held System, Light or Dark, which is now the mode.
/// Only the copy in memory changes: the file is rewritten at the next save,
/// so an older WinCraft put back by a failed update still reads it.
pub fn move_old_theme_to_mode(raw: &mut Value) {
    let Some(object) = raw.as_object_mut() else {
        return;
    };
    let Some(old) = object
        .get("theme")
        .and_then(Value::as_str)
        .filter(|key| ThemeChoice::from_key(key).is_some())
        .map(str::to_string)
    else {
        return;
    };
    object.entry("mode").or_insert(Value::String(old));
    object.insert(
        "theme".to_string(),
        Value::String(ThemeName::Classic.key().to_string()),
    );
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
    pub fn load_in(data: &Path, id: &str) -> Self {
        let path = plugin_in(data, id);
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

    pub fn save_in(&self, data: &Path, id: &str) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialise the settings for {id}: {e}"))?;
        save_readable(&plugin_in(data, id), &text)
    }

    /// Whether the plugin has a settings file yet.
    pub fn exists_in(data: &Path, id: &str) -> bool {
        plugin_in(data, id).exists()
    }

    pub fn reset_in(data: &Path, id: &str) -> Result<(), String> {
        let path = plugin_in(data, id);
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

/// One step of a JSON pointer, with "~" and "/" escaped as RFC 6901 says,
/// so a monitor named "A/B" stays one step.
pub fn pointer_token(text: &str) -> String {
    text.replace('~', "~0").replace('/', "~1")
}

/// Sets the value a JSON pointer ("/monitors/PHL 323E7/mode") names,
/// making the objects on the way and replacing anything on the way that is
/// not an object.
pub fn set_pointer(target: &mut Value, pointer: &str, value: Value) {
    let tokens: Vec<String> = pointer
        .split('/')
        .skip(1)
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect();
    let Some((last, path)) = tokens.split_last() else {
        *target = value;
        return;
    };
    let mut at = target;
    for token in path {
        if !at.is_object() {
            *at = empty_object();
        }
        at = at
            .as_object_mut()
            .expect("just made an object")
            .entry(token.clone())
            .or_insert_with(empty_object);
    }
    if !at.is_object() {
        *at = empty_object();
    }
    at.as_object_mut()
        .expect("just made an object")
        .insert(last.clone(), value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_sets_a_nested_value_and_keeps_its_neighbours() {
        let mut settings = serde_json::json!({ "idle_opacity": 1.0, "monitors": {} });
        let key = format!("/monitors/{}/mode", pointer_token("PHL 323E7"));
        set_pointer(&mut settings, &key, Value::String("Off".to_string()));
        let other = format!("/monitors/{}/mode", pointer_token("A/B~C"));
        set_pointer(&mut settings, &other, Value::String("Dim".to_string()));
        assert_eq!(settings["monitors"]["PHL 323E7"]["mode"], "Off");
        assert_eq!(settings["monitors"]["A/B~C"]["mode"], "Dim");
        assert_eq!(settings["idle_opacity"], 1.0);
        assert_eq!(settings.pointer(&key), Some(&Value::String("Off".into())));
        // Something in the way that is not an object is replaced.
        let mut broken = serde_json::json!({ "monitors": 3 });
        set_pointer(&mut broken, &key, Value::String("Cover".to_string()));
        assert_eq!(broken["monitors"]["PHL 323E7"]["mode"], "Cover");
    }

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
        assert_eq!(config.mode, ThemeChoice::System);
        assert_eq!(config.palette_hotkey, DEFAULT_PALETTE_HOTKEY);

        let written = serde_json::to_string(&config).unwrap();
        assert!(!written.contains("modules"));
    }

    #[test]
    fn a_config_without_modules_migrates_nothing() {
        let mut raw: Value = serde_json::from_str(r#"{"mode":"dark"}"#).unwrap();
        assert!(take_legacy_plugins(&mut raw).is_empty());
        let config: Config = serde_json::from_value(raw).unwrap();
        assert_eq!(config.mode, ThemeChoice::Dark);
    }

    #[test]
    fn missing_files_fall_back_to_defaults() {
        let config: Config = serde_json::from_str("{}").unwrap();
        assert!(!config.start_with_windows);
        assert_eq!(config.theme, ThemeName::Classic);
        assert_eq!(config.mode, ThemeChoice::System);
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
        assert_eq!(config.mode, ThemeChoice::Light);

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
    fn file_search_is_on_with_the_dot_prefix_until_the_file_says_otherwise() {
        let config = SearchConfig::default();
        assert!(config.everything);
        assert_eq!(config.prefixes["files"], ".");
        let (config, _) =
            Config::parse(r#"{"search":{"web_url":"https://x/?q={query}"}}"#).unwrap();
        assert!(
            config.search.everything,
            "an older file keeps file search on"
        );
        let (config, _) = Config::parse(r#"{"search":{"everything":false}}"#).unwrap();
        assert!(!config.search.everything);
    }

    #[test]
    fn ask_ai_opens_claude_until_the_file_says_otherwise() {
        let (config, _) = Config::parse(r#"{"search":{"everything":true}}"#).unwrap();
        assert_eq!(config.search.ask_ai_site, "Claude");
        assert!(config.search.ask_ai_custom.is_empty());
        let (config, _) = Config::parse(
            r#"{"search":{"ask_ai_site":"Custom URL","ask_ai_custom":"https://x/?q={query}"}}"#,
        )
        .unwrap();
        assert_eq!(config.search.ask_ai_site, "Custom URL");
        assert_eq!(config.search.ask_ai_custom, "https://x/?q={query}");
    }

    #[test]
    fn the_windows_index_fallback_is_on_until_the_file_says_otherwise() {
        assert!(SearchConfig::default().file_search_fallback);
        let (config, _) = Config::parse(r#"{"search":{"everything":true}}"#).unwrap();
        assert!(
            config.search.file_search_fallback,
            "an older file gets the fallback"
        );
        let (config, _) = Config::parse(r#"{"search":{"file_search_fallback":false}}"#).unwrap();
        assert!(!config.search.file_search_fallback);
        assert!(config.search.everything);
    }

    #[test]
    fn the_automatic_update_check_is_on_until_the_file_says_otherwise() {
        let (config, _) = Config::parse("{}").unwrap();
        assert_eq!(config.updates, UpdateConfig::default());
        assert!(config.updates.check_daily);
        assert!(config.updates.download_ahead, "on by default");
        assert!(config.updates.hidden_version.is_empty());
        // A config from before 0.20.60 has neither key and loads.
        let (config, _) =
            Config::parse(r#"{"updates":{"check_daily":true,"last_check":5,"latest":"v0.20.59"}}"#)
                .unwrap();
        assert!(config.updates.download_ahead);
        assert_eq!(config.updates.latest, "v0.20.59");
        let (config, _) =
            Config::parse(r#"{"updates":{"download_ahead":false,"hidden_version":"v0.21.0"}}"#)
                .unwrap();
        assert!(!config.updates.download_ahead);
        assert_eq!(config.updates.hidden_version, "v0.21.0");
        let (config, _) =
            Config::parse(r#"{"updates":{"check_daily":false,"last_check":5}}"#).unwrap();
        assert!(!config.updates.check_daily);
        assert_eq!(config.updates.last_check, 5);
        assert_eq!(config.updates.latest, "");
    }

    #[test]
    fn the_old_theme_becomes_the_mode_and_the_theme_becomes_classic() {
        for (old, mode) in [
            ("system", ThemeChoice::System),
            ("light", ThemeChoice::Light),
            ("dark", ThemeChoice::Dark),
        ] {
            let (config, _) = Config::parse(&format!(r#"{{"theme":"{old}"}}"#)).unwrap();
            assert_eq!(config.theme, ThemeName::Classic, "{old}");
            assert_eq!(config.mode, mode, "{old}");
        }
        let (config, _) = Config::parse(r#"{"theme":"paper","mode":"dark"}"#).unwrap();
        assert_eq!(config.theme, ThemeName::Paper);
        assert_eq!(config.mode, ThemeChoice::Dark);
        // A mode already there wins over an old theme word.
        let (config, _) = Config::parse(r#"{"theme":"light","mode":"dark"}"#).unwrap();
        assert_eq!(config.theme, ThemeName::Classic);
        assert_eq!(config.mode, ThemeChoice::Dark);
    }

    #[test]
    fn unknown_theme_and_mode_words_fall_back_to_classic_and_system() {
        let (config, _) = Config::parse(r#"{"theme":"aero","mode":"dim"}"#).unwrap();
        assert_eq!(config.theme, ThemeName::Classic);
        assert_eq!(config.mode, ThemeChoice::System);
        let (config, _) = Config::parse(r#"{"theme":"blue"}"#).unwrap();
        assert_eq!(config.theme, ThemeName::Classic);
    }

    #[test]
    fn theme_and_mode_are_saved_with_their_words() {
        let config = Config {
            theme: ThemeName::HighContrast,
            mode: ThemeChoice::Light,
            ..Config::default()
        };
        let text = serde_json::to_string(&config).unwrap();
        assert!(text.contains(r#""theme":"high-contrast""#), "{text}");
        assert!(text.contains(r#""mode":"light""#), "{text}");
        let (again, _) = Config::parse(&text).unwrap();
        assert_eq!(again.theme, ThemeName::HighContrast);
        assert_eq!(again.mode, ThemeChoice::Light);
        for name in ThemeName::ALL {
            assert_eq!(ThemeName::from_key(name.key()), Some(name));
            assert!(!name.label().is_empty() && !name.blurb().is_empty());
        }
    }

    #[test]
    fn a_field_of_the_wrong_type_fails_the_file_instead_of_defaulting() {
        assert!(Config::parse(r#"{"theme":5}"#).is_err());
        assert!(Config::parse(r#"{"mode":true}"#).is_err());
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

    /// A data folder of its own under the temp folder, removed afterwards.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("wincraft-config-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("plugins")).unwrap();
            Scratch(dir)
        }

        fn config(&self) -> PathBuf {
            config_in(&self.0)
        }

        fn plugin(&self, id: &str) -> PathBuf {
            plugin_in(&self.0, id)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn with_no_files_everything_is_the_default_and_nothing_is_written() {
        let data = Scratch::new("missing");
        let config = Config::load_in(&data.0);
        assert_eq!(config.mode, ThemeChoice::System);
        assert_eq!(config.palette_hotkey, DEFAULT_PALETTE_HOTKEY);
        let plugin = PluginConfig::load_in(&data.0, "screen_dimmer");
        assert!(plugin.enabled && plugin.hotkeys.is_empty());
        assert!(!data.config().exists());
        assert!(!data.plugin("screen_dimmer").exists());
    }

    #[test]
    fn files_that_start_with_a_byte_order_mark_are_read() {
        let data = Scratch::new("bom");
        fs::write(data.config(), "\u{feff}{\"theme\":\"dark\"}").unwrap();
        fs::write(
            data.plugin("window_namer"),
            "\u{feff}{\"enabled\":false,\"hotkeys\":{\"a\":\"Win+Alt+Z\"}}",
        )
        .unwrap();
        assert_eq!(Config::load_in(&data.0).mode, ThemeChoice::Dark);
        let plugin = PluginConfig::load_in(&data.0, "window_namer");
        assert!(!plugin.enabled);
        assert_eq!(plugin.hotkeys["a"], "Win+Alt+Z");
    }

    #[test]
    fn a_corrupt_config_gives_defaults_and_is_never_overwritten() {
        let data = Scratch::new("corrupt-config");
        fs::write(data.config(), "{ \"theme\": \"dark\", oops").unwrap();
        let config = Config::load_in(&data.0);
        assert_eq!(config.mode, ThemeChoice::System);
        assert!(config.save_in(&data.0).is_err(), "saving is refused");
        assert_eq!(
            fs::read_to_string(data.config()).unwrap(),
            "{ \"theme\": \"dark\", oops"
        );
    }

    #[test]
    fn a_corrupt_plugin_file_gives_defaults_and_is_never_overwritten() {
        let data = Scratch::new("corrupt-plugin");
        fs::write(data.plugin("layout_keeper"), "[1, 2").unwrap();
        let plugin = PluginConfig::load_in(&data.0, "layout_keeper");
        assert!(plugin.enabled);
        assert!(plugin.save_in(&data.0, "layout_keeper").is_err());
        assert_eq!(
            fs::read_to_string(data.plugin("layout_keeper")).unwrap(),
            "[1, 2"
        );
        // Another plugin's file is not affected.
        assert!(plugin.save_in(&data.0, "window_namer").is_ok());
    }

    #[test]
    fn a_field_of_the_wrong_type_in_the_file_also_counts_as_unreadable() {
        let data = Scratch::new("wrong-type");
        fs::write(data.plugin("screen_dimmer"), "{\"enabled\": \"yes\"}").unwrap();
        assert!(PluginConfig::load_in(&data.0, "screen_dimmer").enabled);
        assert!(PluginConfig::default()
            .save_in(&data.0, "screen_dimmer")
            .is_err());
    }

    #[test]
    fn unknown_keys_inside_a_plugins_settings_survive_a_save() {
        let data = Scratch::new("unknown-settings");
        fs::write(
            data.plugin("screen_dimmer"),
            r#"{"enabled":true,"settings":{"idle_opacity":0.5,"from_a_newer_version":{"x":[1,2]}}}"#,
        )
        .unwrap();
        let plugin = PluginConfig::load_in(&data.0, "screen_dimmer");
        plugin.save_in(&data.0, "screen_dimmer").unwrap();
        let again = PluginConfig::load_in(&data.0, "screen_dimmer");
        assert_eq!(again.settings["idle_opacity"], 0.5);
        assert_eq!(again.settings["from_a_newer_version"]["x"][1], 2);
    }

    #[test]
    fn unknown_top_level_keys_are_ignored_on_reading_and_dropped_on_saving() {
        let data = Scratch::new("unknown-top");
        fs::write(
            data.config(),
            r#"{"theme":"light","from_a_newer_version":true}"#,
        )
        .unwrap();
        let config = Config::load_in(&data.0);
        assert_eq!(config.mode, ThemeChoice::Light);
        config.save_in(&data.0).unwrap();
        let text = fs::read_to_string(data.config()).unwrap();
        assert!(text.contains("\"light\"") && !text.contains("from_a_newer_version"));
    }

    #[test]
    fn what_the_tips_did_survives_a_save_with_unknown_tip_ids_kept() {
        let data = Scratch::new("tips");
        assert!(Config::load_in(&data.0).tips.show, "tips are on by default");
        fs::write(
            data.config(),
            r#"{"tips":{"show":false,"state":{"bookmark-ctrl-b":{"shown":2},"from-a-newer-version":{"shown":1,"done":true}}}}"#,
        )
        .unwrap();
        let config = Config::load_in(&data.0);
        assert!(!config.tips.show);
        config.save_in(&data.0).unwrap();
        let again = Config::load_in(&data.0);
        assert_eq!(again.tips, config.tips);
        assert_eq!(again.tips.state["bookmark-ctrl-b"].shown, 2);
        assert!(again.tips.state["from-a-newer-version"].done);
    }

    #[test]
    fn reset_deletes_only_that_plugins_file_and_a_missing_file_is_fine() {
        let data = Scratch::new("reset");
        fs::write(data.plugin("a"), "{}").unwrap();
        fs::write(data.plugin("b"), "{}").unwrap();
        PluginConfig::reset_in(&data.0, "a").unwrap();
        assert!(!data.plugin("a").exists() && data.plugin("b").exists());
        PluginConfig::reset_in(&data.0, "a").unwrap();
        assert_eq!(PluginConfig::load_in(&data.0, "a").settings, empty_object());
    }

    #[test]
    fn an_old_config_is_split_into_plugin_files_once() {
        let data = Scratch::new("migrate");
        fs::write(data.config(), V02_CONFIG).unwrap();
        let config = Config::load_in(&data.0);
        assert!(config.start_with_windows);

        let dimmer = PluginConfig::load_in(&data.0, "screen_dimmer");
        assert!(dimmer.enabled);
        assert_eq!(dimmer.settings["hover_opacity"], 0.4);
        assert!(!PluginConfig::load_in(&data.0, "shortcut_detector").enabled);
        let rewritten = fs::read_to_string(data.config()).unwrap();
        assert!(!rewritten.contains("modules"), "{rewritten}");
        assert!(
            rewritten.contains("\"start_with_windows\": true"),
            "{rewritten}"
        );

        // A second start finds nothing to migrate and changes nothing.
        let files = |data: &Scratch| {
            ["screen_dimmer", "shortcut_detector"]
                .map(|id| fs::read_to_string(data.plugin(id)).unwrap())
        };
        let before = files(&data);
        Config::load_in(&data.0);
        assert_eq!(files(&data), before);
        assert_eq!(fs::read_to_string(data.config()).unwrap(), rewritten);
    }

    #[test]
    fn migration_never_replaces_a_plugin_file_that_already_exists() {
        let data = Scratch::new("migrate-existing");
        fs::write(data.config(), V02_CONFIG).unwrap();
        fs::write(
            data.plugin("screen_dimmer"),
            r#"{"settings":{"hover_opacity":0.9}}"#,
        )
        .unwrap();
        Config::load_in(&data.0);
        let dimmer = PluginConfig::load_in(&data.0, "screen_dimmer");
        assert_eq!(dimmer.settings["hover_opacity"], 0.9);
        assert!(data.plugin("shortcut_detector").exists());
    }

    #[test]
    fn an_old_plugin_entry_that_cannot_be_read_does_not_stop_the_others_migrating() {
        let data = Scratch::new("migrate-partial");
        fs::write(
            data.config(),
            r#"{"modules":{"bad":{"enabled":"maybe"},"good":{"enabled":false}}}"#,
        )
        .unwrap();
        Config::load_in(&data.0);
        assert!(!data.plugin("bad").exists());
        assert!(!PluginConfig::load_in(&data.0, "good").enabled);
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

    #[test]
    fn an_old_config_has_no_palette_spots() {
        let config: Config = serde_json::from_str(r#"{ "theme": "Classic" }"#).unwrap();
        assert!(config.palette_spots.is_empty());
    }

    #[test]
    fn palette_spots_survive_a_round_trip() {
        let mut config = Config::default();
        config
            .palette_spots
            .insert("DISPLAY2".to_string(), PaletteSpot { dx: 40, dy: -3 });
        let text = serde_json::to_string(&config).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.palette_spots, config.palette_spots);
    }

    const WORK: WorkArea = (1920, 0, 3840, 1040);

    #[test]
    fn a_spot_is_kept_relative_to_its_monitor() {
        let spot = PaletteSpot::from_screen(WORK, 2100, 200);
        assert_eq!(spot, PaletteSpot { dx: 180, dy: 200 });
        assert_eq!(spot.to_screen(WORK, (800, 520)), (2100, 200));
        // The same spot on a monitor that sits somewhere else.
        assert_eq!(spot.to_screen((0, 0, 1920, 1040), (800, 520)), (180, 200));
    }

    #[test]
    fn a_spot_is_clamped_so_the_whole_panel_shows() {
        let far = PaletteSpot { dx: 5000, dy: 5000 };
        assert_eq!(far.to_screen(WORK, (800, 520)), (3040, 520));
        let before = PaletteSpot { dx: -300, dy: -40 };
        assert_eq!(before.to_screen(WORK, (800, 520)), (1920, 0));
        // A panel larger than the work area is pinned to its top left.
        assert_eq!(far.to_screen((0, 0, 600, 400), (800, 520)), (0, 0));
    }

    #[test]
    fn the_default_spot_is_centred_and_18_percent_down() {
        assert_eq!(centred_spot((0, 0, 1920, 1000), 800), (560, 180));
        assert_eq!(centred_spot(WORK, 800), (2480, 187));
    }
}
