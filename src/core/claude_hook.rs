//! `wincraft.exe --claude-hook`: WinCraft's own Claude Code hook. Claude
//! Code runs it for the events ClaudeDeck's "Connect to Claude Code" button
//! adds to its `settings.json`, and sends one JSON object on stdin. The
//! command decides whether the chat now waits for the user (a question, or
//! a permission prompt), or was answered, and tells the running WinCraft
//! with a `session:<id>` target (see `claude_waits`).
//!
//! It must stay quiet and fast: Claude Code waits for it, and the output
//! or exit code of some hooks changes what Claude does. So it prints
//! nothing, always exits 0, and does nothing when no WinCraft runs. It
//! never starts WinCraft and never writes WinCraft's log.
//!
//! One small file per chat under `%LOCALAPPDATA%\WinCraft\claude-hook\`
//! remembers why the chat was marked, so that only the right event clears
//! the mark.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::claude_waits::{self, Ask, Kind};
use crate::core::config;

mod json;
pub mod settings;
mod turn;

#[cfg(test)]
use turn::last_paragraph_asks;
pub use turn::{last_turn, reply_asks};

/// The flag that runs the hook.
pub const FLAG: &str = "--claude-hook";
/// Claude Code's input is small; anything bigger is not ours.
const MOST_INPUT_BYTES: u64 = 4 << 20;
/// The tool Claude uses to ask the user a question with options.
const ASK_TOOL: &str = "AskUserQuestion";
/// The `by` of a mark set when a turn ended with a question.
const STOP_ASKS: &str = "Stop (reply asks)";

/// The fields of the hook's input this command reads. Every other field is
/// skipped.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Input {
    pub hook_event_name: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub notification_type: Option<String>,
    pub last_assistant_message: Option<String>,
}

impl Input {
    pub fn event(&self) -> &str {
        self.hook_event_name.as_deref().unwrap_or_default()
    }

    fn tool(&self) -> &str {
        self.tool_name.as_deref().unwrap_or_default()
    }

    fn tool_id(&self) -> &str {
        self.tool_use_id.as_deref().unwrap_or_default()
    }
}

/// Why a chat was marked, saved until the mark is cleared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// The event that set the mark; a turn's end is `Stop (reply asks)`.
    pub by: String,
    /// The tool the mark waits for, if a tool set it.
    pub tool_name: Option<String>,
    pub tool_id: Option<String>,
    /// "question" or "approval".
    pub kind: String,
}

/// What one hook call does: clear the chat's mark, then maybe mark it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub answer: bool,
    pub wait: Option<Ask>,
}

/// Events that can mean the user answered.
pub fn answers(event: &str) -> bool {
    matches!(event, "PostToolUse" | "UserPromptSubmit" | "Stop")
}

/// What an event that is not an answer means: Some when the chat now waits.
/// Only a question card, a permission request and a permission prompt
/// count; an idle reminder or "waiting for your input" does not.
pub fn waits(input: &Input) -> Option<Ask> {
    match input.event() {
        "PreToolUse" if input.tool() == ASK_TOOL => Some(Ask::Question),
        "PermissionRequest" => Some(Ask::Approval),
        "Notification" if input.notification_type.as_deref() == Some("permission_prompt") => {
            Some(Ask::Approval)
        }
        _ => None,
    }
}

/// Whether this PostToolUse is for the tool the mark waits for: the same
/// call when both have an id, else the same tool name. A mark with no tool
/// is cleared by any tool.
pub fn same_tool(saved: &Saved, input: &Input) -> bool {
    match (saved.tool_id.as_deref(), saved.tool_name.as_deref()) {
        (Some(id), _) if !id.is_empty() && !input.tool_id().is_empty() => input.tool_id() == id,
        (_, Some(name)) if !name.is_empty() => input.tool() == name,
        _ => true,
    }
}

/// Whether an answering event clears this mark. The user's message and a
/// turn's end always do. A tool that finished does only when the mark
/// waits for that tool, and never when a turn's end set the mark: a
/// PostToolUse seconds after Stop is not the user's answer.
pub fn clears(input: &Input, saved: &Saved) -> bool {
    if input.event() != "PostToolUse" {
        return true;
    }
    !saved.by.starts_with("Stop") && same_tool(saved, input)
}

