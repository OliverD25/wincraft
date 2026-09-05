use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use crate::core::wide;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub start_with_windows: bool,
    #[serde(default)]
    pub modules: BTreeMap<String, ModuleConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleConfig {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub hotkeys: BTreeMap<String, String>,
    #[serde(default = "empty_object")]
    pub settings: Value,
}

impl Default for ModuleConfig {
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

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                log::info!("no config at {} ({err}), using defaults", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str(&text) {
            Ok(config) => config,
            Err(err) => {
                log::warn!("config.json is not valid JSON ({err}), using defaults");
                Self::default()
            }
        }
    }

    /// Writes through a temporary file so a crash mid-write cannot leave the
    /// user with a half-written config that the next start refuses to read.
    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| format!("cannot serialise config: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;

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
            fs::write(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        Ok(())
    }

    pub fn module_mut(&mut self, id: &str) -> &mut ModuleConfig {
        self.modules.entry(id.to_string()).or_default()
    }
}

impl ModuleConfig {
    /// Fills in keys the file does not have yet, so every editable option shows
    /// up in config.json even when the module was added after the file was made.
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
