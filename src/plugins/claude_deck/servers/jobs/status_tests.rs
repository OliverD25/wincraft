//! The Server job status of a PC chat, and its words.

use crate::core::claude_sessions::Age;
use crate::plugins::claude_deck::hover::head_line;
use crate::plugins::claude_deck::model::{Chat, Mark};

use super::text::{chat_full, chat_line, run_time};
use super::{mark, Job};

const NOW: u64 = 1_794_556_800_000;
const MIN: u64 = 60_000;

fn pc_chat(id: &str, mark: Mark) -> Chat {
    Chat {
        id: id.to_string(),
        title: format!("{id} chat"),
        folder: Some(r"E:\code\verdict-ai".to_string()),
        branch: None,
        model: None,
        parent: None,
        created: 0,
        mark,
        since: Some(NOW - 60 * MIN),
        jobs: 0,
        window: None,
        text: None,
        full: None,
        fork: None,
        terminal: None,
        news: false,
        job_on: None,
    }
}

fn job(chat: &str, server: &str, name: &str, cpu10: u32, minutes: u64) -> Job {
    Job {
        server: server.to_string(),
        chats: vec![(chat.to_string(), format!("{chat} chat"))],
        name: name.to_string(),
        command: format!("python -m {name} --limit 5"),
        pid: Some(7),
        started: Some(NOW - minutes * MIN),
        cpu10: Some(cpu10),
        rss_kb: Some(2_726_297),
        ..Job::default()
    }
}

#[test]
fn server_job_ranks_right_after_waits_on_a_server() {
    assert_eq!(Mark::ServerJob(true).rank(), Mark::WaitsOnServer.rank() + 1);
    assert_eq!(Mark::ServerJob(false).rank(), Mark::ServerJob(true).rank());
    assert!(Mark::ServerJob(false).rank() < Mark::Background.rank());
    assert_eq!(Mark::ServerJob(false).word(), "Job");
    assert!(!Mark::ServerJob(true).bold());
}

#[test]
fn only_an_idle_chat_with_its_own_job_turns_into_server_job() {
    let mut chats = vec![
        pc_chat("idle", Mark::Idle(Age::Fresh)),
        pc_chat("working", Mark::Working),
        pc_chat("done", Mark::DoneNotSeen),
        pc_chat("asks", Mark::AsksYou),
        pc_chat("other", Mark::Idle(Age::Cold)),
    ];
    let mut repo = job("other", "rdehome2", "x", 0, 5);
    repo.chats
        .push(("idle".to_string(), "idle chat".to_string()));
    repo.repo = Some("verdict-ai".to_string());
    let jobs = [
        job("idle", "rdehome2", "tools.archive write", 24, 108),
        job("working", "rdehome2", "a", 0, 1),
        job("done", "rdehome2", "a", 0, 1),
        job("asks", "rdehome2", "a", 0, 1),
        repo,
    ];
    mark(&mut chats, &jobs, NOW);
    let marks: Vec<Mark> = chats.iter().map(|chat| chat.mark).collect();
    assert_eq!(
        marks,
        [
            Mark::ServerJob(false),
            Mark::Working,
            Mark::DoneNotSeen,
            Mark::AsksYou,
            Mark::Idle(Age::Cold),
        ]
    );
    let idle = &chats[0];
    assert_eq!(
        idle.text.as_deref(),
        Some("Runs on home2: tools.archive write \u{b7} 1 h 48 m")
    );
    assert_eq!(idle.since, Some(NOW - 108 * MIN));
    assert_eq!(
        head_line(idle, NOW),
        "idle chat \u{b7} job on home2 \u{b7} 1 h"
    );
}

#[test]
fn a_busy_job_pulses_and_several_jobs_are_counted() {
    let mut chats = vec![pc_chat("a", Mark::Idle(Age::Fresh))];
    let jobs = [
        job("a", "rdehome2", "tools.archive write", 240, 10),
        job("a", "rdeoffice1", "office_fetch.py run", 0, 3),
    ];
    mark(&mut chats, &jobs, NOW);
    assert_eq!(chats[0].mark, Mark::ServerJob(true));
    assert_eq!(
        chats[0].text.as_deref(),
        Some("Runs 2 jobs on home2, office1")
    );
    assert_eq!(chats[0].job_on.as_deref(), Some("home2, office1"));
    let refs: Vec<&Job> = jobs.iter().collect();
    let full = chat_full(&refs, NOW);
    assert!(full.starts_with(
        "home2 \u{b7} tools.archive write \u{b7} 10 min \u{b7} CPU 24% \u{b7} RAM 2.6 GB\npython -m tools.archive write --limit 5"
    ));
    assert!(full.contains("\n\noffice1 \u{b7} office_fetch.py run \u{b7} 3 min"));
}

#[test]
fn run_times_read_plainly_and_a_call_has_no_load() {
    assert_eq!(run_time(45_000), "45 s");
    assert_eq!(run_time(12 * MIN), "12 min");
    assert_eq!(run_time(120 * MIN), "2 h");
    assert_eq!(run_time(130 * MIN), "2 h 10 m");
    assert_eq!(run_time(27 * 60 * MIN), "1 d 3 h");
    let call = Job {
        server: "rdehome1".to_string(),
        name: "other.py go".to_string(),
        started: None,
        ..Job::default()
    };
    assert_eq!(chat_line(&[&call], NOW), "Runs on home1: other.py go");
}
