use super::*;
use crate::core::claude_turns::Tail;

// Records in the key order the app writes them (see `claude_turns`).

fn assistant(model: &str, usage: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"message":{{"model":"{model}","type":"message","role":"assistant","content":[{{"type":"text","text":"Done."}}],"stop_reason":"end_turn","usage":{usage}}},"requestId":"r","type":"assistant","uuid":"a"}}"#
    )
}

fn usage(input: u64, read: u64, created: u64, output: u64) -> String {
    format!(
        r#"{{"input_tokens":{input},"cache_creation_input_tokens":{created},"cache_read_input_tokens":{read},"output_tokens":{output},"server_tool_use":{{"web_search_requests":0}},"service_tier":"standard","cache_creation":{{"ephemeral_1h_input_tokens":{created}}},"iterations":[{{"input_tokens":1}}]}}"#
    )
}

fn compact_summary() -> String {
    r#"{"parentUuid":"p","isSidechain":false,"promptId":"x","type":"user","message":{"role":"user","content":"This session is being continued from a previous conversation."},"isVisibleInTranscriptOnly":true,"isCompactSummary":true,"uuid":"u"}"#.to_string()
}

fn user(text: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"promptId":"x","type":"user","message":{{"role":"user","content":"{text}"}},"uuid":"u"}}"#
    )
}

fn read(lines: &[String]) -> Context {
    let mut tail = Tail::default();
    for line in lines {
        tail.line(line);
    }
    tail.turn().context
}

#[test]
fn the_context_is_input_and_both_caches_of_the_last_request() {
    let line = assistant("claude-opus-5-5", &usage(2, 192_964, 791, 802));
    assert_eq!(usage_tokens(&line), Some(193_757));
}

#[test]
fn the_last_usage_wins_and_lines_without_one_change_nothing() {
    let lines = [
        assistant("claude-opus-5-5", &usage(10, 5_000, 0, 50)),
        user("next"),
        assistant("claude-opus-5-5", &usage(3, 300_000, 15_000, 9)),
        // No usage at all, and a failed request with zeros.
        r#"{"parentUuid":"p","isSidechain":false,"message":{"model":"m","content":[]},"type":"assistant"}"#
            .to_string(),
        assistant("<synthetic>", &usage(0, 0, 0, 0)),
    ];
    let context = read(&lines);
    assert_eq!(context.tokens, Some(315_003));
    assert_eq!(context.most, 315_003);
    assert_eq!(context.compactions, 0);
}

#[test]
fn compact_summaries_are_counted_and_the_context_drops_after_one() {
    let lines = [
        assistant("claude-opus-5-5", &usage(1, 700_000, 0, 1)),
        compact_summary(),
        assistant("claude-opus-5-5", &usage(1, 40_000, 2_000, 1)),
        compact_summary(),
        // A user who only writes the words does not compact the chat.
        user("isCompactSummary true"),
    ];
    let context = read(&lines);
    assert_eq!(context.compactions, 2);
    assert_eq!(context.tokens, Some(42_001));
    assert_eq!(context.most, 700_001);
}

#[test]
fn side_chains_and_records_of_other_kinds_are_not_read() {
    let side = assistant("m", &usage(1, 900_000, 0, 1))
        .replace("\"isSidechain\":false", "\"isSidechain\":true");
    assert_eq!(read(&[side]), Context::default());
}

#[test]
fn a_model_with_1m_in_its_id_names_a_large_window() {
    let context = read(&[assistant("claude-opus-5-5[1m]", &usage(1, 10, 0, 1))]);
    assert!(context.one_million);
    let context = read(&[assistant("claude-opus-5-5", &usage(1, 10, 0, 1))]);
    assert!(!context.one_million);
}

#[test]
fn the_compaction_point_is_the_setting_else_80_percent_of_the_window() {
    assert_eq!(compaction_point(Some(777_000), false, 0), 777_000);
    assert_eq!(compaction_point(Some(777_000), true, 900_000), 777_000);
    assert_eq!(compaction_point(None, false, 150_000), 160_000);
    assert_eq!(compaction_point(Some(0), false, 0), 160_000);
    assert_eq!(compaction_point(None, true, 0), 800_000);
    // A context over 200k means the window is larger than 200k.
    assert_eq!(compaction_point(None, false, 200_001), 800_000);
}

#[test]
fn the_setting_is_read_at_the_top_level_first_then_deeper() {
    let both = r#"{"modelSettings":{"claude-opus-5-5":{"autoCompactWindow":500000}},"autoCompactWindow":777000}"#;
    assert_eq!(setting_of(both), Some(777_000));
    let nested_only = r#"{"modelSettings":{"claude-opus-5-5":{"autoCompactWindow":500000}}}"#;
    assert_eq!(setting_of(nested_only), Some(500_000));
    assert_eq!(setting_of(r#"{"model":"opus"}"#), None);
    assert_eq!(setting_of("not json"), None);
    // A text value is not a number of tokens.
    assert_eq!(setting_of(r#"{"autoCompactWindow":"200k"}"#), None);
}
