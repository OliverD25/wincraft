use std::collections::HashSet;

use super::labels::*;
use super::model::*;
use crate::core::claude_sessions::{Age, Lineage, Session};
use crate::core::claude_windows::ChatWindow;

const MIN: u64 = 60_000;
const NOW: u64 = 100 * DAY_MS;

fn chat(id: &str, folder: Option<&str>, created: u64) -> Chat {
    Chat {
        id: id.to_string(),
        title: format!("Chat {id}"),
        folder: folder.map(str::to_string),
        branch: Some("main".into()),
        model: None,
        parent: None,
        created,
        mark: Mark::Idle(Age::Fresh),
        since: Some(NOW - MIN),
        jobs: 0,
        window: None,
        text: None,
        full: None,
    }
}

fn fork(id: &str, parent: &str, created: u64) -> Chat {
    Chat {
        parent: Some(parent.to_string()),
        ..chat(id, Some(r"E:\code\WinCraft"), created)
    }
}

fn cold(id: &str, folder: Option<&str>, days: u64) -> Chat {
    Chat {
        mark: Mark::Idle(Age::Cold),
        since: Some(NOW - days * DAY_MS),
        ..chat(id, folder, days)
    }
}

/// The lines as short words: "G name count", "C id depth", "S title",
/// "~ count".
fn shape(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            Line::Group(g) => format!("G {} {}", g.name, g.chats),
            Line::Chat(c) if c.stand_in => format!("S {}", c.chat.title),
            Line::Chat(c) => format!("C {} {}", c.chat.id, c.depth),
            Line::Cold(c) => format!("~ {}", c.chats),
        })
        .collect()
}

fn chat_line<'a>(lines: &'a [Line], id: &str) -> &'a ChatLine {
    lines
        .iter()
        .find_map(|line| match line {
            Line::Chat(c) if c.chat.id == id => Some(c),
            _ => None,
        })
        .expect("listed")
}

fn session(id: &str) -> Session {
    Session {
        id: id.to_string(),
        title: "A chat".into(),
        latest_user: Some(NOW - 10 * MIN),
        last_turn_edge: Some(NOW - 5 * MIN),
        last_activity: Some(NOW - 5 * MIN),
        lineage: Lineage {
            origin_cwd: Some(r"E:\code\WinCraft".into()),
            cwd: Some(r"E:\code\WinCraft\.claude\worktrees\wt".into()),
            branch: Some("claude/wt".into()),
            model: Some("claude-opus-5-5".into()),
            forked_from: Some("local_p".into()),
            created: Some(42),
            ..Lineage::default()
        },
        ..Session::default()
    }
}

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
    let idle = super::model::chat(&session("local_a"), None, (0, None), NOW);
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
    let asks = super::model::chat(&session("local_a"), Some(&window), (0, None), NOW);
    assert_eq!(asks.mark, Mark::AsksYou);
    assert_eq!(asks.since, Some(NOW - 2 * MIN));
    assert_eq!(asks.window, Some(9));
    let prompt = ChatWindow {
        approval: true,
        ..window.clone()
    };
    let approve = super::model::chat(&session("local_a"), Some(&prompt), (0, None), NOW);
    assert_eq!(approve.mark, Mark::NeedsApproval);
    assert_eq!(approve.since, Some(NOW - 2 * MIN));
    // A stale approval flag without a waiting mark means nothing.
    let stale = ChatWindow {
        waiting_since: None,
        ..prompt
    };
    let idle = super::model::chat(&session("local_a"), Some(&stale), (0, None), NOW);
    assert!(matches!(idle.mark, Mark::Idle(_)));

    let background = super::model::chat(&session("local_a"), None, (2, Some(NOW - 3 * MIN)), NOW);
    assert_eq!(background.mark, Mark::Background);
    assert_eq!(background.jobs, 2);
    assert_eq!(background.since, Some(NOW - 3 * MIN));

    let mut busy = session("local_a");
    busy.latest_user = Some(NOW - MIN);
    busy.last_turn_edge = Some(NOW - MIN);
    let working = super::model::chat(&busy, None, (0, None), NOW);
    assert_eq!(working.mark, Mark::Working);
    assert_eq!(working.since, Some(NOW - MIN));

    let mut old = session("local_a");
    old.last_activity = Some(NOW - 2 * DAY_MS);
    assert_eq!(
        super::model::chat(&old, None, (0, None), NOW).mark,
        Mark::Idle(Age::Cold)
    );
    old.last_activity = None;
    assert_eq!(
        super::model::chat(&old, None, (0, None), NOW).mark,
        Mark::Idle(Age::Fresh)
    );

    let unknown = super::model::chat(&Session::default(), None, (0, None), NOW);
    assert_eq!(unknown.mark, Mark::Unknown);
    assert_eq!(unknown.since, None);
    assert_eq!(unknown.folder, None);
    assert_eq!(unknown.created, u64::MAX);

    let mut unloaded = Session {
        last_activity: Some(NOW - 3 * 60 * MIN),
        ..Session::default()
    };
    let quiet = super::model::chat(&unloaded, None, (0, None), NOW);
    assert_eq!(quiet.mark, Mark::Idle(Age::Cooling));
    assert_eq!(quiet.since, unloaded.last_activity);
    unloaded.last_activity = Some(NOW - 10 * MIN);
    let recent = super::model::chat(&unloaded, None, (0, None), NOW);
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
fn groups_go_new_first_then_by_name_and_chats_by_creation() {
    let chats = vec![
        chat("z", Some(r"E:\b\zeta"), 1),
        chat("a2", Some(r"E:\a\Alpha"), 5),
        chat("a1", Some(r"E:\a\alpha"), 9),
        chat("n", None, 3),
        chat("a0", Some(r"E:\a\Alpha"), 2),
    ];
    let got = lines(&chats, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        shape(&got),
        [
            "G New 1",
            "C n 0",
            "G Alpha 3",
            "C a0 0",
            "C a2 0",
            "C a1 0",
            "G zeta 1",
            "C z 0"
        ]
    );
}

