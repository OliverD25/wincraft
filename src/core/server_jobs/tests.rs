use super::*;

fn entry(chat: &str, servers: &[&str], jobs: usize) -> Entry {
    Entry {
        chat_id: chat.to_string(),
        servers: servers.iter().map(|s| s.to_string()).collect(),
        jobs,
        oldest: Some(1_000),
        busy: true,
    }
}

fn card(status: Status) -> Card {
    Card {
        status: Some(status),
        since: Some(5_000),
        last_activity: Some(5_000),
        ..Card::default()
    }
}

#[test]
fn a_snapshot_older_than_two_minutes_counts_as_nothing() {
    let entries = vec![entry("a", &["home2"], 1)];
    assert!(fresh(0, FRESH_MS));
    assert!(!fresh(0, FRESH_MS + 1));
    assert!(fresh(500, 100), "a clock that went back still counts");
    assert!(pick(&entries, 0, "a", 60_000).is_some());
    assert!(pick(&entries, 0, "a", FRESH_MS + 1).is_none(), "stale");
    assert!(pick(&entries, 0, "b", 60_000).is_none(), "another chat");
}

#[test]
fn the_place_is_the_server_or_how_many() {
    assert_eq!(place_text(&["home2".to_string()]), "home2");
    assert_eq!(
        place_text(&["home2".to_string(), "office1".to_string()]),
        "2 servers"
    );
}

#[test]
fn only_an_idle_chat_becomes_a_job() {
    let job = entry("a", &["home2"], 2);
    let got = apply(card(Status::Idle), Some(&job));
    assert_eq!(got.status, Some(Status::ServerJob));
    assert_eq!(got.since, Some(1_000), "since the oldest job started");
    assert_eq!(got.jobs, 2);
    assert_eq!(
        got.server_job,
        Some(OnServer {
            place: "home2".to_string(),
            busy: true
        })
    );
    for status in [Status::Waiting, Status::Working, Status::Background] {
        assert_eq!(
            apply(card(status), Some(&job)),
            card(status),
            "{status:?} wins"
        );
    }
    assert_eq!(apply(card(Status::Idle), None), card(Status::Idle));
    let none = entry("a", &["home2"], 0);
    assert_eq!(apply(card(Status::Idle), Some(&none)), card(Status::Idle));
    let unknown = Card::default();
    assert_eq!(
        apply(unknown.clone(), Some(&job)),
        unknown,
        "no status, no job"
    );
}
