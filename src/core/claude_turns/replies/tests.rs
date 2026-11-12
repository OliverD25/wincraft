use serde_json::Value;

use super::*;

// Records in the key order the app writes them, with a timestamp.

fn stamp(minute: u32) -> String {
    format!("2026-11-12T10:{minute:02}:00.000Z")
}

fn user(content: &str, minute: u32) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"promptId":"x","type":"user","message":{{"role":"user","content":{content}}},"uuid":"u","timestamp":"{}"}}"#,
        stamp(minute)
    )
}

fn assistant(content: &str, minute: u32) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"message":{{"model":"m","type":"message","role":"assistant","content":{content}}},"requestId":"r","type":"assistant","uuid":"a","timestamp":"{}"}}"#,
        stamp(minute)
    )
}

fn said(text: &str, minute: u32) -> String {
    let text = Value::String(text.to_string()).to_string();
    assistant(&format!(r#"[{{"type":"text","text":{text}}}]"#), minute)
}

fn tool(minute: u32) -> String {
    assistant(
        r#"[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]"#,
        minute,
    )
}

fn result(minute: u32) -> String {
    user(
        r#"[{"type":"tool_result","tool_use_id":"t1","content":"x"}]"#,
        minute,
    )
}

fn typed(text: &str, minute: u32) -> String {
    user(&Value::String(text.to_string()).to_string(), minute)
}

fn read(lines: &[String]) -> Vec<(u32, String)> {
    let mut replies = Replies::default();
    for line in lines {
        replies.line(line);
    }
    let base = crate::core::claude_jobs::timestamp_ms(&format!("\"timestamp\":\"{}\"", stamp(0)))
        .expect("a time");
    replies
        .all()
        .into_iter()
        .map(|reply| (((reply.at - base) / 60_000) as u32, reply.text))
        .collect()
}

#[test]
fn each_turn_that_ends_with_text_gives_one_reply_with_its_time() {
    let got = read(&[
        typed("Fix the build", 1),
        said("Let me look.", 2),
        tool(3),
        result(4),
        said("Fixed.\nThe build passes.", 5),
        typed("Thanks. Now the docs", 6),
        tool(7),
        result(8),
        said("The docs are done.", 9),
    ]);
    assert_eq!(
        got,
        vec![
            (5, "Fixed.\nThe build passes.".to_string()),
            (9, "The docs are done.".to_string()),
        ],
        "words before a tool call are not a reply; the last text is one even with no user record after it"
    );
}

#[test]
fn a_turn_that_ends_with_a_tool_gives_no_reply() {
    let got = read(&[typed("Run it", 1), said("Running.", 2), tool(3)]);
    assert!(got.is_empty(), "{got:?}");
    let got = read(&[typed("Run it", 1), tool(2), result(3)]);
    assert!(got.is_empty(), "a tool-only turn: {got:?}");
}

#[test]
fn text_records_in_a_row_are_one_reply_and_a_failed_request_is_none() {
    let got = read(&[
        typed("Go", 1),
        said("First part.", 2),
        said("Second part.", 3),
        typed("Next", 4),
    ]);
    assert_eq!(got, vec![(3, "First part.\n\nSecond part.".to_string())]);
    let failed = r#"{"parentUuid":"p","isSidechain":false,"message":{"model":"<synthetic>","role":"assistant","content":[{"type":"text","text":"API Error: 500"}]},"isApiErrorMessage":true,"type":"assistant","uuid":"a"}"#;
    let got = read(&[typed("Go", 1), failed.to_string()]);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn side_chains_are_skipped_and_only_the_newest_replies_are_kept() {
    let side =
        said("A subagent's words.", 2).replace("\"isSidechain\":false", "\"isSidechain\":true");
    let got = read(&[typed("Go", 1), side]);
    assert!(got.is_empty());
    let mut lines = Vec::new();
    for turn in 0..30 {
        lines.push(typed("Go", turn));
        lines.push(said(&format!("Reply {turn}"), turn));
    }
    lines.push(typed("Go", 59));
    let got = read(&lines);
    assert_eq!(got.len(), KEEP_REPLIES);
    assert_eq!(got.last().map(|(_, text)| text.as_str()), Some("Reply 29"));
}

#[test]
fn a_long_reply_is_cut() {
    let long = "word ".repeat(2000);
    let got = read(&[typed("Go", 1), said(&long, 2)]);
    assert_eq!(got[0].1.chars().count(), REPLY_CAP);
    assert!(got[0].1.ends_with('\u{2026}'));
}
