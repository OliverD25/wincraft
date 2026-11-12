use super::*;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

fn news() -> Vec<DeckGroup> {
    let mut groups = Vec::new();
    make(
        &mut groups,
        "chrome",
        "NewsHQ",
        &names(&["NewsHQ", "NewsHQ - My Tasks", "NewsHQ - design"]),
    );
    groups
}

#[test]
fn a_group_needs_a_name_and_two_windows() {
    let mut groups = Vec::new();
    assert_eq!(make(&mut groups, "chrome", "  ", &names(&["a", "b"])), None);
    assert_eq!(make(&mut groups, "chrome", "x", &names(&["a", "a"])), None);
    assert_eq!(
        make(&mut groups, "chrome", " Mail ", &names(&["a", "b", "a"])),
        Some("Mail".to_string())
    );
    assert_eq!(groups[0].members, names(&["a", "b"]));
}

#[test]
fn a_window_is_in_one_group_at_a_time() {
    let mut groups = news();
    make(
        &mut groups,
        "chrome",
        "Tasks",
        &names(&["NewsHQ - My Tasks", "Inbox"]),
    );
    assert_eq!(groups[0].members, names(&["NewsHQ", "NewsHQ - design"]));
    assert_eq!(
        group_of(&groups, "chrome", "NewsHQ - My Tasks").map(|g| g.name.as_str()),
        Some("Tasks")
    );
    // The same name in another app is another window.
    assert!(group_of(&groups, "edge", "Inbox").is_none());
}

#[test]
fn names_stay_unique_within_an_app() {
    let mut groups = news();
    let second = make(&mut groups, "chrome", "NewsHQ", &names(&["a", "b"]));
    assert_eq!(second.as_deref(), Some("NewsHQ 2"));
    let other_app = make(&mut groups, "edge", "NewsHQ", &names(&["a", "b"]));
    assert_eq!(other_app.as_deref(), Some("NewsHQ"));
    assert_eq!(
        rename(&mut groups, "chrome", "NewsHQ 2", "NewsHQ").as_deref(),
        Some("NewsHQ 2")
    );
    assert_eq!(groups[1].name, "NewsHQ 2", "its own name counts as free");
    // Its own name is not taken from itself.
    assert!(rename(&mut groups, "chrome", "NewsHQ", "NewsHQ").is_some());
    assert_eq!(groups[0].name, "NewsHQ");
    assert!(rename(&mut groups, "chrome", "NewsHQ", " ").is_none());
    assert!(rename(&mut groups, "chrome", "None such", "x").is_none());
}

#[test]
fn adding_and_taking_out_windows() {
    let mut groups = news();
    assert!(add(&mut groups, "chrome", "NewsHQ", "Inbox"));
    assert_eq!(groups[0].members.len(), 4);
    assert!(!add(&mut groups, "chrome", "Nothing", "Inbox"));
    remove(&mut groups, "chrome", "NewsHQ");
    assert!(!groups[0].has("NewsHQ"));
    // The last name out takes the group with it.
    let mut pair = Vec::new();
    make(&mut pair, "chrome", "P", &names(&["a", "b"]));
    remove(&mut pair, "chrome", "a");
    assert_eq!(pair.len(), 1);
    remove(&mut pair, "chrome", "b");
    assert!(pair.is_empty());
}

#[test]
fn ungroup_and_a_renamed_window() {
    let mut groups = news();
    rename_member(&mut groups, "chrome", "NewsHQ - design", "Design");
    assert!(groups[0].has("Design"));
    ungroup(&mut groups, "chrome", "NewsHQ");
    assert!(groups.is_empty());
}

#[test]
fn the_shared_start_ends_at_a_whole_word() {
    assert_eq!(
        shared_start(&["NewsHQ - My Tasks", "NewsHQ - design", "NewsHQ"]),
        "NewsHQ"
    );
    assert_eq!(shared_start(&["Report 2024", "Report 2025"]), "Report");
    assert_eq!(shared_start(&["Mail", "Maps"]), "");
    assert_eq!(shared_start(&["Same", "Same"]), "Same");
    assert_eq!(shared_start(&[]), "");
    assert_eq!(
        suggested_name(&["Mail", "Maps"], "Google Chrome"),
        "Google Chrome"
    );
    assert_eq!(
        suggested_name(&["Docs: plan", "Docs: notes"], "Chrome"),
        "Docs"
    );
}

#[test]
fn short_names_leave_the_group_name_out() {
    assert_eq!(short_name("NewsHQ - My Tasks", "NewsHQ"), "My Tasks");
    assert_eq!(
        short_name("NewsHQ - design: the news feed screen", "NewsHQ"),
        "design: the news feed screen"
    );
    assert_eq!(short_name("NewsHQ", "NewsHQ"), "NewsHQ");
    assert_eq!(short_name("Inbox", "NewsHQ"), "Inbox");
    assert_eq!(short_name("Привіт світ", "Привіт"), "світ");
}