/// The tool a new mark waits for: this call's tool, or, for a call that
/// names none (a Notification), the one the chat's tool mark already has.
pub fn tool_of(input: &Input, saved: Option<&Saved>) -> (Option<String>, Option<String>) {
    if !input.tool().is_empty() {
        let id = (!input.tool_id().is_empty()).then(|| input.tool_id().to_string());
        return (Some(input.tool().to_string()), id);
    }
    match saved {
        Some(saved) if !saved.by.starts_with("Stop") => {
            (saved.tool_name.clone(), saved.tool_id.clone())
        }
        _ => (None, None),
    }
}

/// The decision table. `stop_asks` reads the transcript, so it runs only
/// for a Stop.
pub fn plan(input: &Input, saved: Option<&Saved>, stop_asks: impl FnOnce() -> bool) -> Plan {
    let event = input.event();
    if !answers(event) {
        return Plan {
            answer: false,
            wait: waits(input),
        };
    }
    Plan {
        answer: saved.is_some_and(|saved| clears(input, saved)),
        wait: (event == "Stop" && stop_asks()).then_some(Ask::Question),
    }
}

/// What is saved for a new mark.
pub fn saved_for(input: &Input, before: Option<&Saved>, ask: Ask) -> Saved {
    let stop = input.event() == "Stop";
    let (tool_name, tool_id) = if stop {
        (None, None)
    } else {
        tool_of(input, before)
    };
    Saved {
        by: if stop {
            STOP_ASKS.to_string()
        } else {
            input.event().to_string()
        },
        tool_name,
        tool_id,
        kind: match ask {
            Ask::Approval => "approval",
            Ask::Question => "question",
        }
        .to_string(),
    }
}

/// Whether the turn that just ended asks the user something.
pub fn stop_asks(input: &Input) -> bool {
    let (text, ask) = input
        .transcript_path
        .as_deref()
        .filter(|path| !path.is_empty())
        .map(|path| last_turn(Path::new(path)))
        .unwrap_or_default();
    let reply = input
        .last_assistant_message
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(&text);
    reply_asks(reply, &ask)
}

/// The chat's state file: only letters, digits, `-` and `_` of the id.
pub fn state_path(dir: &Path, session: &str) -> PathBuf {
    let safe: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let safe = if safe.is_empty() { "unknown" } else { &safe };
    dir.join(format!("{safe}.json"))
}

pub fn state_dir() -> PathBuf {
    config::data_dir().join("claude-hook")
}

fn read_saved(path: &Path) -> Option<Saved> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(config::strip_bom(&text)).ok()
}

/// One hook call, after the input was read. `send` hands a target to the
/// running WinCraft. Returns the plan it carried out.
pub fn handle(input: &Input, dir: &Path, send: &mut dyn FnMut(Kind, Ask, &str)) -> Plan {
    let session = input.session_id.as_deref().unwrap_or_default().trim();
    if session.is_empty() {
        return Plan::default();
    }
    let path = state_path(dir, session);
    let saved = read_saved(&path);
    let plan = plan(input, saved.as_ref(), || stop_asks(input));
    let target = format!("session:{session}");
    if plan.answer {
        let _ = std::fs::remove_file(&path);
        send(Kind::Answered, Ask::Question, &target);
    }
    if let Some(ask) = plan.wait {
        let before = if plan.answer { None } else { saved.as_ref() };
        let new = saved_for(input, before, ask);
        if let Ok(text) = serde_json::to_string(&new) {
            let _ = config::write_atomic(&path, &text);
        }
        send(Kind::Waiting, ask, &target);
    }
    plan
}

/// The whole command: reads stdin and tells the running WinCraft. Quiet in
/// every case.
pub fn run() {
    let mut text = String::new();
    if std::io::stdin()
        .take(MOST_INPUT_BYTES)
        .read_to_string(&mut text)
        .is_err()
    {
        return;
    }
    if !claude_waits::running() {
        return;
    }
    let Ok(input) = serde_json::from_str::<Input>(config::strip_bom(&text)) else {
        return;
    };
    handle(&input, &state_dir(), &mut |kind, ask, target| {
        let _ = claude_waits::run_as(kind, ask, target);
    });
}

#[cfg(test)]
mod tests;
