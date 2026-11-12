//! How a Claude Code chat's last turn ended, and the last thing Claude
//! said in it, for ClaudeDeck's statuses and its line of text. Read from
//! the same transcript as the background jobs (`claude_jobs`), in the same
//! pass, so nothing is read twice.
//!
//! What was learned from real transcripts (2026-11-12):
//! - Records of the chat itself start with `{"parentUuid"` and have
//!   `"isSidechain":false` near the start. A user record names its type
//!   near the start; an assistant record names it near the end.
//! - The user's stop writes a user record whose text starts with
//!   `[Request interrupted by user` (16 of about 150 recent transcripts
//!   ended that way).
//! - A failed request writes an assistant record with
//!   `"isApiErrorMessage":true` and the model `<synthetic>`; its text is
//!   the error the app shows.
//! - A question card is an assistant `tool_use` named `AskUserQuestion`.
//!
//! Only Claude's visible text and a tool's name are kept: never a tool's
//! input (except the question of a question card, which the chat shows),
//! a tool's result, or a file. The text is cleaned by `words::clean`.

use std::collections::VecDeque;

use serde_json::Value;

mod words;
pub use words::{first_sentence, last_question};

/// The full text is cut to this many characters.
pub const TEXT_CAP: usize = 600;
/// How many of the chat's last records are kept, newest last.
const KEEP: usize = 8;
/// Where a user record names its type, and an assistant record.
const USER_TYPE_WITHIN: usize = 600;
const ASSISTANT_TYPE_WITHIN: usize = 3000;
/// A record longer than this is not kept as the last text: such a record
/// is a tool's big input, not words to show.
const MOST_TEXT_LINE: usize = 512 * 1024;

const INTERRUPTED: &str = "[Request interrupted by user";
const QUESTION_TOOL: &str = "AskUserQuestion";

/// How the chat's last record leaves the turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum End {
    /// Nothing read yet, or no record of a known shape.
    #[default]
    Unknown,
    /// The user sent a message; Claude has not answered yet.
    UserMessage,
    /// Claude answered with text and called no tool.
    Replied,
    /// Claude called a tool whose result has not come back.
    ToolCalled,
    /// A tool's result came back; Claude goes on.
    ToolDone,
    /// The user stopped the turn.
    Interrupted,
    /// The request failed (an API error).
    Failed,
}

/// What ClaudeDeck needs of the last turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    pub end: End,
    /// The last text Claude showed, cleaned, at most `TEXT_CAP` characters.
    pub text: Option<String>,
    /// The question of a question card that waits for its answer.
    pub question: Option<String>,
    /// The tool Claude called last, while its result has not come back.
    pub tool: Option<String>,
}

/// The chat's last records, kept while the transcript is read. Only the
/// raw lines are kept; they are read as JSON when the turn is asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tail {
    recent: VecDeque<String>,
    /// The newest assistant record that holds text.
    text_line: Option<String>,
}

impl Tail {
    /// Looks at one line of the transcript.
    pub fn line(&mut self, line: &str) {
        let Some(kind) = kind_of(line) else {
            return;
        };
        if kind == Kind::Assistant
            && line.len() <= MOST_TEXT_LINE
            && line.contains("\"type\":\"text\"")
        {
            self.text_line = Some(line.to_string());
        }
        if self.recent.len() == KEEP {
            self.recent.pop_front();
        }
        let kept = if line.len() <= MOST_TEXT_LINE {
            line.to_string()
        } else {
            stand_in(kind, line)
        };
        self.recent.push_back(kept);
    }

    pub fn reset(&mut self) {
        *self = Tail::default();
    }

    /// The last turn, as far as the kept lines tell.
    pub fn turn(&self) -> Turn {
        let mut turn = Turn::default();
        let last = self
            .recent
            .iter()
            .rev()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|record| !is_meta(record));
        if let Some(record) = &last {
            turn.end = end_of(record);
            if turn.end == End::ToolCalled {
                let tools = tool_names(record);
                turn.tool = tools.last().cloned();
                if tools.iter().any(|name| name == QUESTION_TOOL) {
                    turn.question = question_of(record);
                }
            }
        }
        let shown = if turn.end == End::Failed {
            // The error is the text the chat shows last.
            last
        } else {
            self.text_line
                .as_deref()
                .and_then(|line| serde_json::from_str::<Value>(line).ok())
        };
        turn.text = shown
            .as_ref()
            .and_then(text_of)
            .map(|text| words::cap(&words::clean(&text), TEXT_CAP))
            .filter(|text| !text.is_empty());
        turn
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    User,
    Assistant,
}

