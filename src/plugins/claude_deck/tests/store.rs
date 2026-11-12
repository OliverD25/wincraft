//! The file that keeps the user's arrangement: read, written, and kept
//! safe when it is broken or from a newer WinCraft.

use super::*;
use crate::plugins::claude_deck::store::{load, save, Loaded};

fn scratch(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "wincraft-claude-deck-store-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn sample() -> Saved {
    Saved {
        groups: vec![Group {
            id: "g1".into(),
            name: "Work".into(),
            colour: Some(Colour::Green),
            folded: true,
            folder: Some(r"e:\code\wincraft".into()),
            keep: true,
            chats: vec!["a".into(), "b".into()],
        }],
        hidden: BTreeSet::from(["c".to_string()]),
        pinned: BTreeSet::from(["a".to_string()]),
        cold_open: true,
        ..Saved::default()
    }
}

#[test]
fn no_file_is_the_first_run_and_a_saved_list_reads_back() {
    let folder = scratch("roundtrip");
    let path = folder.join("claude_deck.list.json");
    assert_eq!(load(&path), Loaded::Missing);
    assert_eq!(Loaded::Missing.start(), (Saved::default(), true));
    save(&path, &sample());
    let Loaded::Read(read) = load(&path) else {
        panic!("read back");
    };
    assert_eq!(read.version, VERSION);
    assert_eq!(read.groups, sample().groups);
    assert_eq!(read.hidden, sample().hidden);
    assert!(read.cold_open);
    assert!(!folder.join("claude_deck.list.json.tmp").exists());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_broken_file_is_kept_aside_and_never_overwritten() {
    let folder = scratch("broken");
    let path = folder.join("claude_deck.list.json");
    std::fs::write(&path, "{ not json").expect("written");
    let aside = folder.join("claude_deck.list.json.bad");
    assert_eq!(load(&path), Loaded::Broken(Some(aside.clone())));
    assert!(!path.exists());
    assert_eq!(std::fs::read_to_string(&aside).expect("kept"), "{ not json");
    // A second broken file does not replace the first one kept aside.
    std::fs::write(&path, "[1").expect("written");
    let second = folder.join("claude_deck.list.json.bad.1");
    assert_eq!(load(&path), Loaded::Broken(Some(second.clone())));
    assert_eq!(std::fs::read_to_string(&aside).expect("kept"), "{ not json");
    assert_eq!(std::fs::read_to_string(&second).expect("kept"), "[1");
    // Starting over is allowed, because the old file is safe.
    assert_eq!(
        Loaded::Broken(Some(aside)).start(),
        (Saved::default(), true)
    );
    assert_eq!(Loaded::Broken(None).start(), (Saved::default(), false));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_file_from_a_newer_wincraft_is_used_but_not_written() {
    let folder = scratch("newer");
    let path = folder.join("claude_deck.list.json");
    std::fs::write(
        &path,
        r#"{"version": 99, "groups": [{"id": "g1", "name": "Later"}], "more": 1}"#,
    )
    .expect("written");
    let loaded = load(&path);
    let Loaded::Newer(saved) = &loaded else {
        panic!("newer");
    };
    assert_eq!(saved.groups[0].name, "Later");
    let (_, writable) = loaded.start();
    assert!(!writable);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn missing_fields_take_their_defaults() {
    let saved: Saved = serde_json::from_str(r#"{"groups": [{"id": "g1"}]}"#).expect("read");
    assert_eq!(saved.groups[0].chats, Vec::<String>::new());
    assert_eq!(saved.groups[0].colour, None);
    assert!(!saved.cold_open);
    let text = serde_json::to_string(&Colour::Yellow).expect("written");
    assert_eq!(text, "\"yellow\"");
}
