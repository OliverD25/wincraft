use super::super::data;
use super::*;
use crate::core::claude_sessions::Lineage;

const MIN: u64 = 60_000;
const NOW: u64 = 1_000 * 24 * 60 * MIN;

fn chat(mark: Mark) -> Chat {
    Chat {
        id: "local_a".into(),
        title: "A chat".into(),
        folder: None,
        branch: None,
        model: None,
        parent: None,
        created: 0,
        mark,
        since: Some(NOW - 30 * MIN),
        jobs: 0,
        window: None,
        text: None,
        full: None,
        fork: None,
        terminal: None,
    }
}

/// A chat whose turn ended `ended` ms before now.
fn session(ended: u64) -> Session {
    Session {
        id: "local_a".into(),
        latest_user: Some(NOW - ended - 2 * MIN),
        last_turn_edge: Some(NOW - ended),
        last_activity: Some(NOW - ended),
        ..Session::default()
    }
}

fn turn(end: End, text: Option<&str>) -> Turn {
    Turn {
        end,
        text: text.map(str::to_string),
        question: None,
        tool: None,
        ssh: None,
    }
}

fn watching(turn: Option<&Turn>, since: u64, looked: Option<u64>) -> Evidence<'_> {
    Evidence {
        turn,
        watching_since: Some(since),
        looked_at: looked,
    }
}

const IDLE: Mark = Mark::Idle(Age::Fresh);

#[test]
fn a_turn_that_ended_while_watched_and_unseen_is_done() {
    let reply = turn(End::Replied, Some("The tests pass. Nothing else changed."));
    let ended = session(2 * MIN);
    let open_before = NOW - 10 * MIN;
    let done = refine(
        chat(IDLE),
        &ended,
        &watching(Some(&reply), open_before, None),
    );
    assert_eq!(done.mark, Mark::DoneNotSeen);
    assert_eq!(done.since, Some(NOW - 2 * MIN));
    assert_eq!(done.text.as_deref(), Some("The tests pass."));
    assert_eq!(
        done.full.as_deref(),
        Some("The tests pass. Nothing else changed.")
    );

    // Seen in front after the end: Idle again.
    let looked = watching(Some(&reply), open_before, Some(NOW - MIN));
    let seen = refine(chat(IDLE), &ended, &looked);
    assert_eq!(seen.mark, IDLE);
    assert_eq!(seen.text.as_deref(), Some("Last: The tests pass."));
    // Seen only before the end does not count.
    let before = watching(Some(&reply), open_before, Some(NOW - 3 * MIN));
    assert_eq!(refine(chat(IDLE), &ended, &before).mark, Mark::DoneNotSeen);

    // The app's own focus time after the end counts as seen.
    let mut focused = session(2 * MIN);
    focused.lineage = Lineage {
        last_focused: Some(NOW - MIN),
        ..Lineage::default()
    };
    let none = watching(Some(&reply), open_before, None);
    assert_eq!(refine(chat(IDLE), &focused, &none).mark, IDLE);
}

#[test]
fn a_turn_that_ended_before_watching_stays_idle() {
    let reply = turn(End::Replied, Some("Done."));
    let ended = session(20 * MIN);
    let late = watching(Some(&reply), NOW - 5 * MIN, None);
    assert_eq!(refine(chat(IDLE), &ended, &late).mark, IDLE);
    // Not watching at all.
    let blind = Evidence {
        turn: Some(&reply),
        ..Evidence::default()
    };
    assert_eq!(refine(chat(IDLE), &ended, &blind).mark, IDLE);
    // No end time: cannot tell.
    let no_edge = Session {
        last_turn_edge: None,
        ..session(MIN)
    };
    let early = watching(Some(&reply), 0, None);
    assert_eq!(refine(chat(IDLE), &no_edge, &early).mark, IDLE);
    // A transcript that ends in a tool call: the turn is still open, so the
    // chat works, whatever the session file says.
    let open = turn(End::ToolCalled, None);
    let ended = session(MIN);
    assert_eq!(
        refine(chat(IDLE), &ended, &watching(Some(&open), 0, None)).mark,
        Mark::Working
    );
}

#[test]
fn the_users_stop_and_a_failure_show_while_the_chat_is_recent() {
    let ended = session(10 * MIN);
    let stop = turn(End::Interrupted, Some("Half way."));
    let stopped = refine(chat(IDLE), &ended, &watching(Some(&stop), 0, None));
    assert_eq!(stopped.mark, Mark::Stopped);
    assert_eq!(stopped.since, Some(NOW - 10 * MIN));
    assert_eq!(stopped.text.as_deref(), Some("You stopped it"));
    assert_eq!(stopped.full.as_deref(), Some("Half way."));

    let error = turn(End::Failed, Some("API Error: 529 overloaded. Try later."));
    let failed = refine(chat(IDLE), &ended, &watching(Some(&error), 0, None));
    assert_eq!(failed.mark, Mark::Failed);
    assert_eq!(failed.text.as_deref(), Some("API Error: 529 overloaded."));
    let bare = turn(End::Failed, None);
    let failed = refine(chat(IDLE), &ended, &watching(Some(&bare), 0, None));
    assert_eq!(failed.text.as_deref(), Some("The last request failed"));

    // A day old, an old stop is just an old idle chat.
    let cold = Mark::Idle(Age::Cold);
    assert_eq!(
        refine(chat(cold), &ended, &watching(Some(&stop), 0, None)).mark,
        cold
    );
    // Only an idle chat changes: a working one stays working.
    assert_eq!(
        refine(chat(Mark::Working), &ended, &watching(Some(&stop), 0, None)).mark,
        Mark::Working
    );
}