/// A record of the chat itself (not a side chain) from the user or from
/// Claude, told from the raw line without reading it as JSON.
fn kind_of(line: &str) -> Option<Kind> {
    if !line.starts_with("{\"parentUuid\"") {
        return None;
    }
    let head = prefix(line, USER_TYPE_WITHIN);
    if !head.contains("\"isSidechain\":false") {
        return None;
    }
    if head.contains("\"type\":\"user\"") {
        return Some(Kind::User);
    }
    if head.contains("\"type\":\"") && !head.contains("\"message\":") {
        // An early type that is not "user": system, attachment and others.
        return None;
    }
    suffix(line, ASSISTANT_TYPE_WITHIN)
        .contains("\"type\":\"assistant\"")
        .then_some(Kind::Assistant)
}

/// A short record in place of a very long one, so the tail stays small:
/// a long user record is a tool's result, a long assistant record a tool
/// called with a big input (only its name is kept).
fn stand_in(kind: Kind, line: &str) -> String {
    let record = match kind {
        Kind::User => serde_json::json!({
            "type": "user",
            "message": {"content": [{"type": "tool_result"}]}
        }),
        Kind::Assistant => {
            let key = "\"type\":\"tool_use\"";
            let name = line
                .find(key)
                .map(|at| prefix(&line[at..], 4096))
                .and_then(|near| {
                    let from = near.find("\"name\":\"")? + "\"name\":\"".len();
                    let rest = &near[from..];
                    Some(rest[..rest.find('"')?].to_string())
                })
                .unwrap_or_default();
            serde_json::json!({
                "type": "assistant",
                "message": {"content": [{"type": "tool_use", "name": name}]}
            })
        }
    };
    record.to_string()
}

fn prefix(line: &str, most: usize) -> &str {
    let mut end = line.len().min(most);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

fn suffix(line: &str, most: usize) -> &str {
    let mut start = line.len().saturating_sub(most);
    while !line.is_char_boundary(start) {
        start += 1;
    }
    &line[start..]
}

fn is_meta(record: &Value) -> bool {
    record.get("isMeta").and_then(Value::as_bool) == Some(true)
        || record.get("isSidechain").and_then(Value::as_bool) == Some(true)
}

fn blocks(record: &Value) -> Vec<&Value> {
    match record.get("message").and_then(|m| m.get("content")) {
        Some(Value::Array(list)) => list.iter().collect(),
        Some(text @ Value::String(_)) => vec![text],
        _ => Vec::new(),
    }
}

fn block_type(block: &Value) -> Option<&str> {
    block.get("type").and_then(Value::as_str)
}

fn block_text(block: &Value) -> Option<&str> {
    match block {
        Value::String(text) => Some(text.as_str()),
        _ if block_type(block) == Some("text") => block.get("text").and_then(Value::as_str),
        _ => None,
    }
}

/// The text of a record: its text blocks joined, or its plain text.
fn text_of(record: &Value) -> Option<String> {
    let parts: Vec<&str> = blocks(record).into_iter().filter_map(block_text).collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn tool_names(record: &Value) -> Vec<String> {
    blocks(record)
        .into_iter()
        .filter(|block| block_type(block) == Some("tool_use"))
        .filter_map(|block| block.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// The question a question card shows: the `question` of its first item.
fn question_of(record: &Value) -> Option<String> {
    let block = blocks(record).into_iter().find(|block| {
        block_type(block) == Some("tool_use")
            && block.get("name").and_then(Value::as_str) == Some(QUESTION_TOOL)
    })?;
    let question = block
        .get("input")?
        .get("questions")?
        .as_array()?
        .first()?
        .get("question")?
        .as_str()?;
    let question = words::cap(&words::clean(question), TEXT_CAP);
    (!question.is_empty()).then_some(question)
}

fn end_of(record: &Value) -> End {
    let all = blocks(record);
    match record.get("type").and_then(Value::as_str) {
        Some("assistant") => {
            if record.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true) {
                End::Failed
            } else if all
                .iter()
                .any(|block| block_type(block) == Some("tool_use"))
            {
                End::ToolCalled
            } else {
                End::Replied
            }
        }
        Some("user") => {
            let interrupted = all.iter().any(|block| {
                block_text(block).is_some_and(|text| text.trim_start().starts_with(INTERRUPTED))
            });
            if interrupted {
                End::Interrupted
            } else if !all.is_empty()
                && all
                    .iter()
                    .all(|block| block_type(block) == Some("tool_result"))
            {
                End::ToolDone
            } else {
                End::UserMessage
            }
        }
        _ => End::Unknown,
    }
}

#[cfg(test)]
mod tests;
