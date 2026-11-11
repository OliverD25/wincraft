use super::*;

const MIN: u64 = 60_000;
const HOUR_MS: u64 = 60 * MIN;
const DAY_MS: u64 = 24 * HOUR_MS;

const FULL: &str = r#"{
    "sessionId": "local_1",
    "cliSessionId": "cli-1",
    "title": " Fix the build ",
    "previousTitles": ["Old name", "", null],
    "createdAt": 1000,
    "lastActivityAt": 5000,
    "lastTurnEdgeAt": 4000,
    "latestUserFrameAt": 4500,
    "completedTurns": 3,
    "isArchived": false,
    "remoteMcpServersConfig": [{"name": "big", "tools": [1, 2, 3]}]
}"#;

fn session(title: &str, previous: &[&str], activity: u64) -> Session {
    Session {
        id: format!("local_{title}"),
        title: title.to_string(),
        previous_titles: previous.iter().map(|t| t.to_string()).collect(),
        last_activity: Some(activity),
        last_turn_edge: Some(10_000),
        latest_user: Some(1),
        archived: false,
    }
}

#[test]
fn a_session_file_gives_every_field_it_has() {
    let session = parse_fast(FULL).expect("parses");
    assert_eq!(session.id, "local_1");
    assert_eq!(session.title, "Fix the build");
    assert_eq!(session.previous_titles, vec!["Old name".to_string()]);
    assert_eq!(session.last_activity, Some(5000));
    assert_eq!(session.last_turn_edge, Some(4000));
    assert_eq!(session.latest_user, Some(4500));
    assert!(!session.archived);
    assert_eq!(parse(FULL), Some(session));
}

