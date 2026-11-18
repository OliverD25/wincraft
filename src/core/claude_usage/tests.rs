use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::files::{self, FileState};
use super::parse::{self, Tokens};
use super::*;

fn line(id: &str, model: &str, stamp: &str, usage: &str) -> String {
    format!(
        "{{\"parentUuid\":\"p\",\"isSidechain\":false,\"message\":{{\"model\":\"{model}\",\"id\":\"{id}\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"hi \\\"usage\\\":{{}}\"}}],\"usage\":{usage}}},\"type\":\"assistant\",\"uuid\":\"u\",\"timestamp\":\"{stamp}\"}}\n"
    )
}

const USAGE: &str = "{\"input_tokens\":2,\"cache_creation_input_tokens\":100,\"cache_read_input_tokens\":1000,\"output_tokens\":50,\"cache_creation\":{\"ephemeral_1h_input_tokens\":100,\"ephemeral_5m_input_tokens\":0},\"iterations\":[{\"input_tokens\":2}]}";

/// UTC slots, so the tests do not depend on the PC's time zone.
fn utc(at_ms: u64) -> files::Slot {
    (
        (at_ms / 86_400_000) as i32,
        ((at_ms / 3_600_000) % 24) as u8,
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wincraft-usage-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn append(path: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

#[test]
fn a_usage_line_is_read_and_other_lines_are_not() {
    let text = line(
        "msg_1",
        "claude-opus-5-5",
        "2026-11-18T10:30:00.000Z",
        USAGE,
    );
    let reply = parse::reply(&text).expect("a reply");
    assert_eq!(reply.id.as_deref(), Some("msg_1"));
    assert_eq!(reply.model, "claude-opus-5-5");
    assert_eq!(
        reply.tokens,
        Tokens {
            input: 2,
            output: 50,
            read: 1000,
            write: 100,
            write_1h: 100
        }
    );
    assert_eq!(utc(reply.at_ms).1, 10);
    // A user line with a subagent's usage in its tool result is no reply.
    let user = "{\"type\":\"user\",\"message\":{\"role\":\"user\"},\"toolUseResult\":{\"usage\":{\"input_tokens\":5}},\"timestamp\":\"2026-11-18T10:30:00.000Z\"}";
    assert_eq!(parse::reply(user), None);
    assert_eq!(parse::reply("{\"type\":\"user\"}"), None);
    // A broken line is skipped.
    let broken = &text[..text.len() / 2 + 40];
    assert_eq!(parse::reply(broken), None);
    let zero = line(
        "msg_2",
        "<synthetic>",
        "2026-11-18T10:30:00.000Z",
        "{\"input_tokens\":0,\"output_tokens\":0}",
    );
    assert_eq!(parse::reply(&zero), None);
}

#[test]
fn only_new_bytes_are_read_and_a_shrunk_file_starts_again() {
    let dir = scratch("offsets");
    let path = dir.join("a.jsonl");
    let first = line("msg_1", "claude-opus-5-5", "2026-11-18T10:00:00Z", USAGE);
    append(&path, &first);
    // A line still being written is left for the next pass.
    append(&path, "{\"half\":");
    let mut state = FileState::default();
    files::read_new(&path, &mut state, &utc).unwrap();
    assert_eq!(state.offset, first.len() as u64);
    assert_eq!(state.buckets[0].tokens.output, 50);
    std::fs::write(
        &path,
        format!(
            "{first}{}",
            line("msg_2", "claude-opus-5-5", "2026-11-18T10:05:00Z", USAGE)
        ),
    )
    .unwrap();
    files::read_new(&path, &mut state, &utc).unwrap();
    assert_eq!(
        state.buckets[0].tokens.output, 100,
        "only the new line is added"
    );
    let short = line(
        "msg_9",
        "claude-opus-5-5",
        "2026-11-18T11:00:00Z",
        "{\"output_tokens\":7}",
    );
    std::fs::write(&path, &short).unwrap();
    files::read_new(&path, &mut state, &utc).unwrap();
    assert_eq!(state.offset, short.len() as u64);
    assert_eq!(state.buckets.len(), 1, "the old buckets went");
    assert_eq!(state.buckets[0].tokens.output, 7);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_reply_split_over_lines_counts_once() {
    let mut state = FileState::default();
    let text = line("msg_1", "claude-opus-5-5", "2026-11-18T10:00:00Z", USAGE);
    let reply = parse::reply(&text).unwrap();
    state.count(&reply, utc(reply.at_ms));
    state.count(&reply, utc(reply.at_ms));
    assert_eq!(state.buckets[0].tokens.output, 50);
}

#[test]
fn subagents_add_to_their_chat_and_to_its_subagent_share() {
    let dir = scratch("tree");
    let folder = dir.join("E--code--app");
    let session = "11111111-2222-3333-4444-555555555555";
    std::fs::create_dir_all(folder.join(session).join("subagents")).unwrap();
    append(
        &folder.join(format!("{session}.jsonl")),
        &line("msg_1", "claude-opus-5-5", "2026-11-18T10:00:00Z", USAGE),
    );
    append(
        &folder
            .join(session)
            .join("subagents")
            .join("agent-a1.jsonl"),
        &line("msg_2", "claude-sonnet-5-5", "2026-11-18T22:00:00Z", USAGE),
    );
    let mut book = Book::default();
    let now = parse::reply(&line("m", "claude-opus-5-5", "2026-11-18T23:00:00Z", USAGE))
        .unwrap()
        .at_ms;
    let stop = AtomicBool::new(false);
    let mut seen = Vec::new();
    assert!(pass(
        &mut book,
        &dir,
        now,
        &utc,
        &mut |p| seen.push(p),
        &stop
    ));
    assert_eq!(seen.last(), Some(&Progress { done: 2, total: 2 }));
    let snapshot = summary(&book, utc(now).0);
    let chat = &snapshot.week.chats[0];
    assert_eq!(snapshot.week.chats.len(), 1);
    assert_eq!(chat.session, session);
    assert_eq!(chat.folder, "E--code--app");
    assert_eq!(chat.tokens.output, 100);
    assert_eq!(chat.subagent_tokens.output, 50);
    assert!(chat.subagent_weight > 0.0 && chat.subagent_weight < chat.weight);
    assert_eq!(chat.context, Some(1102), "the main chat's last reply");
    assert!(
        snapshot.week.hours[10] > snapshot.week.hours[22],
        "Opus costs more"
    );
    assert_eq!(snapshot.week.models[0].model, "claude-opus-5-5");
    let parts: Vec<&str> = chat.models.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(
        parts,
        ["claude-opus-5-5", "claude-sonnet-5-5"],
        "biggest first"
    );
    let summed: f64 = chat.models.iter().map(|(_, w)| w).sum();
    assert!(
        (summed - chat.weight).abs() < 1e-9,
        "the parts add up to the chat"
    );
    // Nothing changed: the second pass reads nothing.
    let mut again = Vec::new();
    assert!(!pass(
        &mut book,
        &dir,
        now,
        &utc,
        &mut |p| again.push(p),
        &stop
    ));
    assert_eq!(again, vec![Progress { done: 0, total: 0 }]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn days_outside_the_span_are_left_out() {
    let mut state = FileState {
        session: "s".into(),
        ..FileState::default()
    };
    let reply = parse::reply(&line(
        "msg_1",
        "claude-opus-5-5",
        "2026-11-10T10:00:00Z",
        USAGE,
    ))
    .unwrap();
    state.count(&reply, (100, 10));
    let reply = parse::reply(&line(
        "msg_2",
        "claude-opus-5-5",
        "2026-11-18T10:00:00Z",
        USAGE,
    ))
    .unwrap();
    state.count(&reply, (108, 10));
    let week = tally::span(std::iter::once(&state), 102, 108);
    assert_eq!(week.tokens.output, 50);
    assert_eq!(week.days.len(), 7);
    assert_eq!(week.days.last().map(|d| d.0), Some(108));
    state.drop_before(101);
    assert_eq!(state.buckets.len(), 1);
}

#[test]
fn tokens_are_weighed_by_model_price_and_unknown_models_fall_back() {
    let tokens = Tokens {
        input: 1_000_000,
        output: 1_000_000,
        read: 1_000_000,
        write: 2_000_000,
        write_1h: 1_000_000,
    };
    // Opus 5.5: 4 + 20 + 0.20 + 5 (5-minute writes) + 8 (1-hour writes).
    assert!((tokens.weight("claude-opus-5-5") - 37.2).abs() < 1e-9);
    assert!((tokens.weight("claude-opus-5-5[1m]") - 37.2).abs() < 1e-9);
    // Opus 5 is not Opus 5.5: the longest match wins.
    assert!((tokens.weight("claude-opus-5") - (5.0 + 25.0 + 0.5 + 6.25 + 10.0)).abs() < 1e-9);
    assert_eq!(prices::known("claude-new-9"), None);
    assert!((tokens.weight("claude-new-9") - tokens.weight("claude-sonnet-5-5")).abs() < 1e-9);
    assert!(prices::known("claude-haiku-4-5-20251001").is_some());
}

#[test]
fn context_now_is_the_last_replys_input_and_cache() {
    let mut state = FileState::default();
    let early = parse::reply(&line(
        "msg_1",
        "claude-opus-5-5",
        "2026-11-18T10:00:00Z",
        "{\"input_tokens\":1,\"cache_read_input_tokens\":10,\"output_tokens\":5}",
    ))
    .unwrap();
    let late = parse::reply(&line(
        "msg_2",
        "claude-opus-5-5",
        "2026-11-18T11:00:00Z",
        USAGE,
    ))
    .unwrap();
    state.count(&late, utc(late.at_ms));
    state.count(&early, utc(early.at_ms));
    assert_eq!(state.context.map(|c| c.1), Some(2 + 1000 + 100));
}

#[test]
fn local_days_count_from_1970_and_know_the_weekday() {
    assert_eq!(day_number(1970, 1, 1), 0);
    assert_eq!(day_number(2026, 11, 18), 20_775);
    assert_eq!(weekday(day_number(2026, 11, 18)), 2, "a Wednesday");
}

#[test]
fn a_broken_or_missing_book_is_empty() {
    let dir = scratch("book");
    assert_eq!(load(&dir.join("none.json")), Book::default());
    std::fs::write(dir.join("bad.json"), "{nope").unwrap();
    assert_eq!(load(&dir.join("bad.json")), Book::default());
    let _ = std::fs::remove_dir_all(dir);
}

/// Times a first and a later pass over the real transcripts, read-only;
/// the book stays in memory. Run by hand:
/// `cargo test --release real_transcripts -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_transcripts_timing() {
    let projects = PathBuf::from(std::env::var_os("USERPROFILE").unwrap())
        .join(".claude")
        .join("projects");
    let stop = AtomicBool::new(false);
    let mut book = Book::default();
    let now = now_ms();
    let started = std::time::Instant::now();
    pass(&mut book, &projects, now, &local_slot, &mut |_| {}, &stop);
    let first = started.elapsed();
    let bytes: u64 = book.files.values().map(|f| f.offset).sum();
    let started = std::time::Instant::now();
    pass(
        &mut book,
        &projects,
        now_ms(),
        &local_slot,
        &mut |_| {},
        &stop,
    );
    let later = started.elapsed();
    let snapshot = summary(&book, local_slot(now).0);
    let saved = serde_json::to_string(&book).unwrap().len();
    println!(
        "files {} bytes {} first {:?} later {:?} chats {} book {} bytes",
        book.files.len(),
        bytes,
        first,
        later,
        snapshot.week.chats.len(),
        saved
    );
}
