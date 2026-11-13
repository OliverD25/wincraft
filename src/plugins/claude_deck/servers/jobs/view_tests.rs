//! Where jobs from the PC show: the server line on the Chats tab, the
//! section "Work from the PC" on the Servers tab, and the 24 h history.

use std::collections::BTreeSet;

use crate::core::claude_launches::{Launch, Outcome};

use super::super::load::{Kind, Service, State};
use super::super::poll::HostView;
use super::super::tab::view::{self as tab_view, Line, Work};
use super::super::view::{Shape, NONE_WORKING};
use super::super::Aliases;
use super::history::{Ended, History, How, KEEP_MS, MOST};
use super::link_tests::launch;
use super::load::UnitInfo;
use super::text::{ended_right, job_right, server_hover, server_words, with_jobs, SECTION};
use super::{Job, PcChat};

const NOW: u64 = 1_794_556_800_000;
const MIN: u64 = 60_000;

fn job(server: &str, chat: &str, name: &str, unit: Option<&str>) -> Job {
    Job {
        server: server.to_string(),
        chats: vec![(chat.to_string(), format!("{chat} chat"))],
        name: name.to_string(),
        command: format!("python -m {name}"),
        folder: Some("/home/rde/projects/verdict-ai".to_string()),
        pid: Some(42),
        unit: unit.map(str::to_string),
        started: Some(NOW - 130 * MIN),
        cpu10: Some(24),
        rss_kb: Some(2_726_297),
        ..Job::default()
    }
}

fn online() -> HostView {
    HostView {
        checked: Some(NOW),
        online: true,
        last_seen: Some(NOW),
        ..HostView::default()
    }
}

fn aliases() -> Aliases {
    Aliases::build(&["rdehome2".to_string()], None)
}

#[test]
fn the_server_line_counts_the_jobs_and_their_chats() {
    let a = job("rdehome2", "a", "tools.archive write", None);
    let b = job("rdehome2", "a", "office_fetch.py run", None);
    let mut repo = job("rdehome2", "x", "topup_b.sh", None);
    repo.chats.push(("y".to_string(), "y chat".to_string()));
    repo.repo = Some("verdict-ai".to_string());
    assert_eq!(server_words(&[&a]), "1 job from 1 PC chat \u{b7} CPU 2%");
    assert_eq!(
        server_words(&[&a, &b, &repo]),
        "3 jobs from 2 PC chats \u{b7} CPU 7%"
    );
    assert_eq!(
        with_jobs(NONE_WORKING.to_string(), true, &[&a]),
        "1 job from 1 PC chat \u{b7} CPU 2%"
    );
    assert_eq!(
        with_jobs("1 working".to_string(), true, &[&a]),
        "1 working \u{b7} 1 job from 1 PC chat \u{b7} CPU 2%"
    );
    assert_eq!(with_jobs("Offline".to_string(), false, &[&a]), "Offline");
    assert_eq!(
        server_hover(&[&a, &repo], NOW),
        "Work from the PC:\na chat \u{b7} tools.archive write \u{b7} 2 h 10 m\nverdict-ai chats \u{b7} topup_b.sh \u{b7} 2 h 10 m"
    );
    assert_eq!(job_right(&a, NOW), "2 h 10 m \u{b7} 2.4% \u{b7} 2.6 GB");
}

fn tab_lines(views: &[(String, HostView)], jobs: &[Job], history: &History) -> Vec<Line> {
    let none = BTreeSet::new();
    let shape = Shape {
        pinned: &none,
        hidden: &none,
        quiet_open: &none,
        hidden_open: &none,
        fold_cold: true,
        now: NOW,
    };
    let work = Work { jobs, history };
    tab_view::lines(views, &|_| false, &shape, &work)
}

fn ended(key: &str, at: u64, how: How, code: Option<i32>) -> Ended {
    Ended {
        server: "rdehome2".to_string(),
        key: key.to_string(),
        owner: "a chat".to_string(),
        chat: Some("a".to_string()),
        name: "old job".to_string(),
        command: "python old.py".to_string(),
        folder: None,
        pid: Some(9),
        started: Some(at - 130 * MIN),
        ended: at,
        how,
        code,
    }
}

#[test]
fn work_from_the_pc_is_the_first_section_with_ended_jobs_after() {
    let views = [("rdehome2".to_string(), online())];
    let running = job("rdehome2", "a", "tools.archive write", None);
    let mut history = History::default();
    history.ended = vec![ended("rdehome2/pid 9", NOW - 10 * MIN, How::Well, None)];
    let lines = tab_lines(&views, std::slice::from_ref(&running), &history);
    assert!(matches!(lines[1], Line::Section(SECTION)));
    assert!(matches!(&lines[2], Line::Work { job, .. } if job.name == "tools.archive write"));
    assert!(matches!(&lines[3], Line::Ended { end, .. } if end.name == "old job"));
    let picks: Vec<_> = lines[2..4].iter().map(|line| line.pick(&views)).collect();
    assert!(picks.iter().all(Option::is_some));
    assert_ne!(picks[0], picks[1]);
    let nothing = tab_lines(&views, &[], &History::default());
    assert!(!nothing
        .iter()
        .any(|line| matches!(line, Line::Section(SECTION))));
}

