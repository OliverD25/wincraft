//! The turn that just ended, read from the end of the chat's transcript,
//! and whether its last reply asks the user something.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

/// Only this much of the transcript's end is read: a turn is far shorter,
/// and a long chat's transcript can be many megabytes.
const TAIL_BYTES: u64 = 400_000;

/// Whether a `user` entry is the user's own message, which starts a turn.
/// Tool results, Stop hook feedback, meta and system entries come in the
/// middle of a turn and do not start one.
pub fn is_user_prompt(entry: &Value, content: Option<&Value>) -> bool {
    if entry.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    if let Some(origin) = entry.get("origin").and_then(Value::as_object) {
        return origin.get("kind").and_then(Value::as_str) == Some("human");
    }
    if entry.get("isMeta").and_then(Value::as_bool) == Some(true)
        || entry.get("promptSource").and_then(Value::as_str) == Some("system")
    {
        return false;
    }
    !content.and_then(Value::as_array).is_some_and(|blocks| {
        blocks
            .iter()
            .all(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
    })
}

/// The last reply text of the turn in these transcript lines, and the
/// question of the app's next-step box (a `wrap_up` tool call's `ask`).
pub fn turn_of(lines: &str) -> (String, String) {
    let (mut text, mut ask) = (String::new(), String::new());
    for line in lines.lines().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let content = entry.get("message").and_then(|m| m.get("content"));
        match entry.get("type").and_then(Value::as_str) {
            Some("user") if is_user_prompt(&entry, content) => break,
            Some("assistant") => {}
            _ => continue,
        }
        let Some(blocks) = content.and_then(Value::as_array) else {
            continue;
        };
        for block in blocks.iter().rev() {
            let kind = block.get("type").and_then(Value::as_str);
            if kind == Some("text") && text.is_empty() {
                text = block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            let name = block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if kind == Some("tool_use") && name.ends_with("wrap_up") && ask.is_empty() {
                ask = block
                    .get("input")
                    .and_then(|input| input.get("ask"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
        }
    }
    (text, ask)
}

/// `turn_of` the transcript file's end. Empty when it cannot be read.
pub fn last_turn(path: &Path) -> (String, String) {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Default::default();
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file
        .seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)))
        .is_err()
    {
        return Default::default();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Default::default();
    }
    turn_of(&String::from_utf8_lossy(&bytes))
}

/// Whether the last paragraph of a reply ends with a question mark. The
/// paragraph is the text after the last blank line; code blocks are left
/// out, and so is markdown emphasis or a closing bracket or quote after
/// the mark ("**Which one?**").
pub fn last_paragraph_asks(text: &str) -> bool {
    let mut in_code = false;
    let mut blank = false;
    let mut paragraph: Vec<&str> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if trimmed.is_empty() {
            blank = true;
            continue;
        }
        if std::mem::take(&mut blank) {
            paragraph.clear();
        }
        paragraph.push(trimmed);
    }
    let Some(last) = paragraph.last() else {
        return false;
    };
    last.trim_end_matches(['*', '_', ')', '"', '\'', '\u{201d}', '\u{2019}', ' '])
        .ends_with('?')
}

/// Whether a finished turn leaves the user a question: the app's next-step
/// box asks one, or the last paragraph of the reply ends with "?".
pub fn reply_asks(text: &str, ask: &str) -> bool {
    !ask.trim().is_empty() || last_paragraph_asks(text)
}
