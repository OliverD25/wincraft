use super::words::{cap, clean, HIDDEN};
use super::*;

// Records are written by hand, in the key order the app writes them: a
// user record names its type near the start, an assistant record near the
// end, and a tool call starts with its type, id and name.

fn user(content: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"promptId":"x","type":"user","message":{{"role":"user","content":{content}}},"uuid":"u"}}"#
    )
}

fn assistant(content: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"message":{{"model":"m","type":"message","role":"assistant","content":{content}}},"requestId":"r","type":"assistant","uuid":"a"}}"#
    )
}

fn json_text(text: &str) -> String {
    Value::String(text.to_string()).to_string()
}

fn said(text: &str) -> String {
    assistant(&format!(
        r#"[{{"type":"text","text":{}}}]"#,
        json_text(text)
    ))
}

fn tool(name: &str) -> String {
    assistant(&format!(
        r#"[{{"type":"tool_use","id":"t1","name":"{name}","input":{{"command":"secret-command --flag"}}}}]"#
    ))
}

fn result() -> String {
    user(r#"[{"type":"tool_result","tool_use_id":"t1","content":"file contents here"}]"#)
}

fn typed(text: &str) -> String {
    user(&json_text(text))
}

fn system() -> String {
    r#"{"parentUuid":"p","isSidechain":false,"type":"system","subtype":"stop_hook_summary","content":"x"}"#.to_string()
}

fn tail(lines: &[String]) -> Tail {
    let mut tail = Tail::default();
    for line in lines {
        tail.line(line);
    }
    tail
}

#[test]
fn a_reply_ends_the_turn_and_its_text_is_kept() {
    let turn = tail(&[
        typed("Fix the build"),
        tool("Bash"),
        result(),
        said("The build passes now. Two tests were fixed."),
        system(),
    ])
    .turn();
    assert_eq!(turn.end, End::Replied);
    assert_eq!(
        turn.text.as_deref(),
        Some("The build passes now. Two tests were fixed.")
    );
    assert_eq!(turn.tool, None);
    assert_eq!(turn.question, None);
}

#[test]
fn a_tool_that_runs_names_only_the_tool_never_its_input() {
    let turn = tail(&[said("Let me look."), tool("Bash")]).turn();
    assert_eq!(turn.end, End::ToolCalled);
    assert_eq!(turn.tool.as_deref(), Some("Bash"));
    assert_eq!(turn.text.as_deref(), Some("Let me look."));
    assert!(!format!("{turn:?}").contains("secret-command"));

    let turn = tail(&[tool("Read"), result()]).turn();
    assert_eq!(turn.end, End::ToolDone);
    assert_eq!(turn.tool, None);
    assert!(!format!("{turn:?}").contains("file contents"));
}

#[test]
fn the_users_stop_and_a_failed_request_are_told_apart() {
    let stopped = tail(&[
        said("Working on it."),
        user(r#"[{"type":"text","text":"[Request interrupted by user]"}]"#),
    ])
    .turn();
    assert_eq!(stopped.end, End::Interrupted);
    let for_tool = tail(&[
        tool("Bash"),
        typed("[Request interrupted by user for tool use]"),
    ])
    .turn();
    assert_eq!(for_tool.end, End::Interrupted);

    let error = said("API Error: 500 server error. Try again.").replace(
        r#""requestId":"r""#,
        r#""requestId":"r","isApiErrorMessage":true"#,
    );
    let failed = tail(&[said("Earlier text."), error]).turn();
    assert_eq!(failed.end, End::Failed);
    assert_eq!(
        failed.text.as_deref(),
        Some("API Error: 500 server error. Try again.")
    );

    // A new message after the stop is a new turn.
    let again = tail(&[typed("[Request interrupted by user]"), typed("Go on")]).turn();
    assert_eq!(again.end, End::UserMessage);
}

#[test]
fn a_question_card_gives_its_question() {
    let card = assistant(
        r#"[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{"questions":[{"question":"Which **layout** should stay?","options":[]}]}}]"#,
    );
    let turn = tail(&[said("Two options."), card.clone()]).turn();
    assert_eq!(turn.end, End::ToolCalled);
    assert_eq!(turn.tool.as_deref(), Some("AskUserQuestion"));
    assert_eq!(turn.question.as_deref(), Some("Which layout should stay?"));
    // Answered, the question is gone.
    let turn = tail(&[card, result()]).turn();
    assert_eq!(turn.question, None);
}

#[test]
fn side_chains_meta_records_and_strangers_are_skipped() {
    let side = said("From a subagent.").replace(r#""isSidechain":false"#, r#""isSidechain":true"#);
    let meta = typed("caveat").replace(r#""promptId":"x""#, r#""promptId":"x","isMeta":true"#);
    let turn = tail(&[
        said("The real reply."),
        side,
        meta,
        system(),
        "not json at all".to_string(),
        r#"{"type":"custom-title","customTitle":"x"}"#.to_string(),
    ])
    .turn();
    assert_eq!(turn.end, End::Replied);
    assert_eq!(turn.text.as_deref(), Some("The real reply."));
    assert_eq!(Tail::default().turn(), Turn::default());
}

#[test]
fn a_very_long_record_is_kept_short() {
    let big = "x".repeat(MOST_TEXT_LINE + 10);
    let huge_result = user(&format!(
        r#"[{{"type":"tool_result","tool_use_id":"t","content":"{big}"}}]"#
    ));
    let mut kept = tail(&[said("Reading."), huge_result]);
    assert_eq!(kept.turn().end, End::ToolDone);
    assert!(kept.recent.iter().all(|line| line.len() < 1000));
    let huge_call = assistant(&format!(
        r#"[{{"type":"tool_use","id":"t","name":"Write","input":{{"content":"{big}"}}}}]"#
    ));
    kept.line(&huge_call);
    let turn = kept.turn();
    assert_eq!(turn.end, End::ToolCalled);
    assert_eq!(turn.tool.as_deref(), Some("Write"));
    assert_eq!(turn.text.as_deref(), Some("Reading."));
    kept.reset();
    assert_eq!(kept, Tail::default());
}

#[test]
fn text_loses_code_marks_and_secrets() {
    let text = "## Done\n\nThe **fix** is in `main.rs`. See [the PR](https://x.y/1).\n\n```\nrm -rf /\n```\n| a | b |\n- set API_KEY=abc123 and token sk-abcdefghijklmnop\n- id 3f9a1c2e4b5d6f708192a3b4c5d6e7f8 and `cargo test --bin wincraft deck::rows::a_long_name`";
    let cleaned = clean(text);
    assert_eq!(
        cleaned,
        format!(
            "Done\nThe fix is in main.rs. See the PR.\nset API_KEY={HIDDEN} and token {HIDDEN} id {HIDDEN} and \u{2026}"
        )
    );
    assert!(!cleaned.contains("rm -rf"));
}

#[test]
fn ordinary_words_and_paths_stay() {
    let text = "WinCraft 0.20.99 is out; see E:\\code\\WinCraft and version_2_of_the_plan.";
    assert_eq!(clean(text), text);
}

#[test]
fn sentences_and_questions_are_found() {
    assert_eq!(first_sentence("Done. Two more.\nNext"), "Done. Two more.");
    assert_eq!(first_sentence("It works now. Two more."), "It works now.");
    assert_eq!(first_sentence("No end here"), "No end here");
    assert_eq!(first_sentence(""), "");
    assert_eq!(
        last_question("Two ways. Should it be a row? Or a fan? Tell me.\nThanks."),
        Some("Or a fan?".to_string())
    );
    assert_eq!(last_question("No question here."), None);
    assert_eq!(cap("abcdef", 4), "abc\u{2026}");
    assert_eq!(cap("abc", 4), "abc");
}

fn shell(name: &str, input: &str) -> String {
    let record = assistant(&format!(
        r#"[{{"type":"tool_use","id":"t9","name":"{name}","input":{input}}}]"#
    ));
    // The app writes the time after the record's type.
    record.replacen(
        r#""uuid":"a"}"#,
        r#""uuid":"a","timestamp":"2026-11-12T10:00:00.000Z"}"#,
        1,
    )
}

#[test]
fn a_foreground_ssh_command_is_kept_with_its_host_and_time() {
    let turn = tail(&[shell(
        "Bash",
        r#"{"command":"ssh -o BatchMode=yes rdehome2 uptime"}"#,
    )])
    .turn();
    let ssh = turn.ssh.expect("an ssh call");
    assert_eq!(ssh.hosts, vec!["rdehome2".to_string()]);
    assert_eq!(ssh.command, "ssh -o BatchMode=yes rdehome2 uptime");
    assert_eq!(ssh.at, Some(1_794_477_600_000));
}

#[test]
fn a_background_ssh_job_or_another_command_keeps_nothing() {
    let background = shell(
        "Bash",
        r#"{"command":"ssh rdehome2 uptime","run_in_background":true}"#,
    );
    assert_eq!(tail(&[background]).turn().ssh, None);
    let plain = shell("Bash", r#"{"command":"echo ssh rdehome2"}"#);
    let turn = tail(&[plain]).turn();
    assert_eq!(turn.ssh, None);
    assert!(!format!("{turn:?}").contains("echo"));
    let read = shell("Read", r#"{"command":"ssh rdehome2"}"#);
    assert_eq!(tail(&[read]).turn().ssh, None);
    // The result came back: the chat no longer waits on it.
    let done = shell("Bash", r#"{"command":"ssh rdehome2 uptime"}"#);
    assert_eq!(tail(&[done, result()]).turn().ssh, None);
}

#[test]
fn a_long_ssh_command_is_cut_to_show() {
    let long = format!("ssh rdehome2 'echo {}'", "word ".repeat(60));
    let input = serde_json::json!({ "command": long }).to_string();
    let ssh = tail(&[shell("Bash", &input)])
        .turn()
        .ssh
        .expect("an ssh call");
    assert!(ssh.command.chars().count() <= COMMAND_CAP);
}
