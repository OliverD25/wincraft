//! The short texts of a row and the header, in plain words.

use crate::core::claude_sessions;

use super::model::Mark;

/// "6 cold chats", "1 cold chat".
pub fn cold_label(chats: usize) -> String {
    match chats {
        1 => "1 cold chat".to_string(),
        n => format!("{n} cold chats"),
    }
}

/// "2 hidden chats", "1 hidden chat".
pub fn hidden_label(chats: usize) -> String {
    match chats {
        1 => "1 hidden chat".to_string(),
        n => format!("{n} hidden chats"),
    }
}

/// "2 jobs", "1 job".
pub fn jobs_label(jobs: usize) -> String {
    match jobs {
        1 => "1 job".to_string(),
        n => format!("{n} jobs"),
    }
}

/// "3 forks", "1 fork".
pub fn forks_label(forks: usize) -> String {
    match forks {
        1 => "1 fork".to_string(),
        n => format!("{n} forks"),
    }
}

/// "3 chats", "1 chat".
pub fn chats_label(chats: usize) -> String {
    match chats {
        1 => "1 chat".to_string(),
        n => format!("{n} chats"),
    }
}

/// "1 – 9 days", or "3 days" when all are as old.
pub fn days_label((least, most): (u64, u64)) -> String {
    if least == most && least == 1 {
        "1 day".to_string()
    } else if least == most {
        format!("{least} days")
    } else {
        format!("{least} \u{2013} {most} days")
    }
}

/// The time in the status at the right edge: "40 s", "4 min", "2 h", or a
/// dash when WinCraft cannot tell.
pub fn time_label(since: Option<u64>, now: u64) -> String {
    match since {
        Some(at) => claude_sessions::duration_text(now.saturating_sub(at)),
        None => "\u{2013}".to_string(),
    }
}

/// The model name without the "claude-" every model starts with.
pub fn model_label(model: &str) -> &str {
    model.strip_prefix("claude-").unwrap_or(model)
}

/// The status in words for the hover panel: "asks you", "done, not seen".
pub fn status_words(mark: Mark) -> &'static str {
    match mark {
        Mark::NeedsApproval => "needs approval",
        Mark::AsksYou => "asks you",
        Mark::DoneNotSeen => "done, not seen",
        Mark::Working => "working",
        Mark::Background => "background",
        Mark::Idle(_) => "idle",
        Mark::Stopped => "stopped",
        Mark::Failed => "failed",
        Mark::Unknown => "unknown",
    }
}

/// The start of a folded chat's line when a fork is the most urgent.
pub fn fork_line(mark: Mark) -> &'static str {
    match mark {
        Mark::NeedsApproval => "A fork needs approval",
        Mark::AsksYou => "A fork asks you",
        Mark::DoneNotSeen => "A fork is done",
        Mark::Working => "A fork works",
        Mark::Background => "A fork runs jobs",
        Mark::Idle(_) => "A fork is idle",
        Mark::Stopped => "A fork was stopped",
        Mark::Failed => "A fork failed",
        Mark::Unknown => "A fork",
    }
}

/// What a tool does, in plain words: as the chat does it ("Running a
/// command") and as a thing to allow ("run a command"). Only the tool's
/// name is used, never its input.
fn tool_words(name: &str) -> (String, String) {
    let pair = |doing: &str, to: &str| (doing.to_string(), to.to_string());
    match name {
        "Bash" | "PowerShell" => pair("Running a command", "run a command"),
        "Read" | "NotebookRead" => pair("Reading a file", "read a file"),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => pair("Editing a file", "edit a file"),
        "Grep" | "Glob" => pair("Searching the files", "search the files"),
        "Agent" | "Task" => pair("Running a subagent", "start a subagent"),
        "WebFetch" | "WebSearch" => pair("Looking on the web", "look on the web"),
        "TodoWrite" => pair("Updating its plan", "update its plan"),
        "Skill" => pair("Using a skill", "use a skill"),
        "AskUserQuestion" => pair("Asking you", "ask you"),
        _ if name.starts_with("mcp__") => pair("Using a connector", "use a connector"),
        _ if !name.is_empty()
            && name.len() <= 32
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            (format!("Using {name}"), format!("use {name}"))
        }
        _ => pair("Using a tool", "use a tool"),
    }
}

/// "Running a command".
pub fn tool_doing(name: &str) -> String {
    tool_words(name).0
}

/// "Waits for your OK to run a command".
pub fn approval_line(tool: Option<&str>) -> String {
    match tool {
        Some(name) => format!("Waits for your OK to {}", tool_words(name).1),
        None => "Waits for your OK".to_string(),
    }
}

pub const THINKING: &str = "Thinking";
pub const STOPPED_LINE: &str = "You stopped it";
pub const FAILED_LINE: &str = "The last request failed";
pub const UNKNOWN_LINE: &str = "WinCraft cannot read this chat's state";
pub const LAST: &str = "Last: ";
pub const NO_WINDOW: &str = "No open window";
