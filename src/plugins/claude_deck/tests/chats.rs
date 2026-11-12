//! A session becomes a chat: its mark, its word, its folder, its labels.

use super::*;

#[test]
fn urgency_runs_from_approval_to_unknown() {
    let order = [
        Mark::NeedsApproval,
        Mark::AsksYou,
        Mark::DoneNotSeen,
        Mark::Working,
        Mark::Background,
        Mark::Idle(Age::Frozen),
        Mark::Stopped,
        Mark::Failed,
        Mark::Unknown,
    ];
    for pair in order.windows(2) {
        assert!(pair[0].rank() < pair[1].rank(), "{:?}", pair);
    }
    assert_eq!(Mark::Idle(Age::Fresh).rank(), Mark::Idle(Age::Cold).rank());
}

#[test]
fn each_mark_has_its_word_and_the_ones_that_want_you_are_bold() {
    let words: Vec<&str> = [
        Mark::NeedsApproval,
        Mark::AsksYou,
        Mark::DoneNotSeen,
        Mark::Working,
        Mark::Background,
        Mark::Idle(Age::Cooling),
        Mark::Stopped,
        Mark::Failed,
        Mark::Unknown,
    ]
    .iter()
    .map(|mark| mark.word())
    .collect();
    assert_eq!(
        words,
        ["Approve", "Asks", "Done", "Working", "Bg", "Idle", "Stopped", "Failed", "Unknown"]
    );
    assert!(Mark::NeedsApproval.bold() && Mark::AsksYou.bold() && Mark::Failed.bold());
    assert!(!Mark::Working.bold() && !Mark::Idle(Age::Fresh).bold());
}

#[test]
fn a_session_becomes_a_chat_with_windowdecks_status() {
    let idle = crate::plugins::claude_deck::model::chat(&session("local_a"), None, (0, None), NOW);
    assert_eq!(idle.mark, Mark::Idle(Age::Fresh));
    assert_eq!(idle.since, Some(NOW - 5 * MIN));
    assert_eq!(idle.folder.as_deref(), Some(r"E:\code\WinCraft"));
    assert_eq!(idle.branch.as_deref(), Some("claude/wt"));
    assert_eq!(idle.parent.as_deref(), Some("local_p"));
    assert_eq!(idle.created, 42);
    assert_eq!(idle.window, None);

    let window = ChatWindow {
        hwnd: 9,
        session_id: "local_a".into(),
        waiting_since: Some(NOW - 2 * MIN),
        approval: false,
    };
    let asks = crate::plugins::claude_deck::model::chat(
        &session("local_a"),
        Some(&window),
        (0, None),
        NOW,
    );
    assert_eq!(asks.mark, Mark::AsksYou);
    assert_eq!(asks.since, Some(NOW - 2 * MIN));
    assert_eq!(asks.window, Some(9));
    let prompt = ChatWindow {
        approval: true,
        ..window.clone()
    };
    let approve = crate::plugins::claude_deck::model::chat(
        &session("local_a"),
        Some(&prompt),
        (0, None),
        NOW,
    );
    assert_eq!(approve.mark, Mark::NeedsApproval);
    assert_eq!(approve.since, Some(NOW - 2 * MIN));
    // A stale approval flag without a waiting mark means nothing.
    let stale = ChatWindow {
        waiting_since: None,
        ..prompt
    };
    let idle =
        crate::plugins::claude_deck::model::chat(&session("local_a"), Some(&stale), (0, None), NOW);
    assert!(matches!(idle.mark, Mark::Idle(_)));

    let background = crate::plugins::claude_deck::model::chat(
        &session("local_a"),
        None,
        (2, Some(NOW - 3 * MIN)),
        NOW,
    );
    assert_eq!(background.mark, Mark::Background);
    assert_eq!(background.jobs, 2);
    assert_eq!(background.since, Some(NOW - 3 * MIN));

    let mut busy = session("local_a");
    busy.latest_user = Some(NOW - MIN);
    busy.last_turn_edge = Some(NOW - MIN);
    let working = crate::plugins::claude_deck::model::chat(&busy, None, (0, None), NOW);
    assert_eq!(working.mark, Mark::Working);
    assert_eq!(working.since, Some(NOW - MIN));

    let mut old = session("local_a");
    old.last_activity = Some(NOW - 2 * DAY_MS);
    assert_eq!(
        crate::plugins::claude_deck::model::chat(&old, None, (0, None), NOW).mark,
        Mark::Idle(Age::Cold)
    );
    old.last_activity = None;
    assert_eq!(
        crate::plugins::claude_deck::model::chat(&old, None, (0, None), NOW).mark,
        Mark::Idle(Age::Fresh)
    );

    let unknown =
        crate::plugins::claude_deck::model::chat(&Session::default(), None, (0, None), NOW);
    assert_eq!(unknown.mark, Mark::Unknown);
    assert_eq!(unknown.since, None);
    assert_eq!(unknown.folder, None);
    assert_eq!(unknown.created, u64::MAX);

    let mut unloaded = Session {
        last_activity: Some(NOW - 3 * 60 * MIN),
        ..Session::default()
    };
    let quiet = crate::plugins::claude_deck::model::chat(&unloaded, None, (0, None), NOW);
    assert_eq!(quiet.mark, Mark::Idle(Age::Cooling));
    assert_eq!(quiet.since, unloaded.last_activity);
    unloaded.last_activity = Some(NOW - 10 * MIN);
    let recent = crate::plugins::claude_deck::model::chat(&unloaded, None, (0, None), NOW);
    assert_eq!(recent.mark, Mark::Unknown);
}

#[test]
fn worktrees_count_as_their_repository() {
    assert_eq!(
        project_folder(r"E:\code\WinCraft\.claude\worktrees\cranky"),
        Some(r"E:\code\WinCraft".to_string())
    );
    assert_eq!(
        project_folder("E:/code/WinCraft/.Claude/Worktrees/x/"),
        Some(r"E:\code\WinCraft".to_string())
    );
    assert_eq!(project_folder(r" C:\work\ "), Some(r"C:\work".to_string()));
    assert_eq!(project_folder("   "), None);
    assert_eq!(group_name(r"E:\code\NewsHQ"), "NewsHQ");
    assert_eq!(group_name(r"C:\"), r"C:");
    assert_eq!(group_name("plain"), "plain");
}

#[test]
fn labels_say_one_or_many() {
    assert_eq!(cold_label(1), "1 cold chat");
    assert_eq!(cold_label(6), "6 cold chats");
    assert_eq!(jobs_label(1), "1 job");
    assert_eq!(jobs_label(2), "2 jobs");
    assert_eq!(forks_label(1), "1 fork");
    assert_eq!(forks_label(3), "3 forks");
    assert_eq!(chats_label(1), "1 chat");
    assert_eq!(chats_label(3), "3 chats");
    assert_eq!(days_label((1, 9)), "1 \u{2013} 9 days");
    assert_eq!(days_label((1, 1)), "1 day");
    assert_eq!(days_label((3, 3)), "3 days");
    assert_eq!(time_label(Some(NOW - 30_000), NOW), "30 s");
    assert_eq!(time_label(Some(NOW - 4 * MIN), NOW), "4 min");
    assert_eq!(time_label(None, NOW), "\u{2013}");
    assert_eq!(model_label("claude-opus-5-5"), "opus-5-5");
    assert_eq!(model_label("other"), "other");
    assert_eq!(
        stand_in_title(&Archived::new(), "x"),
        "Chat not found".to_string()
    );
}
