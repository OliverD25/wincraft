//! The "Claude Code hook" row on ClaudeDeck's settings page. Its button
//! adds WinCraft's own hook (`wincraft.exe --claude-hook`) to the user's
//! Claude Code settings, or takes it out, and only on that click
//! (Principle 1). Without the hook ClaudeDeck cannot know that a chat asks
//! the user something or needs approval.

use serde_json::Value;

use crate::core::claude_hook::settings::{self, Status};
use crate::core::host;
use crate::core::traits::{DynamicField, FieldKind};

pub(super) const KEY: &str = "/claude_code_hook";
pub(super) const GROUP: &str = "Claude Code";
const CONNECT: &str = "Connect to Claude Code";
const DISCONNECT: &str = "Disconnect from Claude Code";
const HELP: &str = "ClaudeDeck shows Asks you and Needs approval only when Claude Code tells WinCraft. Connect adds WinCraft's hook to your Claude Code settings.json and keeps a backup of the file next to it. Disconnect takes the hook out again. A chat shows the mark from its next question on.";

fn status() -> Status {
    settings::settings_path().map_or(Status::NotConnected, |path| settings::status_of(&path))
}

pub(super) fn field() -> DynamicField {
    let status = status();
    DynamicField {
        key: KEY.to_string(),
        label: "Claude Code hook".to_string(),
        help: format!("{}. {HELP}", status.line()),
        kind: FieldKind::Button(if status.connected() {
            DISCONNECT
        } else {
            CONNECT
        }),
        value: Value::Null,
    }
}

/// The button was clicked: connect, or disconnect when connected.
pub(super) fn press() {
    let Some(path) = settings::settings_path() else {
        host::notify(
            "ClaudeDeck",
            "WinCraft did not find your Claude Code settings: USERPROFILE is not set.",
        );
        return;
    };
    let connect = !settings::status_of(&path).connected();
    let result = if connect {
        std::env::current_exe()
            .map_err(|err| format!("WinCraft cannot find its own program ({err})"))
            .and_then(|exe| settings::connect_file(&path, &exe))
    } else {
        settings::disconnect_file(&path)
    };
    match &result {
        Ok(changed) => log::info!(
            "deck: Claude Code hook {} ({}changed {})",
            if connect { "connected" } else { "disconnected" },
            if *changed { "" } else { "un" },
            path.display()
        ),
        Err(err) => log::warn!(
            "deck: Claude Code hook not changed in {}: {err}",
            path.display()
        ),
    }
    host::notify("ClaudeDeck", &settings::balloon(connect, &result, &path));
    host::plugin_changed();
}
