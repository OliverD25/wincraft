use super::*;

/// Which programs LayoutKeeper looks after: every app ("*") or a list, less
/// the excluded ones, and never WinCraft itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Watch {
    pub(super) all: bool,
    pub(super) programs: Vec<String>,
    excluded: Vec<String>,
}

impl Watch {
    pub(super) fn new(programs: &str, excluded: &str) -> Self {
        let programs = parse_programs(programs);
        Self {
            all: programs.iter().any(|program| program == "*"),
            programs: programs
                .into_iter()
                .filter(|program| program != "*")
                .collect(),
            excluded: parse_programs(excluded),
        }
    }

    pub(super) fn covers(&self, exe: &str) -> bool {
        let exe = exe.to_lowercase();
        windows::is_app(&exe)
            && !self.excluded.contains(&exe)
            && (self.all || self.programs.contains(&exe))
    }
}

/// "any app", "any app except telegram.exe", "chrome.exe, msedge.exe".
impl std::fmt::Display for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.all {
            f.write_str("any app")?;
            if !self.excluded.is_empty() {
                write!(f, " except {}", self.excluded.join(", "))?;
            }
            return Ok(());
        }
        f.write_str(&self.programs.join(", "))
    }
}

/// Settings from before 0.8 name only Chrome, which was the default then,
/// not a choice: that one value becomes "every app". A list the user wrote
/// is kept.
pub(super) fn migrate_programs(settings: &mut serde_json::Value) -> bool {
    let old = settings.get("programs").and_then(|value| value.as_str());
    if old.map(str::trim) != Some(OLD_DEFAULT_PROGRAMS) {
        return false;
    }
    settings["programs"] = serde_json::Value::from(DEFAULT_PROGRAMS);
    true
}

/// A number setting that still holds the old default was never a choice, so
/// it takes the new default. Any other number is kept.
pub(super) fn migrate_default(
    settings: &mut serde_json::Value,
    key: &str,
    old: u64,
    new: u64,
) -> bool {
    if settings.get(key).and_then(|value| value.as_f64()) != Some(old as f64) {
        return false;
    }
    settings[key] = serde_json::Value::from(new);
    true
}

pub(super) fn parse_programs(text: &str) -> Vec<String> {
    let mut programs: Vec<String> = Vec::new();
    for name in text.split(',').map(|name| name.trim().to_lowercase()) {
        if !name.is_empty() && !programs.contains(&name) {
            programs.push(name);
        }
    }
    programs
}

pub(super) fn number(settings: &serde_json::Value, key: &str, min: u64, max: u64) -> Option<u64> {
    let value = settings.get(key)?.as_f64()?;
    Some((value.round().max(0.0) as u64).clamp(min, max))
}

/// How many windows the taskbar button's app has, on every desktop.
pub(super) fn flag(settings: &serde_json::Value, key: &str) -> bool {
    settings
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

pub(super) fn win_alt(vk: u32) -> Hotkey {
    Hotkey {
        modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
        vk,
    }
}
