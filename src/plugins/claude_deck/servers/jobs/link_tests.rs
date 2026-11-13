//! Which PC chat a server job belongs to. The jobs are real ones seen on
//! home1 and home2 (2026-11-13), shortened.

use crate::core::claude_launches::{keys_of, Launch, Outcome};

use super::super::poll::HostView;
use super::super::Aliases;
use super::link::{self, link, same_words, server_repo, units_to_ask};
use super::load::RawJob;
use super::{Job, PcChat};

const NOW: u64 = 1_794_556_800_000;
const MIN: u64 = 60_000;

fn aliases() -> Aliases {
    Aliases::build(
        &["rdehome2".to_string(), "rdehome1".to_string()],
        Some("Host rdehome2 home2alias\n"),
    )
}

pub fn launch(command: &str, at: u64, background: bool) -> Launch {
    let keys = keys_of(command);
    Launch {
        hosts: keys.hosts,
        command: command.to_string(),
        at: Some(at),
        units: keys.units,
        words: keys.words,
        remote: keys.remote,
        background,
        ..Launch::default()
    }
}

fn job(unit: Option<&str>, secs: u64, cwd: &str, args: &str) -> RawJob {
    RawJob {
        unit: unit.map(str::to_string),
        pid: 100,
        secs,
        cpu10: 24,
        rss_kb: 2_000_000,
        cwd: cwd.to_string(),
        args: args.to_string(),
        ..RawJob::default()
    }
}

fn view(jobs: Vec<RawJob>) -> HostView {
    HostView {
        checked: Some(NOW),
        online: true,
        last_seen: Some(NOW),
        home: Some("/home/rde".to_string()),
        jobs,
        ..HostView::default()
    }
}

fn chat<'a>(id: &'a str, folder: &'a str, launches: &'a [Launch]) -> PcChat<'a> {
    PcChat {
        id,
        title: id,
        folder: Some(folder),
        launches,
    }
}

const ARCHIVE: &str = "/home/rde/projects/verdict-ai-archive/api/.venv/bin/python -m tools.archive write --time-limit 19800";

#[test]
fn a_unit_started_by_a_chat_belongs_to_that_chat() {
    let calls = [launch(
        "ssh home2alias 'systemctl --user start verdict-archive-write'",
        NOW - 30 * MIN,
        false,
    )];
    let other: [Launch; 0] = [];
    let chats = [
        chat("starter", r"E:\code\verdict-ai", &calls),
        chat("other", r"E:\code\verdict-ai", &other),
    ];
    let mut timer = job(
        Some("verdict-archive-write.service"),
        20 * 60,
        "/home/rde/projects/verdict-ai-archive/api",
        ARCHIVE,
    );
    timer.timer = true;
    let views = [("rdehome2".to_string(), view(vec![timer]))];
    let jobs = link(&chats, &views, &aliases(), NOW);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].owner(), "starter");
    assert_eq!(jobs[0].name, "verdict-archive-write");
    assert_eq!(jobs[0].started, Some(NOW - 20 * MIN));
}

#[test]
fn a_timer_job_nobody_started_is_left_out() {
    let calls = [launch(
        "ssh rdehome2 'python -m tools.archive write --limit 5'",
        NOW - 60 * MIN,
        false,
    )];
    let chats = [chat("a", r"E:\code\verdict-ai-archive", &calls)];
    let mut timer = job(
        Some("verdict-archive-write.service"),
        10 * 60,
        "/home/rde/projects/verdict-ai-archive/api",
        ARCHIVE,
    );
    timer.timer = true;
    let mut boot = job(
        Some("newshq-builder.service"),
        60,
        "/home/rde/projects/verdict-ai-archive",
        "node tools/builder/src/cli.ts serve",
    );
    boot.enabled = true;
    let views = [("rdehome2".to_string(), view(vec![timer, boot]))];
    assert!(link(&chats, &views, &aliases(), NOW).is_empty());
}

#[test]
fn program_words_link_a_nohup_job_started_after_the_call() {
    let calls = [launch(
        "ssh rdehome2 'cd ~/projects/x && nohup bash /tmp/topup_b.sh &'",
        NOW - 10 * MIN,
        false,
    )];
    let chats = [chat("a", r"E:\code\elsewhere", &calls)];
    let after = job(None, 5 * 60, "/home/rde/projects/x", "bash /tmp/topup_b.sh");
    let before = job(None, 60 * 60, "/home/rde", "bash /tmp/topup_b.sh");
    let views = [("rdehome2".to_string(), view(vec![after, before]))];
    let jobs = link(&chats, &views, &aliases(), NOW);
    assert_eq!(jobs.len(), 1, "a job older than the call is not its job");
    assert_eq!(jobs[0].name, "topup_b.sh");
    assert_eq!(jobs[0].owner(), "a");
}