#[test]
fn the_line_says_what_a_chat_asks_or_does() {
    let mut card = turn(End::ToolCalled, Some("Two ways. Which one?"));
    card.tool = Some("AskUserQuestion".into());
    card.question = Some("Should the group open as a row or a fan?".into());
    assert_eq!(
        line_text(Mark::AsksYou, Some(&card)).as_deref(),
        Some("Should the group open as a row or a fan?")
    );
    assert_eq!(
        full_text(Mark::AsksYou, Some(&card)).as_deref(),
        Some("Should the group open as a row or a fan?")
    );
    let plain = turn(
        End::Replied,
        Some("I built it. Should I also add tests? Tell me."),
    );
    assert_eq!(
        line_text(Mark::AsksYou, Some(&plain)).as_deref(),
        Some("Should I also add tests?")
    );
    let statement = turn(End::Replied, Some("Please look at the page. Then reply."));
    assert_eq!(
        line_text(Mark::AsksYou, Some(&statement)).as_deref(),
        Some("Please look at the page.")
    );

    let mut running = turn(End::ToolCalled, Some("Let me check."));
    running.tool = Some("Bash".into());
    assert_eq!(
        line_text(Mark::Working, Some(&running)).as_deref(),
        Some("Running a command")
    );
    assert_eq!(
        line_text(Mark::NeedsApproval, Some(&running)).as_deref(),
        Some("Waits for your OK to run a command")
    );
    assert_eq!(
        line_text(Mark::NeedsApproval, None).as_deref(),
        Some("Waits for your OK")
    );
    running.tool = Some("mcp__a1b2__notion-search".into());
    assert_eq!(
        line_text(Mark::Working, Some(&running)).as_deref(),
        Some("Using a connector")
    );
    let thinking = turn(End::ToolDone, None);
    assert_eq!(
        line_text(Mark::Working, Some(&thinking)).as_deref(),
        Some("Thinking")
    );
    // Not read yet: no guess.
    assert_eq!(line_text(Mark::Working, None), None);
    assert_eq!(line_text(IDLE, None), None);
    assert_eq!(
        line_text(Mark::Unknown, None).as_deref(),
        Some("WinCraft cannot read this chat's state")
    );
    assert_eq!(full_text(Mark::Unknown, Some(&plain)), None);
    let jobs = turn(End::Replied, Some("Started the build."));
    assert_eq!(
        line_text(Mark::Background, Some(&jobs)).as_deref(),
        Some("Last: Started the build.")
    );
}

#[test]
fn the_window_in_front_is_noted_from_the_first_look() {
    let windows = [
        ChatWindow {
            hwnd: 5,
            session_id: "local_a".into(),
            waiting_since: None,
            approval: false,
        },
        ChatWindow {
            hwnd: 6,
            session_id: "local_b".into(),
            waiting_since: None,
            approval: false,
        },
    ];
    let mut seen = Seen::default();
    assert_eq!(seen.evidence("local_a", None).watching_since, None);
    seen.note(&windows, 0, 100);
    seen.note(&windows, 5, 200);
    seen.note(&windows, 99, 300);
    let a = seen.evidence("local_a", None);
    assert_eq!(a.watching_since, Some(100));
    assert_eq!(a.looked_at, Some(200));
    assert_eq!(seen.evidence("local_b", None).looked_at, None);
}

#[test]
fn archived_titles_are_read_only_for_a_missing_parent() {
    let mut fork = chat(Mark::Unknown);
    fork.id = "b".into();
    fork.parent = Some("local_a".into());
    assert!(data::archived(&[chat(Mark::Unknown), fork]).is_empty());
}

#[test]
fn the_hover_panel_names_the_chat_its_place_and_status() {
    use super::super::hover::{head_line, place_line};
    let mut asks = chat(Mark::AsksYou);
    asks.since = Some(NOW - 4 * MIN);
    assert_eq!(head_line(&asks, NOW), "A chat \u{b7} asks you \u{b7} 4 min");
    assert_eq!(place_line(&asks), "New");
    asks.folder = Some(r"E:\code\WinCraft".into());
    asks.branch = Some("design".into());
    assert_eq!(place_line(&asks), "WinCraft \u{b7} design");
}

#[test]
fn an_open_turn_in_the_transcript_means_working_even_when_the_file_says_idle() {
    let fresh = Mark::Idle(Age::Fresh);
    let now_working = session(MIN);
    for end in [End::ToolCalled, End::ToolDone, End::UserMessage] {
        let open = turn(end, Some("Both fixes are pushed."));
        let evidence = Evidence {
            turn: Some(&open),
            ..Evidence::default()
        };
        let got = refine(chat(fresh), &now_working, &evidence);
        assert_eq!(got.mark, Mark::Working, "{end:?}");
        assert_eq!(got.since, now_working.latest_user);
    }
    let replied = turn(End::Replied, Some("Done."));
    let evidence = Evidence {
        turn: Some(&replied),
        ..Evidence::default()
    };
    assert_eq!(refine(chat(fresh), &now_working, &evidence).mark, fresh);
    let cut_off = turn(End::ToolCalled, None);
    let evidence = Evidence {
        turn: Some(&cut_off),
        ..Evidence::default()
    };
    let old = Mark::Idle(Age::Cooling);
    assert_eq!(
        refine(chat(old), &session(120 * MIN), &evidence).mark,
        old,
        "a turn cut off hours ago is not working"
    );
}
