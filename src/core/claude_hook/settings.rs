//! ClaudeDeck's "Connect to Claude Code" button: adds WinCraft's hook
//! (`wincraft.exe --claude-hook`) to the user's Claude Code
//! `settings.json`, or takes it out again. Only on the user's click
//! (Principle 1). Every other key and hook stays exactly as it was, in its
//! order; the file is parsed, edited and written through a temporary file,
//! and never written when it cannot be read as JSON.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::json::Json;
use super::FLAG;
use crate::core::config;

/// The events the hook needs, in the order they are added.
pub const EVENTS: [&str; 6] = [
    "PreToolUse",
    "PermissionRequest",
    "Notification",
    "PostToolUse",
    "UserPromptSubmit",
    "Stop",
];
/// Claude Code stops a hook that takes longer, in seconds.
const TIMEOUT_S: u64 = 5;
/// The creator's own hook script, which marks chats the old way.
const OTHER_HOOK: &str = "claude-attention";

/// Where Claude Code keeps the user's settings: `CLAUDE_CONFIG_DIR` when
/// set, else `%USERPROFILE%\.claude`.
pub fn settings_path() -> Option<PathBuf> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude"))
        })?;
    Some(dir.join("settings.json"))
}

/// The command Claude Code runs. Forward slashes work in both shells
/// Claude Code may use on Windows (Git Bash and cmd).
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\" {FLAG}", exe.to_string_lossy().replace('\\', "/"))
}

/// Whether a hook command is WinCraft's own hook.
pub fn is_ours(command: &str) -> bool {
    let lower = command.to_lowercase();
    lower.contains(FLAG) && lower.contains("wincraft")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connected,
    NotConnected,
    /// The creator's own hook script marks chats, and WinCraft's is off.
    Other,
    /// Both hooks are in.
    ConnectedAndOther,
    /// The file is there but is not JSON.
    Unreadable,
}

impl Status {
    pub fn line(self) -> &'static str {
        match self {
            Status::Connected => "Connected to Claude Code",
            Status::NotConnected => "Not connected",
            Status::Other => "Another hook already marks chats (claude-attention.py)",
            Status::ConnectedAndOther => {
                "Connected to Claude Code. Another hook also marks chats (claude-attention.py)"
            }
            Status::Unreadable => "Not connected: settings.json is not valid JSON",
        }
    }

    pub fn connected(self) -> bool {
        matches!(self, Status::Connected | Status::ConnectedAndOther)
    }
}

fn hook_lists(root: &Json) -> impl Iterator<Item = (&str, &Json)> {
    let hooks = match root.get("hooks") {
        Some(Json::Object(entries)) => entries.as_slice(),
        _ => &[],
    };
    hooks.iter().map(|(event, groups)| (event.as_str(), groups))
}

/// Every hook command of one event's list of groups.
fn commands(groups: &Json) -> Vec<&str> {
    let Json::Array(groups) = groups else {
        return Vec::new();
    };
    groups
        .iter()
        .filter_map(|group| match group.get("hooks") {
            Some(Json::Array(hooks)) => Some(hooks),
            _ => None,
        })
        .flatten()
        .filter_map(|hook| hook.get("command").and_then(Json::as_str))
        .collect()
}

/// The status of settings.json's text; None for a file that is not there.
pub fn status(text: Option<&str>) -> Status {
    let Some(text) = text else {
        return Status::NotConnected;
    };
    let Ok(root) = serde_json::from_str::<Json>(config::strip_bom(text)) else {
        return Status::Unreadable;
    };
    let lists: Vec<(&str, Vec<&str>)> = hook_lists(&root)
        .map(|(event, groups)| (event, commands(groups)))
        .collect();
    let connected = EVENTS.iter().all(|wanted| {
        lists
            .iter()
            .any(|(event, commands)| event == wanted && commands.iter().any(|c| is_ours(c)))
    });
    let other = lists
        .iter()
        .flat_map(|(_, commands)| commands)
        .any(|command| command.contains(OTHER_HOOK));
    match (connected, other) {
        (true, true) => Status::ConnectedAndOther,
        (true, false) => Status::Connected,
        (false, true) => Status::Other,
        (false, false) => Status::NotConnected,
    }
}

fn parse(text: &str) -> Result<Json, String> {
    let root: Json = serde_json::from_str(config::strip_bom(text))
        .map_err(|err| format!("it is not valid JSON ({err})"))?;
    match root {
        Json::Object(_) => Ok(root),
        _ => Err("it is not a JSON object".to_string()),
    }
}

fn write_out(root: &Json, newline: bool) -> Result<String, String> {
    let mut text = serde_json::to_string_pretty(root).map_err(|err| err.to_string())?;
    if newline {
        text.push('\n');
    }
    Ok(text)
}

fn our_group(event: &str, command: &str) -> Json {
    let hook = vec![
        ("type".to_string(), Json::text("command")),
        ("command".to_string(), Json::text(command)),
        ("timeout".to_string(), Json::Leaf(Value::from(TIMEOUT_S))),
    ];
    let mut group = Vec::new();
    // The question card is the only PreToolUse the hook needs, so Claude
    // Code does not start WinCraft before every other tool.
    if event == "PreToolUse" {
        group.push(("matcher".to_string(), Json::text("AskUserQuestion")));
    }
    group.push(("hooks".to_string(), Json::Array(vec![Json::Object(hook)])));
    Json::Object(group)
}

