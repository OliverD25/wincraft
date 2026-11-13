use super::*;

/// 2026-11-13T08:00:00Z.
const AT: u64 = 1_794_556_800_000;
const NOW: u64 = AT + 60 * 60 * 1000;

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

fn call(id: &str, command: &str, background: bool, stamp: &str) -> String {
    let mut input = serde_json::json!({"command": command, "description": "x"});
    if background {
        input["run_in_background"] = serde_json::json!(true);
    }
    serde_json::json!({
        "parentUuid": "p",
        "isSidechain": false,
        "message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": id, "name": "Bash", "input": input}
        ]},
        "type": "assistant",
        "timestamp": stamp,
    })
    .to_string()
}

fn started(tool: &str, task: &str) -> String {
    format!(
        "{{\"parentUuid\":\"q\",\"isSidechain\":false,\"type\":\"user\",\"message\":{{\"content\":[{{\"tool_use_id\":\"{tool}\",\"type\":\"tool_result\",\"content\":\"Command running in background with ID: {task}.\"}}]}},\"timestamp\":\"2026-11-13T08:00:01.000Z\",\"toolUseResult\":{{\"backgroundTaskId\":\"{task}\"}}}}"
    )
}

fn notice(task: &str, tool: &str, status: &str, code: i32) -> String {
    format!(
        "{{\"parentUuid\":\"r\",\"isSidechain\":false,\"attachment\":{{\"type\":\"queued_command\",\"prompt\":\"<task-notification>\\n<task-id>{task}</task-id>\\n<tool-use-id>{tool}</tool-use-id>\\n<status>{status}</status>\\n<summary>Background command \\\"x\\\" completed (exit code {code})</summary>\\n</task-notification>\",\"timestamp\":\"2026-11-13T09:00:00.000Z\"}},\"type\":\"attachment\"}}"
    )
}

fn read(lines: &[String]) -> Vec<Launch> {
    let mut launches = Launches::default();
    for line in lines {
        launches.line(line, NOW);
    }
    launches.recent(NOW)
}

#[test]
fn program_words_name_what_a_command_runs() {
    let cases = [
        (
            "nice -n 10 ionice -c3 .venv/bin/python -m tools.archive write --time-limit 5",
            Some("tools.archive write"),
        ),
        (
            "/home/rde/x/.venv/bin/python office_fetch.py run --delay-min 1.1",
            Some("office_fetch.py run"),
        ),
        ("bash /tmp/topup_b.sh", Some("topup_b.sh")),
        ("nohup bash ~/p/sync_law.sh --local 2>", Some("sync_law.sh")),
        (
            "bash -c until docker exec db psql -c select",
            Some("docker exec"),
        ),
        (
            "env -u TOKEN A=b setsid python3 -u worker.py",
            Some("worker.py"),
        ),
        (
            "node tools/builder/src/cli.ts serve --store s3",
            Some("cli.ts serve"),
        ),
        ("cd ~/projects/x", None),
        ("systemctl --user status x", None),
        ("EOF", None),
        ("bash -s", None),
    ];
    for (command, want) in cases {
        assert_eq!(program_words(&words(command)).as_deref(), want, "{command}");
    }
}

#[test]
fn units_come_from_systemctl_start_and_systemd_run() {
    let cases = [
        (
            "systemctl --user start verdict-archive-write.service",
            vec!["verdict-archive-write"],
        ),
        ("systemctl --user restart a b.service", vec!["a", "b"]),
        ("systemctl --user enable --now fetch", vec!["fetch"]),
        ("systemctl --user enable fetch", vec![]),
        ("systemctl start nginx", vec![]),
        ("systemctl --user stop x", vec![]),
        (
            "systemd-run --user --unit=verdict-queue-fix-20261113 bash /tmp/q.sh",
            vec!["verdict-queue-fix-20261113"],
        ),
        ("systemd-run --user -u job2 python x.py", vec!["job2"]),
    ];
    for (command, want) in cases {
        assert_eq!(units_of(&words(command)), want, "{command}");
    }
}

#[test]
fn keys_cover_the_remote_command_and_a_script_on_stdin() {
    let keys = keys_of(
        "ssh rdehome2 'cd ~/projects/verdict-ai-archive/api && systemctl --user start verdict-archive-write'",
    );
    assert_eq!(keys.hosts, ["rdehome2"]);
    assert_eq!(keys.units, ["verdict-archive-write"]);
    let keys = keys_of(
        "cat > /tmp/s.sh <<'EOF'\nset -e\nnohup .venv/bin/python -m tools.load_kas kas > log &\nEOF\nwsl.exe -e bash -lc 'ssh rdehome1 bash -s' < /tmp/s.sh",
    );
    assert_eq!(keys.hosts, ["rdehome1"]);
    assert_eq!(keys.words, ["tools.load_kas kas"]);
    let keys = keys_of("ssh -o BatchMode=yes rdeoffice1 python3 office_fetch.py run");
    assert_eq!(keys.remote.as_deref(), Some("office_fetch.py run"));
}

#[test]
fn ssh_calls_are_kept_with_their_time_and_keys() {
    let launches = read(&[
        call(
            "toolu_1",
            "ssh rdehome2 'systemctl --user start verdict-archive-write'",
            false,
            "2026-11-13T08:00:00.000Z",
        ),
        call("toolu_2", "git status", false, "2026-11-13T08:01:00.000Z"),
        call(
            "toolu_3",
            "echo ssh rdehome2",
            false,
            "2026-11-13T08:02:00.000Z",
        ),
    ]);
    assert_eq!(launches.len(), 1, "only commands that run ssh");
    let launch = &launches[0];
    assert_eq!(launch.at, Some(AT));
    assert_eq!(launch.units, ["verdict-archive-write"]);
    assert!(!launch.background && !launch.is_open());
}

#[test]
fn a_background_ssh_call_stays_open_until_its_notice() {
    let command = "ssh rdehome2 'cd ~/projects/x && python -m tools.archive write'";
    let open = read(&[
        call("toolu_9", command, true, "2026-11-13T08:00:00.000Z"),
        started("toolu_9", "b1"),
    ]);
    assert!(open[0].is_open());
    assert_eq!(open[0].remote.as_deref(), Some("tools.archive write"));
    let done = read(&[
        call("toolu_9", command, true, "2026-11-13T08:00:00.000Z"),
        started("toolu_9", "b1"),
        notice("b1", "toolu_9", "completed", 0),
    ]);
    assert_eq!(
        done[0].ended,
        Some((Some(AT + 60 * 60 * 1000), Outcome::Done))
    );
    let failed = read(&[
        call("toolu_9", command, true, "2026-11-13T08:00:00.000Z"),
        notice("b1", "toolu_9", "completed", 1),
    ]);
    assert_eq!(failed[0].ended.map(|e| e.1), Some(Outcome::Failed(Some(1))));
    let stopped = read(&[
        call("toolu_9", command, true, "2026-11-13T08:00:00.000Z"),
        started("toolu_9", "b1"),
        "{\"name\":\"TaskStop\",\"input\":{\"task_id\":\"b1\"}}".to_string(),
    ]);
    assert_eq!(stopped[0].ended.map(|e| e.1), Some(Outcome::Stopped));
}

#[test]
fn calls_older_than_a_week_are_dropped() {
    let launches = read(&[call(
        "toolu_1",
        "ssh rdehome2 'python -m a b'",
        false,
        "2026-11-01T08:00:00.000Z",
    )]);
    assert!(launches.is_empty());
}