// The user's screenshot of 0.20.91 showed "fork) - Custom WinDeck Idea":
// the shared start was cut inside "(fork)".
#[test]
fn names_are_cut_only_between_whole_words() {
    let names = [
        "WinCraft Rust",
        "WinCraft Rust (fork) - Design Session",
        "WinCraft Rust (fork) - Custom WinDeck Idea",
    ];
    assert_eq!(shared_start(&names), "WinCraft Rust");
    let group = suggested_name(&names, "Claude");
    assert_eq!(group, "WinCraft Rust");
    let short: Vec<String> = names.iter().map(|n| short_name(n, &group)).collect();
    assert_eq!(
        short,
        [
            "WinCraft Rust",
            "fork - Design Session",
            "fork - Custom WinDeck Idea"
        ]
    );
    assert_eq!(
        shared_start(&[names[1], names[2]]),
        "WinCraft Rust (fork)",
        "a closing bracket stays with its opening one"
    );
    assert_eq!(
        short_name(names[2], "WinCraft Rust (fork)"),
        "Custom WinDeck Idea"
    );
    assert_eq!(shared_start(&["Rust (fork", "Rust (fork"]), "Rust");
    assert_eq!(
        short_name("Rust (two words) left", "Rust"),
        "(two words) left",
        "a bracket of more than one word stays whole"
    );
    assert_eq!(short_name("Rust: [beta]", "Rust"), "beta");
    assert_eq!(short_name("Rust · notes", "Rust"), "notes");
}

#[test]
fn the_file_keeps_the_groups() {
    let groups = news();
    assert_eq!(parse(&to_json(&groups)).expect("reads back"), groups);
    assert!(parse("{").is_err());
    let dir = std::env::temp_dir().join(format!("wincraft-deck-groups-{}", std::process::id()));
    let path = dir.join("window_deck.groups.json");
    assert!(load(&path).is_empty());
    save(&path, &groups).expect("saved");
    assert_eq!(load(&path), groups);
    std::fs::write(&path, "not json").expect("written");
    assert!(load(&path).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_window_renamed_outside_the_deck_keeps_its_group() {
    let mut groups = news();
    assert!(follow_rename(
        &mut groups,
        "chrome",
        "NewsHQ - design",
        "NewsHQ - feed design"
    ));
    assert_eq!(
        groups[0].members,
        names(&["NewsHQ", "NewsHQ - My Tasks", "NewsHQ - feed design"])
    );
}

#[test]
fn a_rename_changes_nothing_when_it_cannot_tell_the_window() {
    let mut groups = news();
    let before = groups.clone();
    // The same name, an empty name, another app, a name not in a group, and
    // a new name another member already has.
    assert!(!follow_rename(&mut groups, "chrome", "NewsHQ", "NewsHQ"));
    assert!(!follow_rename(&mut groups, "chrome", "NewsHQ", "  "));
    assert!(!follow_rename(&mut groups, "edge", "NewsHQ", "Mail"));
    assert!(!follow_rename(&mut groups, "chrome", "Inbox", "Mail"));
    assert!(!follow_rename(
        &mut groups,
        "chrome",
        "NewsHQ",
        "NewsHQ - My Tasks"
    ));
    assert_eq!(groups, before);
}

const CLAUDE_OLD: &str = r"d:\windowsapps\claude_4.23118.0.0_x64__pzs8sxrjxfjjc\app\claude.exe";
const CLAUDE_NEW: &str =
    r"c:\program files\windowsapps\Claude_5.1231.2.0_x64__pzs8sxrjxfjjc\app\claude.exe";

#[test]
fn a_store_app_update_keeps_its_groups() {
    assert!(same_app(CLAUDE_OLD, CLAUDE_NEW));
    let mut groups = Vec::new();
    make(
        &mut groups,
        CLAUDE_OLD,
        "NewsHQ",
        &names(&["NewsHQ", "NewsHQ - My Tasks"]),
    );
    assert_eq!(
        group_of(&groups, CLAUDE_NEW, "NewsHQ").map(|g| g.name.as_str()),
        Some("NewsHQ")
    );
    // Another package, another program, or another publisher is another app.
    let other = r"c:\program files\windowsapps\notepad_11.2607.14.0_x64__8wekyb3d8bbwe\notepad.exe";
    assert!(!same_app(CLAUDE_NEW, other));
    assert!(!same_app(
        CLAUDE_NEW,
        r"c:\windowsapps\claude_5.0.0.0_x64__pzs8sxrjxfjjc\app\other.exe"
    ));
    assert!(!same_app(
        CLAUDE_NEW,
        r"c:\windowsapps\claude_5.0.0.0_x64__someoneelse\app\claude.exe"
    ));
    assert!(!same_app(r"c:\tools\a.exe", r"c:\tools\b.exe"));
}

#[test]
fn a_group_saved_before_and_after_an_update_is_kept_once() {
    let old = DeckGroup {
        app: CLAUDE_OLD.to_string(),
        name: "NewsHQ".to_string(),
        members: names(&["NewsHQ", "NewsHQ - My Tasks"]),
    };
    let other = DeckGroup {
        name: "WinCraft".to_string(),
        ..old.clone()
    };
    let new = DeckGroup {
        app: CLAUDE_NEW.to_string(),
        members: names(&["NewsHQ", "NewsHQ - design"]),
        ..old.clone()
    };
    let kept = merge_updated(vec![old, other.clone(), new.clone()]);
    assert_eq!(kept, vec![other, new]);
}
