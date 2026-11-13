//! The tests of terminal forks: no terminal, ssh or claude is started.

use crate::core::claude_sessions::Age;
use crate::core::claude_turns::{End, Turn};
use crate::plugins::claude_deck::menu::{chat_items, Item};
use crate::plugins::claude_deck::model::{Chat, ChatLine, Fold, Mark};
use crate::plugins::claude_deck::servers::status::Parsed;

use super::*;

const NOW: u64 = 1_000_000_000;
const ID: &str = "11111111-2222-4333-8444-555555555555";

fn record(id: &str, server: Option<&str>, started: u64) -> Record {
    Record {
        id: id.to_string(),
        parent: "local_a".to_string(),
        title: "Fork of Parent".to_string(),
        folder: "E:/code/app".to_string(),
        server: server.map(str::to_string),
        started,
    }
}

fn parent() -> Chat {
    Chat {
        id: "local_a".into(),
        title: "Parent".into(),
        folder: Some("E:/code/app".into()),
        branch: None,
        model: None,
        parent: None,
        created: 1,
        mark: Mark::Idle(Age::Fresh),
        since: None,
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

fn title_of(record: &Record) -> String {
    window_title(&record.title, &record.id)
}

fn no_transcript(_: &str) -> Option<Transcript> {
    None
}

#[test]
fn a_new_id_is_a_version_4_uuid() {
    let id = uuid_from([0xff; 16]);
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "4", "version");
    assert!("89ab".contains(&id[19..20]), "variant");
    assert_eq!(id.matches('-').count(), 4);
    assert!(crate::plugins::claude_deck::fork::valid_id(&id));
    assert_eq!(uuid_from([0; 16]), "00000000-0000-4000-8000-000000000000");
}

#[test]
fn the_window_title_is_safe_and_short() {
    let launch = Launch::with_id(ID, "Say \"hi\"; it's `x`\nnow");
    let args = launch.wt_prefix();
    let title = args[3].clone();
    assert!(title.starts_with("Fork of Say hi its `x`now"), "{title}");
    assert!(title.ends_with(" \u{b7} 11111111"));
    assert!(!title.contains([';', '"', '\'', '\n']));
    let long = Launch::with_id(ID, &"x".repeat(200)).wt_prefix()[3].clone();
    assert!(long.chars().count() <= TITLE_MAX, "{long}");
    assert!(long.ends_with("11111111"), "two forks of one chat differ");
    assert_eq!(args[..3], ["-w", "new", "--title"]);
    assert_eq!(args[4], "--suppressApplicationTitle");
}

#[test]
fn the_records_are_capped_at_twenty_and_the_oldest_go() {
    let mut records = Vec::new();
    for n in 0..25u64 {
        records = add(records, record(&format!("id{n}"), None, 100 + n));
    }
    assert_eq!(records.len(), MAX_RECORDS);
    assert_eq!(records[0].id, "id5");
    assert_eq!(records[19].id, "id24");
}

#[test]
fn a_broken_or_missing_file_gives_no_records() {
    let dir = std::env::temp_dir().join(format!("wincraft-forks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("claude_deck.forks.json");
    assert!(load(&file).is_empty());
    std::fs::write(&file, "{ not json").unwrap();
    assert!(load(&file).is_empty());
    save(&file, &[record(ID, None, 5)]);
    assert_eq!(load(&file), [record(ID, None, 5)]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_fork_with_a_window_is_a_row_under_its_chat() {
    let record = record(ID, None, NOW - 5 * HOUR_MS / 10);
    let windows = vec![(77, title_of(&record))];
    let (shown, kept) = rows(&[record], &windows, &[parent()], NOW, &no_transcript);
    assert_eq!(kept.len(), 1);
    let fork = &shown[0];
    assert_eq!(fork.id, format!("fork/{ID}"));
    assert_eq!(fork.parent.as_deref(), Some("local_a"));
    assert_eq!(fork.window, Some(77));
    assert_eq!(fork.title, "Fork of Parent");
    assert_eq!(fork.terminal.as_deref(), Some("E:/code/app"));
}

#[test]
fn a_fork_with_no_listed_parent_stands_on_its_own() {
    let record = record(ID, None, NOW - HOUR_MS);
    let windows = vec![(77, title_of(&record))];
    let (shown, _) = rows(&[record], &windows, &[], NOW, &no_transcript);
    assert_eq!(shown[0].parent, None);
    assert_eq!(shown[0].folder.as_deref(), Some("E:\\code\\app"));
}

#[test]
fn a_fork_without_a_window_gives_no_row_and_the_record_goes() {
    let old = record("aaaa-1", None, NOW - HOUR_MS);
    let fresh = record("bbbb-2", None, NOW - 5_000);
    let (shown, kept) = rows(&[old, fresh.clone()], &[], &[parent()], NOW, &no_transcript);
    assert!(shown.is_empty());
    assert_eq!(kept, [fresh], "one just started keeps its record");
}

#[test]
fn a_fork_older_than_a_day_is_dropped_even_with_a_window() {
    let record = record(ID, None, 1);
    let windows = vec![(1, title_of(&record))];
    let (shown, kept) = rows(&[record], &windows, &[], 2 * KEEP_MS, &no_transcript);
    assert!(shown.is_empty() && kept.is_empty());
}

#[test]
fn the_row_takes_its_status_from_the_transcript() {
    let record = record(ID, None, NOW - HOUR_MS);
    let windows = vec![(1, title_of(&record))];
    let read = |_: &str| {
        Some(Transcript {
            parsed: Parsed {
                turn: Turn {
                    end: End::ToolCalled,
                    tool: Some("Bash".into()),
                    ..Turn::default()
                },
                cwd: None,
                title: Some("Named by Claude".into()),
                replies: Vec::new(),
            },
            quiet_ms: 4_000,
        })
    };
    let (shown, _) = rows(&[record], &windows, &[], NOW, &read);
    assert_eq!(shown[0].mark, Mark::Working);
    assert_eq!(shown[0].title, "Named by Claude");
    assert_eq!(shown[0].since, Some(NOW - 4_000));
}

#[test]
fn a_server_fork_says_where_it_runs() {
    let record = record(ID, Some("rdehome2"), NOW - 60_000);
    let windows = vec![(9, title_of(&record))];
    let (shown, _) = rows(&[record], &windows, &[parent()], NOW, &no_transcript);
    assert_eq!(
        shown[0].text.as_deref(),
        Some("Runs on home2 in a terminal")
    );
    assert_eq!(shown[0].parent, None);
    assert_eq!(shown[0].folder, None);
    assert_eq!(shown[0].terminal.as_deref(), Some("rdehome2:E:/code/app"));
}

#[test]
fn the_menu_of_a_terminal_fork_is_open_and_copy_only() {
    let record = record(ID, None, NOW - 60_000);
    let windows = vec![(9, title_of(&record))];
    let (shown, _) = rows(&[record], &windows, &[parent()], NOW, &no_transcript);
    let line = ChatLine {
        chat: shown[0].clone(),
        stand_in: false,
        depth: 1,
        rails: Vec::new(),
        last: true,
        fold: Fold::None,
        pinned: false,
        hidden: false,
    };
    assert_eq!(chat_items(&line, true), [Item::Open, Item::Copy]);
}
