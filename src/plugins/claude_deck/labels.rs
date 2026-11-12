//! The short texts of a row and the header, in plain words.

use crate::core::claude_sessions;

/// "6 cold chats", "1 cold chat".
pub fn cold_label(chats: usize) -> String {
    match chats {
        1 => "1 cold chat".to_string(),
        n => format!("{n} cold chats"),
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