#[test]
fn forks_hang_under_their_chat_with_tree_lines() {
    let chats = vec![
        chat("rust", Some(r"E:\code\WinCraft"), 1),
        fork("deck", "rust", 30),
        fork("design", "rust", 20),
        fork("coder", "design", 25),
    ];
    let got = lines(&chats, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        shape(&got),
        [
            "G WinCraft 4",
            "C rust 0",
            "C design 1",
            "C coder 2",
            "C deck 1"
        ]
    );
    let design = chat_line(&got, "design");
    assert_eq!(design.fold, Fold::Open);
    assert!(!design.last);
    assert!(design.rails.is_empty());
    let coder = chat_line(&got, "coder");
    assert!(coder.last);
    assert_eq!(coder.rails, vec![true], "design's line goes on to deck");
    let deck = chat_line(&got, "deck");
    assert!(deck.last);
    assert_eq!(deck.fold, Fold::None);
    assert_eq!(chat_line(&got, "rust").fold, Fold::Open);
}

#[test]
fn a_folded_chat_shows_its_forks_most_urgent_status() {
    let mut chats = vec![
        Chat {
            mark: Mark::Working,
            ..chat("rust", Some(r"E:\code\WinCraft"), 1)
        },
        fork("design", "rust", 20),
        fork("coder", "design", 25),
    ];
    chats[2].mark = Mark::AsksYou;
    chats[2].since = Some(NOW - 7 * MIN);
    let view = View {
        folded_chats: HashSet::from(["rust".to_string()]),
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G WinCraft 3", "C rust 0"]);
    let rust = chat_line(&got, "rust");
    assert_eq!(rust.fold, Fold::Folded(2));
    assert_eq!(rust.chat.mark, Mark::AsksYou);
    assert_eq!(rust.chat.since, Some(NOW - 7 * MIN));
}

#[test]
fn a_folded_group_shows_its_count_and_most_urgent_status() {
    let mut chats = vec![
        chat("a", Some(r"E:\NewsHQ"), 1),
        chat("b", Some(r"E:\NewsHQ"), 2),
    ];
    chats[1].mark = Mark::Background;
    let view = View {
        folded_groups: HashSet::from([r"e:\newshq".to_string()]),
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G NewsHQ 2"]);
    let Line::Group(group) = &got[0] else {
        panic!("a group");
    };
    assert!(group.folded);
    assert_eq!(group.roll, Mark::Background);
}

#[test]
fn a_fork_of_an_archived_or_missing_chat_keeps_a_stand_in() {
    let chats = vec![
        fork("deck", "gone", 30),
        fork("other", "gone", 10),
        fork("lost", "never", 40),
    ];
    let archived = Archived::from([("gone".to_string(), "WinCraft Rust".to_string())]);
    let got = lines(&chats, &archived, &View::default(), NOW);
    assert_eq!(
        shape(&got),
        [
            "G WinCraft 3",
            "S WinCraft Rust \u{b7} archived",
            "C other 1",
            "C deck 1",
            "S Chat not found",
            "C lost 1",
        ]
    );
    assert!(chat_line(&got, "deck").last);
    assert!(!chat_line(&got, "other").last);

    let view = View {
        folded_chats: HashSet::from(["gone".to_string()]),
        ..View::default()
    };
    let mut urgent = chats.clone();
    urgent[0].mark = Mark::Working;
    let got = lines(&urgent, &archived, &view, NOW);
    let stand_in = chat_line(&got, "gone");
    assert!(stand_in.stand_in);
    assert_eq!(stand_in.fold, Fold::Folded(2));
    assert_eq!(stand_in.chat.mark, Mark::Working);
}

#[test]
fn a_chat_that_names_itself_or_a_loop_still_shows_once() {
    let mut own = chat("self", Some(r"E:\x"), 1);
    own.parent = Some("self".into());
    let mut a = chat("a", Some(r"E:\x"), 2);
    a.parent = Some("b".into());
    let mut b = chat("b", Some(r"E:\x"), 3);
    b.parent = Some("a".into());
    let got = lines(&[own, a, b], &Archived::new(), &View::default(), NOW);
    let ids: Vec<String> = shape(&got);
    assert_eq!(ids.iter().filter(|line| line.starts_with("C ")).count(), 3);
    assert_eq!(ids[0], "G x 3");
}

#[test]
fn only_open_windows_keeps_the_chats_above_an_open_one() {
    let mut chats = vec![
        chat("rust", Some(r"E:\code\WinCraft"), 1),
        fork("design", "rust", 20),
        fork("deck", "rust", 30),
        chat("other", Some(r"E:\code\Other"), 2),
    ];
    chats[1].window = Some(5);
    let view = View {
        only_open: true,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(shape(&got), ["G WinCraft 2", "C rust 0", "C design 1"]);
    assert!(chat_line(&got, "design").last);
    assert_eq!(counts(&chats, true), vec![]);
    chats[1].mark = Mark::Working;
    chats[3].mark = Mark::Working;
    assert_eq!(counts(&chats, true), vec![(Mark::Working, 1)]);
    assert_eq!(counts(&chats, false), vec![(Mark::Working, 2)]);
}

#[test]
fn chats_idle_over_a_day_fold_into_one_line_at_the_end() {
    let mut chats = vec![
        chat("live", Some(r"E:\a"), 1),
        cold("old1", Some(r"E:\a"), 1),
        cold("old9", Some(r"E:\b"), 9),
        cold("parent", Some(r"E:\a"), 4),
        fork("kid", "parent", 50),
    ];
    chats[4].folder = Some(r"E:\a".into());
    let view = View {
        fold_cold: true,
        ..View::default()
    };
    let got = lines(&chats, &Archived::new(), &view, NOW);
    assert_eq!(
        shape(&got),
        ["G a 3", "C live 0", "C parent 0", "C kid 1", "~ 2"],
        "a cold chat with a fresh fork stays"
    );
    let Some(Line::Cold(line)) = got.last() else {
        panic!("the cold line");
    };
    assert_eq!(line.days, (1, 9));
    assert!(!line.open);

    let open = View {
        cold_open: true,
        ..view.clone()
    };
    let got = lines(&chats, &Archived::new(), &open, NOW);
    assert_eq!(&shape(&got)[4..], ["~ 2", "C old1 0", "C old9 0"]);

    let off = View::default();
    let got = lines(&chats, &Archived::new(), &off, NOW);
    assert!(!shape(&got).iter().any(|line| line.starts_with('~')));
}

#[test]
fn the_header_counts_only_the_marks_it_shows_in_its_order() {
    let mut chats: Vec<Chat> = (0..5).map(|n| chat(&n.to_string(), None, n)).collect();
    chats[0].mark = Mark::DoneNotSeen;
    chats[1].mark = Mark::Working;
    chats[2].mark = Mark::NeedsApproval;
    chats[3].mark = Mark::Working;
    chats[4].mark = Mark::Failed;
    assert_eq!(
        counts(&chats, false),
        vec![
            (Mark::NeedsApproval, 1),
            (Mark::Working, 2),
            (Mark::DoneNotSeen, 1)
        ]
    );
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

#[test]
fn settings_read_with_defaults_for_what_is_missing() {
    use super::{MarkStyle, Settings};
    let defaults = Settings::read(&serde_json::json!({}));
    assert_eq!(defaults, Settings::default());
    assert!(defaults.stay_on_top && defaults.fold_cold && defaults.time);
    assert!(!defaults.only_open && !defaults.model);
    assert_eq!(defaults.mark, MarkStyle::Shapes);
    let set = Settings::read(&serde_json::json!({
        "stay_on_top": false,
        "only_open_windows": true,
        "status_mark": "Shapes and words",
        "show_model": true,
        "show_jobs": "yes",
    }));
    assert!(!set.stay_on_top && set.only_open && set.model && set.jobs);
    assert_eq!(set.mark, MarkStyle::ShapesAndWords);
    let off = Settings::read(&serde_json::json!({ "status_mark": "Off" }));
    assert_eq!(off.mark, MarkStyle::Off);
}

#[test]
fn the_row_fields_sit_under_on_each_row() {
    use crate::core::traits::WinCraftPlugin;
    let deck = super::ClaudeDeck::new();
    for field in deck.settings_fields() {
        let row = matches!(
            field.key,
            "status_mark" | "show_text" | "show_time" | "show_project" | "show_jobs" | "show_model"
        );
        assert_eq!(
            deck.settings_group(field.key),
            row.then_some("On each row"),
            "{}",
            field.key
        );
    }
}

#[test]
fn only_a_listed_claude_window_is_brought_forward_and_never_by_a_test_copy() {
    let listed = [ChatWindow {
        hwnd: 42,
        session_id: "local_a".into(),
        waiting_since: None,
        approval: false,
    }];
    assert!(super::may_activate(42, &listed, false));
    assert!(!super::may_activate(42, &listed, true));
    assert!(!super::may_activate(43, &listed, false));
    assert!(!super::may_activate(0, &listed, false));
}

#[test]
fn the_place_is_kept_only_when_it_is_worth_it_and_reads_back() {
    use super::place::{load, save, worth_keeping, Place};
    let place = Place {
        x: 100,
        y: -20,
        width: 360,
        height: 480,
    };
    assert!(worth_keeping(false, true, place));
    assert!(!worth_keeping(true, true, place));
    assert!(!worth_keeping(false, false, place));
    let minimised = Place {
        x: -32000,
        y: -32000,
        ..place
    };
    assert!(!worth_keeping(false, true, minimised));
    let tiny = Place { width: 10, ..place };
    assert!(!worth_keeping(false, true, tiny));

    let folder = std::env::temp_dir().join(format!("wincraft-claude-deck-{}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    let path = folder.join("place.json");
    assert_eq!(load(&path), None);
    save(&path, place);
    assert_eq!(load(&path), Some(place));
    std::fs::write(&path, r#"{"x":1,"y":2,"width":0,"height":5}"#).expect("written");
    assert_eq!(load(&path), None);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_folded_chat_says_when_a_fork_is_the_most_urgent() {
    let mut chats = vec![
        chat("rust", Some(r"E:\code\WinCraft"), 1),
        fork("design", "rust", 20),
    ];
    chats[0].text = Some("Last: Done.".into());
    chats[1].mark = Mark::NeedsApproval;
    chats[1].text = Some("Waits for your OK to run a command".into());
    let view = View {
        folded_chats: HashSet::from(["rust".to_string()]),
        ..View::default()
    };
    let lines = lines(&chats, &Archived::new(), &view, NOW);
    let rust = chat_line(&lines, "rust");
    assert_eq!(rust.chat.mark, Mark::NeedsApproval);
    assert_eq!(
        rust.chat.text.as_deref(),
        Some("A fork needs approval: Waits for your OK to run a command")
    );
    // Open, each keeps its own line.
    let lines = super::model::lines(&chats, &Archived::new(), &View::default(), NOW);
    assert_eq!(
        chat_line(&lines, "rust").chat.text.as_deref(),
        Some("Last: Done.")
    );
}