/// settings.json's text with WinCraft's hook in every event; None when it
/// is in already with this command. `text` None is a file not there yet.
pub fn connect(text: Option<&str>, command: &str) -> Result<Option<String>, String> {
    let mut root = match text {
        Some(text) => parse(text)?,
        None => Json::object(),
    };
    let mut changed = false;
    let hooks = root.entry("hooks", Json::object).ok_or("no object")?;
    if !matches!(hooks, Json::Object(_)) {
        return Err("its \"hooks\" is not an object".to_string());
    }
    for event in EVENTS {
        let Some(Json::Array(groups)) = hooks.entry(event, || Json::Array(Vec::new())) else {
            return Err(format!("its \"hooks\".\"{event}\" is not a list"));
        };
        let mut found = false;
        for group in groups.iter_mut() {
            let Some(Json::Array(list)) = group.get_mut("hooks") else {
                continue;
            };
            for hook in list.iter_mut() {
                let Some(old) = hook.get("command").and_then(Json::as_str) else {
                    continue;
                };
                if !is_ours(old) {
                    continue;
                }
                found = true;
                if old != command {
                    if let Some(slot) = hook.get_mut("command") {
                        *slot = Json::text(command);
                        changed = true;
                    }
                }
            }
        }
        if !found {
            groups.push(our_group(event, command));
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    write_out(&root, text.is_none_or(|text| text.ends_with('\n'))).map(Some)
}

/// settings.json's text without WinCraft's hook, and without the groups
/// and events that held only it; None when it was not in.
pub fn disconnect(text: &str) -> Result<Option<String>, String> {
    let mut root = parse(text)?;
    let mut changed = false;
    let Some(Json::Object(events)) = root.get_mut("hooks") else {
        return Ok(None);
    };
    let mut gone: Vec<String> = Vec::new();
    for (event, groups) in events.iter_mut() {
        let Json::Array(groups) = groups else {
            continue;
        };
        let before = groups.len();
        let mut emptied = Vec::new();
        for (at, group) in groups.iter_mut().enumerate() {
            let Some(Json::Array(list)) = group.get_mut("hooks") else {
                continue;
            };
            let count = list.len();
            list.retain(|hook| {
                !hook
                    .get("command")
                    .and_then(Json::as_str)
                    .is_some_and(is_ours)
            });
            if list.len() != count {
                changed = true;
                if list.is_empty() {
                    emptied.push(at);
                }
            }
        }
        for at in emptied.into_iter().rev() {
            groups.remove(at);
        }
        if groups.len() != before && groups.is_empty() {
            gone.push(event.clone());
        }
    }
    if !changed {
        return Ok(None);
    }
    events.retain(|(event, _)| !gone.contains(event));
    if events.is_empty() {
        root.remove("hooks");
    }
    write_out(&root, text.ends_with('\n')).map(Some)
}

fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("it cannot be read ({err})")),
    }
}

/// The status of the settings file at `path`.
pub fn status_of(path: &Path) -> Status {
    match read(path) {
        Ok(text) => status(text.as_deref()),
        Err(_) => Status::Unreadable,
    }
}

/// The backup next to the settings file, made once before the first
/// change and then kept.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".wincraft-backup");
    path.with_file_name(name)
}

fn save(path: &Path, text: &str, existed: bool) -> Result<(), String> {
    let backup = backup_path(path);
    if existed && !backup.exists() {
        std::fs::copy(path, &backup).map_err(|err| format!("its backup cannot be made ({err})"))?;
    }
    config::write_atomic(path, text)
}

/// Connects the settings file at `path` to the exe at `exe`. Returns
/// whether the file changed.
pub fn connect_file(path: &Path, exe: &Path) -> Result<bool, String> {
    let text = read(path)?;
    let new = connect(text.as_deref(), &command_for(exe))?;
    match new {
        Some(new) => save(path, &new, text.is_some()).map(|()| true),
        None => Ok(false),
    }
}

/// Takes WinCraft's hook out of the settings file at `path`. Returns
/// whether the file changed.
pub fn disconnect_file(path: &Path) -> Result<bool, String> {
    let Some(text) = read(path)? else {
        return Ok(false);
    };
    let new = disconnect(&text)?;
    match new {
        Some(new) => save(path, &new, true).map(|()| true),
        None => Ok(false),
    }
}

/// The balloon after a click.
pub fn balloon(connected: bool, result: &Result<bool, String>, path: &Path) -> String {
    match (result, connected) {
        (Ok(_), true) => {
            "WinCraft connected to Claude Code. New chats show when they ask you.".to_string()
        }
        (Ok(_), false) => "WinCraft disconnected from Claude Code.".to_string(),
        (Err(err), _) => format!("WinCraft did not change {}: {err}.", path.display()),
    }
}

#[cfg(test)]
mod tests;
