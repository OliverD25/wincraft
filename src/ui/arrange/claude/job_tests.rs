//! The Board and the chips with chats whose job runs on a server.

use super::*;
use crate::core::server_jobs::OnServer;

const MIN: u64 = 60_000;
const NOW: u64 = 100 * 24 * 60 * MIN;

fn card(status: Status, since_ms: u64) -> Card {
    Card {
        status: Some(status),
        since: Some(NOW - since_ms),
        last_activity: Some(NOW - since_ms),
        ..Card::default()
    }
}

fn job(since_ms: u64, place: &str, busy: bool) -> Card {
    Card {
        jobs: 1,
        server_job: Some(OnServer {
            place: place.to_string(),
            busy,
        }),
        ..card(Status::ServerJob, since_ms)
    }
}

#[test]
fn the_job_chip_names_the_time_and_the_server() {
    assert_eq!(
        chip_text(&job(108 * MIN, "home2", false), NOW).as_deref(),
        Some("Job 1 h 48 m \u{b7} home2")
    );
    assert_eq!(
        chip_text(&job(3 * MIN, "2 servers", true), NOW).as_deref(),
        Some("Job 3 min \u{b7} 2 servers")
    );
    let no_time = Card {
        since: None,
        ..job(0, "home2", false)
    };
    assert_eq!(
        chip_text(&no_time, NOW).as_deref(),
        Some("Job \u{b7} home2")
    );
}

#[test]
fn the_working_filter_takes_server_jobs_and_idle_does_not() {
    let card = job(5 * MIN, "home2", false);
    assert!(Filter::Working.matches(&card, NOW));
    assert!(!Filter::Idle.matches(&card, NOW));
    assert!(!Filter::Cold.matches(&card, NOW));
    assert!(!Filter::Waiting.matches(&card, NOW));
    assert_eq!(counts([card].iter(), NOW), [0, 1, 0, 0]);
}

#[test]
fn server_jobs_close_the_working_column_after_the_background_rows() {
    let background = Card {
        jobs: 2,
        ..card(Status::Background, 20 * MIN)
    };
    let cards = [
        job(90 * MIN, "home2", true),   // 1
        card(Status::Working, 2 * MIN), // 2
        background,                     // 3
        job(10 * MIN, "home1", false),  // 4
        card(Status::Working, 9 * MIN), // 5
    ];
    let names: Vec<String> = (1..=cards.len()).map(|n| format!("Chat {n}")).collect();
    let windows: Vec<_> = cards
        .iter()
        .enumerate()
        .map(|(at, card)| (at as isize + 1, at + 1, names[at].as_str(), card))
        .collect();
    let columns = columns(&windows, NOW);
    let numbers: Vec<usize> = columns[1].iter().map(|row| row.number).collect();
    assert_eq!(numbers, [5, 2, 3, 1, 4], "longest run first in each kind");
    assert_eq!(columns[1][3].time, "1 h 30 m \u{b7} home2");
    assert!(columns[2].is_empty() && columns[3].is_empty());
}

#[test]
fn the_working_column_ranks_working_background_then_job() {
    assert_eq!(working_rank(Some(Status::Working)), 0);
    assert_eq!(working_rank(Some(Status::Background)), 1);
    assert_eq!(working_rank(Some(Status::ServerJob)), 2);
}

#[test]
fn a_busy_job_is_marked_for_its_pulse() {
    let busy = Mark::new(&job(MIN, "home2", true), NOW, None, true);
    assert!(busy.job_busy);
    assert_eq!(busy.status, Some(Status::ServerJob));
    let resting = Mark::new(&job(MIN, "home2", false), NOW, None, true);
    assert!(!resting.job_busy);
}