#[test]
fn missing_or_odd_fields_mean_no_value_not_a_failure() {
    let session = parse_fast(r#"{"title": "Only a title"}"#).expect("parses");
    assert_eq!(session.title, "Only a title");
    assert_eq!(session.last_activity, None);
    assert_eq!(status(&session), None);
    // A time written as text counts as missing, and the rest still reads.
    let session = parse_fast(r#"{"title": "T", "lastTurnEdgeAt": "soon", "lastActivityAt": 7}"#)
        .expect("parses");
    assert_eq!(session.last_turn_edge, None);
    assert_eq!(session.last_activity, Some(7));
}

#[test]
fn archived_sessions_are_read_but_never_matched() {
    let mut archived = parse_fast(r#"{"title": "Gone", "isArchived": true}"#).expect("parses");
    assert!(archived.archived);
    archived.id = "a".into();
    assert_eq!(find(&[archived], &[], "Gone"), None);
}

#[test]
fn bad_json_is_none() {
    assert_eq!(parse("{not json"), None);
    assert_eq!(parse_fast("{not json"), None);
    assert_eq!(parse("[1, 2]"), None);
    assert_eq!(parse_fast(""), None);
}

#[test]
fn popouts_give_session_ids_and_titles() {
    let text = r#"{"version":1,"popouts":[
        {"sessionId":"local_a","title":"Verdict AI","bounds":{}},
        {"sessionId":"local_b","title":"  "},
        {"title":"no id"}]}"#;
    let popouts = parse_popouts(text);
    assert_eq!(popouts.len(), 2);
    assert_eq!(popouts[0].session_id, "local_a");
    assert_eq!(popouts[0].title, "Verdict AI");
    assert_eq!((popouts[0].width, popouts[0].height), (0, 0));
    assert_eq!(popouts[1].title, "");
    assert!(parse_popouts("nonsense").is_empty());
}

#[test]
fn working_lasts_until_the_edge_that_ends_the_turn() {
    let mut s = session("Chat", &[], 10);
    s.latest_user = Some(100_000);
    // The edge that starts the turn: a little before or after the message.
    for edge in [99_000, 100_000, 100_027, 101_104, 102_999] {
        s.last_turn_edge = Some(edge);
        assert_eq!(status(&s), Some(Status::Working), "edge {edge}");
    }
    // The edge that ends it, 3 s or more later.
    for edge in [103_000, 105_575, 149_658] {
        s.last_turn_edge = Some(edge);
        assert_eq!(status(&s), Some(Status::Idle), "edge {edge}");
    }
    s.latest_user = None;
    assert_eq!(status(&s), None);
}

#[test]
fn a_waiting_mark_wins_over_working_and_idle() {
    let mut working = session("Chat", &[], 10);
    working.latest_user = Some(200);
    working.last_turn_edge = Some(100);
    let card_now = card(Some(&working), Some(150));
    assert_eq!(card_now.status, Some(Status::Waiting));
    assert_eq!(card_now.since, Some(150));
    assert_eq!(card_now.last_activity, Some(10));

    let card_now = card(Some(&working), None);
    assert_eq!(card_now.status, Some(Status::Working));
    assert_eq!(card_now.since, Some(200));

    let idle = session("Chat", &[], 10);
    assert_eq!(card(Some(&idle), None).status, Some(Status::Idle));
    assert_eq!(card(Some(&idle), None).since, None);
    // No session, but the hook said it waits.
    assert_eq!(card(None, Some(5)).status, Some(Status::Waiting));
    assert_eq!(card(None, None), Card::default());
}

#[test]
fn age_steps_change_at_their_borders() {
    assert_eq!(Age::of(59 * MIN), Age::Fresh);
    assert_eq!(Age::of(HOUR_MS), Age::Cooling);
    assert_eq!(Age::of(23 * HOUR_MS + 59 * MIN), Age::Cooling);
    assert_eq!(Age::of(DAY_MS), Age::Cold);
    assert_eq!(Age::of(3 * DAY_MS - 1), Age::Cold);
    assert_eq!(Age::of(3 * DAY_MS), Age::Frozen);
    assert!(!Age::Cooling.is_cold());
    assert!(Age::Cold.is_cold() && Age::Frozen.is_cold());
    assert_eq!(Age::Fresh.picture_opacity(), 1.0);
    assert_eq!(Age::Frozen.picture_opacity(), 0.4);
}

#[test]
fn age_and_duration_texts_are_short_and_plain() {
    assert_eq!(age_text(30_000), "now");
    assert_eq!(age_text(12 * MIN), "12 min");
    assert_eq!(age_text(59 * MIN + 59_000), "59 min");
    assert_eq!(age_text(5 * HOUR_MS + 10 * MIN), "5 h");
    assert_eq!(age_text(DAY_MS), "1 day");
    assert_eq!(age_text(2 * DAY_MS + HOUR_MS), "2 days");
    assert_eq!(duration_text(40_000), "40 s");
    assert_eq!(duration_text(4 * MIN), "4 min");
    assert_eq!(duration_text(0), "0 s");
}

#[test]
fn a_window_finds_its_session_by_title_previous_title_or_popout() {
    let sessions = vec![
        session("Fix the build", &["Setup"], 10),
        session("Other", &[], 20),
    ];
    assert_eq!(
        find(&sessions, &[], "Fix the build").map(|s| s.title.as_str()),
        Some("Fix the build")
    );
    assert_eq!(
        find(&sessions, &[], "  Setup ").map(|s| s.title.as_str()),
        Some("Fix the build")
    );
    let popouts = vec![popout("local_Other", "Short name", 0, 0)];
    assert_eq!(
        find(&sessions, &popouts, "Short name").map(|s| s.title.as_str()),
        Some("Other")
    );
    assert_eq!(find(&sessions, &[], "Nothing like it"), None);
    assert_eq!(find(&sessions, &[], ""), None);
}

#[test]
fn a_current_title_beats_an_old_one_and_newer_beats_older() {
    let sessions = vec![
        session("New", &["Shared"], 50),
        session("Shared", &[], 10),
        session("Twin", &[], 10),
        session("Twin", &[], 30),
    ];
    let found = find(&sessions, &[], "Shared").expect("found");
    assert_eq!(found.title, "Shared");
    let found = find(&sessions, &[], "Twin").expect("found");
    assert_eq!(found.last_activity, Some(30));
}

#[test]
fn a_renamed_window_matches_by_its_app_title() {
    // The host passes WindowNamer's app title for a renamed window, so the
    // name the user gave it never has to match.
    let sessions = vec![session("WinCraft Rust", &[], 10)];
    assert!(find(&sessions, &[], "My own name").is_none());
    assert!(find(&sessions, &[], "WinCraft Rust").is_some());
}

fn popout(id: &str, title: &str, width: i32, height: i32) -> Popout {
    Popout {
        session_id: id.to_string(),
        title: title.to_string(),
        width,
        height,
    }
}

fn window(hwnd: isize, title: &str, width: i32, height: i32) -> ShownWindow {
    ShownWindow {
        hwnd,
        title: title.to_string(),
        width,
        height,
        scale: 1.0,
    }
}

#[test]
fn popouts_read_their_saved_size() {
    let text = r#"{"popouts":[{"sessionId":"local_a","title":"A","bounds":{"x":0,"y":0,"width":2562,"height":1393}}]}"#;
    assert_eq!(
        parse_popouts(text),
        vec![popout("local_a", "A", 2562, 1393)]
    );
}

#[test]
fn a_popout_with_an_unknown_title_is_matched_by_its_size() {
    let sessions = vec![
        session("VerdictAI - UA Legal AI platform", &[], 10),
        session("Fix the build", &[], 10),
    ];
    let popouts = vec![popout(
        "local_VerdictAI - UA Legal AI platform",
        "Verdict AI",
        1000,
        800,
    )];
    let windows = vec![
        window(1, "Claude", 1000, 800), // the main window is never taken
        window(2, "Fix the build", 1000, 800), // claimed by its session
        window(3, "Verdict AI 2", 1020, 790), // 2 % off: this one
        window(4, "Something", 1100, 800), // 10 % off
    ];
    assert_eq!(
        match_by_size(&windows, &sessions, &popouts),
        vec![(3, "local_VerdictAI - UA Legal AI platform".to_string())]
    );
}

#[test]
fn size_matching_skips_shown_popouts_far_sizes_and_unknown_sizes() {
    let sessions = vec![session("Chat", &[], 10)];
    // The window shows the session by title: nothing to guess.
    let shown = vec![window(1, "Chat", 500, 500), window(2, "Other", 500, 500)];
    let popouts = vec![popout("local_Chat", "", 500, 500)];
    assert!(match_by_size(&shown, &sessions, &popouts).is_empty());
    // Too far off, or no saved size, or no such session: no match.
    let far = vec![window(2, "Other", 520, 500)];
    assert!(match_by_size(&far, &sessions, &popouts).is_empty());
    let unsized_popouts = vec![popout("local_Chat", "", 0, 0)];
    let near = vec![window(2, "Other", 500, 500)];
    assert!(match_by_size(&near, &sessions, &unsized_popouts).is_empty());
    let unknown = vec![popout("local_Gone", "", 500, 500)];
    assert!(match_by_size(&near, &sessions, &unknown).is_empty());
}

#[test]
fn a_size_match_counts_only_when_it_is_sure() {
    let sessions = vec![session("A", &[], 10), session("B", &[], 10)];
    // Two popouts that could both be the one window: neither gets it.
    let popouts = vec![
        popout("local_A", "", 800, 600),
        popout("local_B", "", 800, 600),
    ];
    let windows = vec![window(7, "x", 800, 600)];
    assert!(match_by_size(&windows, &sessions, &popouts).is_empty());
    // One popout, two windows of its size (both maximized): no guess.
    let one = vec![popout("local_A", "", 800, 600)];
    let two = vec![window(7, "x", 800, 600), window(8, "y", 800, 600)];
    assert!(match_by_size(&two, &sessions, &one).is_empty());
    assert_eq!(
        match_by_size(&windows, &sessions, &one),
        vec![(7, "local_A".to_string())]
    );
}

#[test]
fn the_saved_size_is_scaled_by_the_monitor() {
    let sessions = vec![session("A", &[], 10)];
    let popouts = vec![popout("local_A", "", 2576, 1408)];
    let mut big = window(7, "x", 3862, 2110);
    assert!(match_by_size(&[big.clone()], &sessions, &popouts).is_empty());
    big.scale = 1.5;
    assert_eq!(
        match_by_size(&[big], &sessions, &popouts),
        vec![(7, "local_A".to_string())]
    );
}