#[test]
fn an_ended_row_says_how_long_it_ran_and_how_it_ended() {
    let well = ended("k", NOW, How::Well, None);
    assert_eq!(ended_right(&well), "ran 2 h 10 m \u{b7} ended well");
    let failed = ended("k", NOW, How::Failed, Some(1));
    assert_eq!(ended_right(&failed), "ran 2 h 10 m \u{b7} failed (exit 1)");
    let mut gone = ended("k", NOW, How::Ended, None);
    gone.started = None;
    assert_eq!(ended_right(&gone), "ended");
}

#[test]
fn a_failed_job_unit_is_no_problem_of_the_server() {
    let mut view = online();
    view.services = vec![
        Service {
            kind: Kind::User,
            name: "verdict-import.service".to_string(),
            state: State::Failed,
            detail: "failed".to_string(),
        },
        Service {
            kind: Kind::User,
            name: "other.service".to_string(),
            state: State::Failed,
            detail: "failed".to_string(),
        },
    ];
    view.units = vec![UnitInfo {
        name: "verdict-import.service".to_string(),
        active: "failed".to_string(),
        ..UnitInfo::default()
    }];
    let problems = tab_view::problems(&[("rdehome2".to_string(), view)]);
    assert_eq!(problems, ["home2: other.service failed"]);
}

fn pc<'a>(launches: &'a [Launch]) -> [PcChat<'a>; 1] {
    [PcChat {
        id: "a",
        title: "a chat",
        folder: None,
        launches,
    }]
}

#[test]
fn a_job_that_goes_away_while_its_server_answers_is_kept_for_a_day() {
    let views = [("rdehome2".to_string(), online())];
    let none: [Launch; 0] = [];
    let mut history = History::default();
    let running = job("rdehome2", "a", "topup_b.sh", None);
    assert!(!history.note(&[running], &views, &pc(&none), &aliases(), NOW));
    assert!(history.note(&[], &views, &pc(&none), &aliases(), NOW + MIN));
    assert_eq!(history.ended.len(), 1);
    assert_eq!(history.ended[0].how, How::Ended);
    assert_eq!(history.ended[0].ended, NOW + MIN);
    // A day later it is gone.
    assert!(history.prune(NOW + MIN + KEEP_MS + 1));
    assert!(history.ended.is_empty());
}

#[test]
fn a_unit_end_comes_from_systemd_and_an_offline_server_ends_nothing() {
    let mut view = online();
    view.units = vec![UnitInfo {
        name: "verdict-archive-write.service".to_string(),
        active: "failed".to_string(),
        result: "exit-code".to_string(),
        status: Some(1),
        started: Some(NOW - 60 * MIN),
        ended: Some(NOW - 5 * MIN),
    }];
    let calls = [launch(
        "ssh rdehome2 'systemctl --user start verdict-archive-write'",
        NOW - 61 * MIN,
        false,
    )];
    let mut offline = online();
    offline.online = false;
    let mut history = History::default();
    let unit_job = job(
        "rdehome2",
        "a",
        "verdict-archive-write",
        Some("verdict-archive-write.service"),
    );
    history.note(
        std::slice::from_ref(&unit_job),
        &[("rdehome2".to_string(), offline.clone())],
        &pc(&calls),
        &aliases(),
        NOW,
    );
    assert!(!history.note(
        &[],
        &[("rdehome2".to_string(), offline)],
        &pc(&calls),
        &aliases(),
        NOW
    ));
    assert!(history.ended.is_empty(), "offline says nothing");
    assert!(history.note(
        &[],
        &[("rdehome2".to_string(), view)],
        &pc(&calls),
        &aliases(),
        NOW
    ));
    assert_eq!(history.ended.len(), 1, "the same end is kept once");
    let end = &history.ended[0];
    assert_eq!((end.how, end.code), (How::Failed, Some(1)));
    assert_eq!(end.ended, NOW - 5 * MIN);
    assert_eq!(ended_right(end), "ran 55 min \u{b7} failed (exit 1)");
}

#[test]
fn a_background_calls_notice_is_its_end_and_the_list_stays_small() {
    let mut call = launch("ssh rdehome2 'python3 other.py go'", NOW - 30 * MIN, true);
    call.ended = Some((Some(NOW - MIN), Outcome::Failed(Some(2))));
    let calls = [call];
    let mut history = History::default();
    let views = [("rdehome2".to_string(), online())];
    assert!(history.note(&[], &views, &pc(&calls), &aliases(), NOW));
    assert_eq!(history.ended[0].name, "other.py go");
    assert_eq!(
        ended_right(&history.ended[0]),
        "ran 29 min \u{b7} failed (exit 2)"
    );
    assert!(!history.note(&[], &views, &pc(&calls), &aliases(), NOW));
    history.ended = (0..MOST as u64 + 20)
        .map(|n| ended(&format!("k{n}"), NOW - n * 1000, How::Well, None))
        .collect();
    history.prune(NOW);
    assert_eq!(history.ended.len(), MOST);
    assert_eq!(history.ended[0].key, "k0", "the newest are kept");
}
