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
