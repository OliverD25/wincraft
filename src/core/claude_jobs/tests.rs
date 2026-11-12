use super::*;

const MIN: u64 = 60_000;
const NOW: u64 = 1_794_384_000_000 + 5 * MIN;

fn start(id: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"timestamp\":\"2026-11-11T08:00:00.000Z\",\"toolUseResult\":{{\"stdout\":\"\",\"backgroundTaskId\":\"{id}\",\"backgroundCwdHint\":\"x\"}}}}\n"
    )
}

fn agent_start(id: &str) -> String {
    format!(
        "{{\"timestamp\":\"2026-11-11T08:00:00.000Z\",\"toolUseResult\":{{\"isAsync\":true,\"status\":\"async_launched\",\"agentId\":\"{id}\"}}}}\n"
    )
}

fn end(id: &str, status: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"content\":\"<task-notification>\\n<task-id>{id}</task-id>\\n<tool-use-id>toolu_1</tool-use-id>\\n<status>{status}</status>\"}}\n"
    )
}

fn stop(id: &str) -> String {
    format!("{{\"name\":\"TaskStop\",\"input\":{{\"task_id\":\"{id}\"}},\"caller\":{{}}}}\n")
}

fn scan(text: &str) -> Scan {
    let mut scan = Scan::default();
    scan.feed(text.as_bytes(), NOW);
    scan
}

#[test]
fn a_start_and_an_end_leave_no_job() {
    let text = format!("{}{}", start("b1"), end("b1", "completed"));
    assert_eq!(scan(&text).running(NOW), (0, None));
}

#[test]
fn a_start_alone_is_one_job_with_its_own_time() {
    let found = scan(&start("b1")).running(NOW);
    let started = timestamp_ms("\"timestamp\":\"2026-11-11T08:00:00.000Z\"").unwrap();
    assert_eq!(found, (1, Some(started)));
}

#[test]
fn the_timestamp_is_read_as_unix_milliseconds() {
    let at = timestamp_ms("\"timestamp\":\"1970-01-02T00:00:01.250Z\"");
    assert_eq!(at, Some(86_400_000 + 1250));
    assert_eq!(timestamp_ms("\"timestamp\":\"2026-13-01T00:00:00Z\""), None);
    assert_eq!(timestamp_ms("no stamp"), None);
}

#[test]
fn task_stop_ends_a_job() {
    let text = format!("{}{}", start("b1"), stop("b1"));
    assert_eq!(scan(&text).running(NOW).0, 0);
}

#[test]
fn a_background_agent_counts_and_any_end_status_ends_it() {
    let text = agent_start("a1");
    assert_eq!(scan(&text).running(NOW).0, 1);
    for status in ["completed", "failed", "killed", "something-new"] {
        let text = format!("{}{}", agent_start("a1"), end("a1", status));
        assert_eq!(scan(&text).running(NOW).0, 0, "{status}");
    }
}

#[test]
fn only_the_ended_job_goes() {
    let text = format!("{}{}{}", start("b1"), start("b2"), end("b1", "completed"));
    assert_eq!(scan(&text).running(NOW).0, 1);
}

#[test]
fn an_escaped_result_still_counts() {
    let text = "{\"content\":\"{\\\"backgroundTaskId\\\":\\\"b9\\\"}\"}\n";
    assert_eq!(scan(text).running(NOW).0, 1);
}

#[test]
fn unknown_lines_mean_no_job() {
    let text = "not json at all\n{}\n{\"backgroundTaskId\":5}\n<task-id></task-id>\n\
                {\"agentId\":\"x1\"}\n\u{fffd}\u{fffd}\n";
    assert_eq!(scan(text).running(NOW), (0, None));
    // A task id in plain text with no status is not an end.
    let text = format!("{}{}", start("b1"), "doc: `status <task-id>` polls\n");
    assert_eq!(scan(&text).running(NOW).0, 1);
}

#[test]
fn a_partial_last_line_waits_for_the_next_read() {
    let text = start("b1");
    let (first, second) = text.as_bytes().split_at(text.len() / 2);
    let mut scan = Scan::default();
    scan.feed(first, NOW);
    assert_eq!(scan.running(NOW).0, 0);
    scan.feed(second, NOW);
    assert_eq!(scan.running(NOW).0, 1);
}

#[test]
fn a_read_in_pieces_gives_the_same_as_a_full_read() {
    let text = format!(
        "{}{}{}{}{}",
        start("b1"),
        agent_start("a1"),
        end("b1", "completed"),
        start("b2"),
        stop("a1")
    );
    let whole = scan(&text);
    for piece in [1usize, 7, 50, 333] {
        let mut parts = Scan::default();
        for chunk in text.as_bytes().chunks(piece) {
            parts.feed(chunk, NOW);
        }
        assert_eq!(parts, whole, "pieces of {piece}");
    }
    assert_eq!(whole.running(NOW).0, 1);
}

#[test]
fn a_shorter_file_starts_over() {
    let dir = std::env::temp_dir().join(format!("wincraft-jobs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.jsonl");
    std::fs::write(&path, format!("{}{}", start("b1"), start("b2"))).unwrap();
    let mut entry = Entry::default();
    read_new(&path, &mut entry, NOW).unwrap();
    assert_eq!(entry.scan.running(NOW).0, 2);
    std::fs::write(&path, start("b3")).unwrap();
    read_new(&path, &mut entry, NOW).unwrap();
    assert_eq!(entry.scan.running(NOW).0, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_job_older_than_three_hours_is_ignored() {
    let mut scan = Scan::default();
    scan.open.insert("old".into(), NOW - MAX_JOB_AGE_MS);
    scan.open.insert("edge".into(), NOW - MAX_JOB_AGE_MS + 1);
    scan.open.insert("new".into(), NOW - MIN);
    assert_eq!(
        scan.running(NOW),
        (2, Some(NOW - MAX_JOB_AGE_MS + 1)),
        "the border job at exactly 3 hours is out, one ms younger is in"
    );
}

#[test]
fn a_quoted_start_after_the_end_does_not_reopen_the_job() {
    let text = start("b1") + &end("b1", "completed") + &start("b1");
    assert_eq!(scan(&text).running(NOW), (0, None));
}

#[test]
fn windowdeck_and_claudedeck_want_their_chats_side_by_side() {
    want(vec!["a".into(), "b".into()]);
    want_for_deck(vec!["b".into(), "c".into()]);
    assert_eq!(wanted(), vec!["a", "b", "c"]);
    want_for_deck(Vec::new());
    assert_eq!(wanted(), vec!["a", "b"]);
    want(Vec::new());
    assert!(wanted().is_empty());
}

#[test]
fn the_job_count_stays_while_a_read_runs() {
    let entry = Entry {
        scan: scan(&start("keep1")),
        ..Entry::default()
    };
    let mut entries = HashMap::new();
    entries.insert("chat-keep".to_string(), entry);
    publish_open(&entries);
    // A long read holds the reader's own state; the count must not drop.
    let held = READ.lock().unwrap().take();
    assert_eq!(running("chat-keep", NOW).0, 1);
    *READ.lock().unwrap() = held;
}
