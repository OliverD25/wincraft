use super::turn::{is_user_prompt, turn_of};
use super::*;
use serde_json::Value;

fn input(json: &str) -> Input {
    serde_json::from_str(json).expect("test input")
}

fn saved(by: &str, tool: Option<&str>, id: Option<&str>) -> Saved {
    Saved {
        by: by.into(),
        tool_name: tool.map(str::to_string),
        tool_id: id.map(str::to_string),
        kind: "question".into(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wincraft-hook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

#[test]
fn only_questions_and_permission_prompts_wait() {
    let ask = input(r#"{"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion"}"#);
    assert_eq!(waits(&ask), Some(Ask::Question));
    let bash = input(r#"{"hook_event_name":"PreToolUse","tool_name":"Bash"}"#);
    assert_eq!(waits(&bash), None);
    let request = input(r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash"}"#);
    assert_eq!(waits(&request), Some(Ask::Approval));
    let prompt =
        input(r#"{"hook_event_name":"Notification","notification_type":"permission_prompt"}"#);
    assert_eq!(waits(&prompt), Some(Ask::Approval));
    for other in [
        r#"{"hook_event_name":"Notification","notification_type":"idle_prompt"}"#,
        r#"{"hook_event_name":"Notification","message":"Claude is waiting for your input"}"#,
        r#"{"hook_event_name":"SessionStart"}"#,
        r#"{}"#,
    ] {
        assert_eq!(waits(&input(other)), None, "{other}");
    }
}

#[test]
fn unknown_fields_and_nulls_are_skipped() {
    let parsed = input(
        r#"{"hook_event_name":"PostToolUse","tool_name":null,"tool_input":{"a":[1,2]},"cwd":"C:\\x"}"#,
    );
    assert_eq!(parsed.event(), "PostToolUse");
    assert_eq!(parsed.tool_name, None);
}

#[test]
fn the_decision_table() {
    let never = || panic!("the transcript is read only at Stop");
    let ask = input(r#"{"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion"}"#);
    assert_eq!(
        plan(&ask, None, never),
        Plan {
            answer: false,
            wait: Some(Ask::Question)
        }
    );
    let prompt = input(r#"{"hook_event_name":"UserPromptSubmit"}"#);
    let mark = saved("PreToolUse", Some("AskUserQuestion"), None);
    assert_eq!(plan(&prompt, None, never), Plan::default());
    assert_eq!(
        plan(&prompt, Some(&mark), never),
        Plan {
            answer: true,
            wait: None
        }
    );
    let stop = input(r#"{"hook_event_name":"Stop"}"#);
    assert_eq!(
        plan(&stop, Some(&mark), || true),
        Plan {
            answer: true,
            wait: Some(Ask::Question)
        }
    );
    assert_eq!(plan(&stop, None, || false), Plan::default());
}

#[test]
fn a_tool_clears_only_its_own_mark() {
    let by_id = saved("PermissionRequest", Some("Bash"), Some("toolu_1"));
    let same =
        input(r#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_use_id":"toolu_1"}"#);
    let other_call =
        input(r#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_use_id":"toolu_2"}"#);
    let other_tool = input(r#"{"hook_event_name":"PostToolUse","tool_name":"Read"}"#);
    assert!(clears(&same, &by_id));
    assert!(!clears(&other_call, &by_id));
    // No id on the call: the tool's name decides.
    let no_id = input(r#"{"hook_event_name":"PostToolUse","tool_name":"Bash"}"#);
    assert!(clears(&no_id, &by_id));
    assert!(!clears(&other_tool, &by_id));
    // A mark with no tool is cleared by any tool.
    assert!(clears(&other_tool, &saved("PreToolUse", None, None)));
}

#[test]
fn a_tool_never_clears_a_mark_set_at_a_turns_end() {
    let at_stop = saved(STOP_ASKS, None, None);
    let post = input(r#"{"hook_event_name":"PostToolUse","tool_name":"Bash"}"#);
    assert!(!clears(&post, &at_stop));
    assert!(clears(
        &input(r#"{"hook_event_name":"UserPromptSubmit"}"#),
        &at_stop
    ));
    assert!(clears(&input(r#"{"hook_event_name":"Stop"}"#), &at_stop));
}

#[test]
fn a_notification_keeps_the_tool_of_the_mark_before_it() {
    let request = saved("PermissionRequest", Some("Bash"), Some("toolu_9"));
    let note =
        input(r#"{"hook_event_name":"Notification","notification_type":"permission_prompt"}"#);
    assert_eq!(
        tool_of(&note, Some(&request)),
        (Some("Bash".into()), Some("toolu_9".into()))
    );
    assert_eq!(
        tool_of(&note, Some(&saved(STOP_ASKS, None, None))),
        (None, None)
    );
    let new = saved_for(&note, Some(&request), Ask::Approval);
    assert_eq!(new.by, "Notification");
    assert_eq!(new.kind, "approval");
    assert_eq!(new.tool_id.as_deref(), Some("toolu_9"));
    let stop = saved_for(&input(r#"{"hook_event_name":"Stop"}"#), None, Ask::Question);
    assert_eq!(stop, saved(STOP_ASKS, None, None));
}

#[test]
fn the_state_file_name_keeps_only_safe_characters() {
    let dir = Path::new("x");
    assert_eq!(state_path(dir, "1b2c-3d_4"), dir.join("1b2c-3d_4.json"));
    assert_eq!(state_path(dir, r"..\..\evil"), dir.join("evil.json"));
    assert_eq!(state_path(dir, "///"), dir.join("unknown.json"));
}

#[test]
fn a_question_card_marks_and_its_answer_clears() {
    let dir = scratch("card");
    let mut sent = Vec::new();
    let mut send = |kind: Kind, ask: Ask, target: &str| sent.push((kind, ask, target.to_string()));
    let ask = input(
        r#"{"hook_event_name":"PreToolUse","session_id":"abc","tool_name":"AskUserQuestion","tool_use_id":"t1"}"#,
    );
    handle(&ask, &dir, &mut send);
    let other = input(
        r#"{"hook_event_name":"PostToolUse","session_id":"abc","tool_name":"Read","tool_use_id":"t0"}"#,
    );
    handle(&other, &dir, &mut send);
    let done = input(
        r#"{"hook_event_name":"PostToolUse","session_id":"abc","tool_name":"AskUserQuestion","tool_use_id":"t1"}"#,
    );
    handle(&done, &dir, &mut send);
    // Nothing is marked any more, so a second answer sends nothing.
    handle(&done, &dir, &mut send);
    assert_eq!(
        sent,
        vec![
            (Kind::Waiting, Ask::Question, "session:abc".to_string()),
            (Kind::Answered, Ask::Question, "session:abc".to_string()),
        ]
    );
    assert!(!state_path(&dir, "abc").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_call_without_a_session_does_nothing() {
    let dir = scratch("nosession");
    let mut count = 0;
    let request = input(r#"{"hook_event_name":"PermissionRequest"}"#);
    handle(&request, &dir, &mut |_, _, _| count += 1);
    assert_eq!(count, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_turn_that_ends_with_a_question_marks_the_chat() {
    let dir = scratch("stop");
    let transcript = dir.join("t.jsonl");
    std::fs::write(&transcript, TURN_WITH_FEEDBACK).expect("transcript");
    let stop = Input {
        hook_event_name: Some("Stop".into()),
        session_id: Some("s1".into()),
        transcript_path: Some(transcript.to_string_lossy().into_owned()),
        ..Input::default()
    };
    let mut sent = Vec::new();
    handle(&stop, &dir, &mut |kind, ask, _: &str| {
        sent.push((kind, ask))
    });
    assert_eq!(sent, vec![(Kind::Waiting, Ask::Question)]);
    let saved = read_saved(&state_path(&dir, "s1")).expect("saved");
    assert_eq!(saved.by, STOP_ASKS);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A turn that asked, then got Stop hook feedback and a tool result, and
/// ended with a short reply that does not ask.
const TURN_WITH_FEEDBACK: &str = concat!(
    r#"{"type":"user","message":{"role":"user","content":"Old prompt"}}"#,
    "\n",
    r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Old reply?"}]}}"#,
    "\n",
    r#"{"type":"user","message":{"role":"user","content":"Fix the build"}}"#,
    "\n",
    r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done."},{"type":"tool_use","name":"mcp__ccd_turn__wrap_up","input":{"recap":"r","ask":"Push it now?"}}]}}"#,
    "\n",
    r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Stop hook feedback: save the task"}}"#,
    "\n",
    r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{}}]}}"#,
    "\n",
    r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#,
    "\n",
    r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Saved the task."}]}}"#,
    "\n",
);

#[test]
fn the_last_turn_starts_at_the_users_own_message() {
    let (text, ask) = turn_of(TURN_WITH_FEEDBACK);
    assert_eq!(text, "Saved the task.");
    assert_eq!(ask, "Push it now?");
    // The turn before had no wrap_up: its question is not this turn's.
    let (text, ask) = turn_of(concat!(
        r#"{"type":"user","message":{"content":"Q"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"wrap_up","input":{"ask":"Old?"}}]}}"#,
        "\n",
        r#"{"type":"user","origin":{"kind":"human"},"message":{"content":"New"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Fine."}]}}"#,
    ));
    assert_eq!((text.as_str(), ask.as_str()), ("Fine.", ""));
}

#[test]
fn which_user_entries_start_a_turn() {
    let prompt = |json: &str| {
        let entry: Value = serde_json::from_str(json).expect("entry");
        let content = entry.get("message").and_then(|m| m.get("content")).cloned();
        is_user_prompt(&entry, content.as_ref())
    };
    assert!(prompt(r#"{"message":{"content":"hello"}}"#));
    assert!(prompt(r#"{"isCompactSummary":true,"isMeta":true}"#));
    assert!(prompt(r#"{"origin":{"kind":"human"},"isMeta":true}"#));
    assert!(!prompt(
        r#"{"origin":{"kind":"task-notification"},"message":{"content":"x"}}"#
    ));
    assert!(!prompt(r#"{"isMeta":true,"message":{"content":"x"}}"#));
    assert!(!prompt(
        r#"{"promptSource":"system","message":{"content":"x"}}"#
    ));
    assert!(!prompt(
        r#"{"message":{"content":[{"type":"tool_result"},{"type":"tool_result"}]}}"#
    ));
    assert!(prompt(
        r#"{"message":{"content":[{"type":"text","text":"hi"}]}}"#
    ));
}

#[test]
fn the_last_paragraph_decides_whether_a_reply_asks() {
    assert!(last_paragraph_asks(
        "I looked at both.\n\nWhich one do you want?"
    ));
    assert!(last_paragraph_asks("Which one do you want?  \n\n"));
    assert!(last_paragraph_asks("**Shall I push it?**"));
    // The user's "no click steps" reply.
    assert!(!last_paragraph_asks(
        "The fix is in. There are no click steps for you: it works now."
    ));
    // A "?" only inside a code block.
    assert!(!last_paragraph_asks(
        "Run this:\n\n```bash\nls *.txt?\n```\n"
    ));
    // A question mid-reply, then a statement.
    assert!(!last_paragraph_asks(
        "Do you want the old one? I kept it.\n\nIt is in the backup folder."
    ));
    assert!(!last_paragraph_asks(""));
}

#[test]
fn a_next_step_question_asks_even_when_the_text_does_not() {
    assert!(reply_asks("Done.", "Merge now?"));
    assert!(!reply_asks("Done.", "   "));
    assert!(reply_asks("Done. Which one?", ""));
}

#[test]
fn a_missing_transcript_reads_as_nothing() {
    assert_eq!(
        last_turn(Path::new(r"C:\no\such\transcript.jsonl")),
        (String::new(), String::new())
    );
}

#[test]
fn a_session_target_names_a_chat_by_its_id() {
    assert_eq!(claude_waits::session_target("session:abc-1"), Some("abc-1"));
    assert_eq!(claude_waits::session_target(" session: abc "), Some("abc"));
    assert_eq!(claude_waits::session_target("session:"), None);
    assert_eq!(claude_waits::session_target("Fix the build"), None);
    assert_eq!(claude_waits::session_target("hwnd:12"), None);
}