#[test]
fn with_no_launch_the_folder_names_the_repos_chats() {
    let none: [Launch; 0] = [];
    let chats = [
        chat("one", r"E:\code\verdict-ai", &none),
        chat("two", r"E:\code\Verdict-AI\.claude\worktrees\x", &none),
        chat("three", r"E:\code\wincraft", &none),
    ];
    let topup = job(
        None,
        379,
        "/home/rde/projects/verdict-ai/api",
        "bash /tmp/topup_b.sh",
    );
    let home = job(None, 379, "/home/rde", "bash /tmp/queue_fix.sh");
    let old = job(
        None,
        8 * 24 * 3600,
        "/home/rde/projects/verdict-ai",
        "python x.py",
    );
    let views = [("rdehome2".to_string(), view(vec![topup, home, old]))];
    let jobs = link(&chats, &views, &aliases(), NOW);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].owner(), "verdict-ai chats");
    assert_eq!(jobs[0].chats.len(), 2);
    assert_eq!(jobs[0].chat(), None);
}

#[test]
fn an_open_background_call_is_a_job_unless_it_named_one() {
    let mut calls = vec![
        launch(
            "ssh rdehome1 'python3 office_fetch.py run'",
            NOW - 5 * MIN,
            true,
        ),
        launch("ssh rdehome1 'python3 other.py go'", NOW - 5 * MIN, true),
        launch("ssh rdehome1 'python3 done.py go'", NOW - 5 * MIN, true),
        launch(
            "ssh rdehome1 'python3 old.py go'",
            NOW - 13 * 60 * MIN,
            true,
        ),
    ];
    calls[2].ended = Some((Some(NOW), Outcome::Done));
    let chats = [chat("a", r"E:\code\verdict-ai", &calls)];
    let fetch = job(
        None,
        4 * 60,
        "/home/rde/projects/verdict-ai/homelab",
        "python3 office_fetch.py run",
    );
    let views = [("rdehome1".to_string(), view(vec![fetch]))];
    let jobs = link(&chats, &views, &aliases(), NOW);
    let names: Vec<&str> = jobs.iter().map(|job| job.name.as_str()).collect();
    assert_eq!(names, ["office_fetch.py run", "other.py go"]);
    assert_eq!(jobs[1].pid, None);
    assert_eq!(link::call_name(&calls[1]), "other.py go");
}

#[test]
fn an_offline_server_shows_no_jobs_and_claudes_own_server_is_skipped() {
    let none: [Launch; 0] = [];
    let chats = [chat("a", r"E:\code\x", &none)];
    let mut offline = view(vec![job(None, 60, "/home/rde/projects/x", "python a.py")]);
    offline.online = false;
    let claude = job(
        None,
        60,
        "/home/rde/projects/x",
        "/home/rde/.claude/remote/srv/4c11/server --serve",
    );
    let views = [
        ("rdehome2".to_string(), offline),
        ("rdehome1".to_string(), view(vec![claude])),
    ];
    assert!(link(&chats, &views, &aliases(), NOW).is_empty());
}

#[test]
fn words_and_repos_compare_plainly() {
    assert!(same_words("office_fetch.py", "office_fetch.py run"));
    assert!(same_words("Tools.Archive write", "tools.archive write"));
    assert!(!same_words("tools.archive status", "tools.archive write"));
    assert_eq!(
        server_repo(
            "/home/rde/projects/verdict-ai/.claude/worktrees/x",
            Some("/home/rde")
        ),
        Some("verdict-ai".to_string())
    );
    assert_eq!(server_repo("/home/rde/apps/x", Some("/home/rde")), None);
    assert_eq!(
        server_repo("/srv/projects/abc/d", None),
        Some("abc".to_string())
    );
}

#[test]
fn units_named_by_launches_and_shown_jobs_are_asked_about() {
    let calls = [launch(
        "ssh home2alias 'systemctl --user restart verdict-fetch-remote'",
        NOW,
        false,
    )];
    let chats = [chat("a", r"E:\code\verdict-ai", &calls)];
    let shown = Job {
        server: "rdehome1".to_string(),
        unit: Some("x.service".to_string()),
        ..Job::default()
    };
    let asks = units_to_ask(&chats, &[shown], &aliases());
    assert_eq!(asks["rdehome2"], ["verdict-fetch-remote.service"]);
    assert_eq!(asks["rdehome1"], ["x.service"]);
}
