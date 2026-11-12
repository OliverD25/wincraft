//! The walks on made-up trees. No UI Automation and no window is used.

use super::*;

struct Item {
    name: &'static str,
    kind: Kind,
    children: Vec<usize>,
}

struct Fake(Vec<Item>);

impl Tree for Fake {
    type Node = usize;

    fn children(&self, node: &usize) -> Vec<usize> {
        self.0[*node].children.clone()
    }

    fn name(&self, node: &usize) -> Option<String> {
        Some(self.0[*node].name.to_string())
    }

    fn kind(&self, node: &usize) -> Kind {
        self.0[*node].kind
    }
}

fn item(name: &'static str, kind: Kind, children: &[usize]) -> Item {
    Item {
        name,
        kind,
        children: children.to_vec(),
    }
}

/// 0 window; 1 the empty Sidebar group; 2 a list group; then the given
/// leaves under the list group. As the live app: the chat buttons are not
/// inside the Sidebar group.
fn home(leaves: &[(&'static str, Kind)]) -> Fake {
    let mut items = vec![
        item("Claude Home", Kind::Other, &[1, 2]),
        item(SIDEBAR, Kind::Group, &[]),
        item("", Kind::Group, &[]),
    ];
    for (name, kind) in leaves {
        let id = items.len();
        items[2].children.push(id);
        items.push(item(name, *kind, &[]));
    }
    Fake(items)
}

#[test]
fn a_button_outside_the_sidebar_group_is_found_by_its_exact_title() {
    let tree = home(&[
        ("Other chat", Kind::Button),
        ("WinCraft engine", Kind::Button),
    ]);
    let found = find_button(&tree, &0, "  WinCraft engine ").expect("a match");
    assert_eq!(found.node, 4);
    assert_eq!(found.matches, 1);
    assert!(find_button(&tree, &0, "WinCraft").is_none(), "not a prefix");
}

#[test]
fn a_name_on_something_that_is_not_a_button_is_ignored() {
    let tree = home(&[("Same", Kind::Other), ("Same", Kind::Group)]);
    assert!(find_button(&tree, &0, "Same").is_none());
}

#[test]
fn of_two_buttons_with_one_title_the_first_is_used() {
    let tree = home(&[("Twin", Kind::Button), ("Twin", Kind::Button)]);
    let found = find_button(&tree, &0, "Twin").unwrap();
    assert_eq!(found.node, 3);
    assert_eq!(found.matches, 2);
}

#[test]
fn a_missing_title_gives_none() {
    let tree = home(&[("A", Kind::Button)]);
    assert!(find_button(&tree, &0, "B").is_none());
}

/// A chain of groups `depth` deep, a button at the end.
fn chain(depth: usize) -> Fake {
    let mut items = vec![item("root", Kind::Group, &[1])];
    for level in 1..depth {
        items.push(item("g", Kind::Group, &[level + 1]));
    }
    items.push(item("Deep", Kind::Button, &[]));
    Fake(items)
}

#[test]
fn the_walk_stops_at_its_depth_limit() {
    assert!(find_button(&chain(DEPTH), &0, "Deep").is_some());
    assert!(find_button(&chain(DEPTH + 1), &0, "Deep").is_none());
}

#[test]
fn the_walk_stops_at_its_item_limit() {
    let mut items = vec![item("root", Kind::Group, &[])];
    for n in 0..ITEMS + 10 {
        items[0].children.push(n + 1);
        let name = if n == ITEMS + 5 { "Late" } else { "x" };
        items.push(item(name, Kind::Button, &[]));
    }
    assert!(find_button(&Fake(items), &0, "Late").is_none());
}

#[test]
fn a_menu_item_is_found_by_its_exact_name_among_menu_items() {
    let tree = home(&[
        ("Archive", Kind::Button),
        ("Pin", Kind::MenuItem),
        ("Archive", Kind::MenuItem),
        ("Delete", Kind::MenuItem),
    ]);
    let found = find_named(&tree, &0, Kind::MenuItem, "Archive").unwrap();
    assert_eq!(found.node, 5);
    assert!(find_named(&tree, &0, Kind::MenuItem, "Arch").is_none());
    assert!(find_named(&tree, &0, Kind::MenuItem, "Rename").is_none());
}

#[test]
fn the_first_edit_field_is_found() {
    let tree = home(&[("A", Kind::Button), ("", Kind::Edit)]);
    assert_eq!(find_edit(&tree, &0).unwrap().node, 4);
    assert!(find_edit(&home(&[("A", Kind::Button)]), &0).is_none());
}

#[test]
fn the_home_window_has_the_sidebar_group_or_the_show_button() {
    assert!(is_home(&home(&[]), &0));
    let narrow = Fake(vec![
        item("w", Kind::Other, &[1]),
        item(SHOW_SIDEBAR, Kind::Button, &[]),
    ]);
    assert!(is_home(&narrow, &0));
    let popped_out = Fake(vec![item("Chat", Kind::Other, &[])]);
    assert!(!is_home(&popped_out, &0));
    let not_a_group = Fake(vec![
        item("w", Kind::Other, &[1]),
        item(SIDEBAR, Kind::Other, &[]),
    ]);
    assert!(!is_home(&not_a_group, &0));
}

#[test]
fn the_notice_names_claudedeck_and_the_chat() {
    assert_eq!(
        not_found_notice("My chat"),
        "ClaudeDeck could not find \"My chat\" in the Claude app's list. Open it there."
    );
}

#[test]
fn unit_tests_start_nothing() {
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    open_chat("Any chat", move |text| {
        let _ = sender.send(text);
    });
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn the_archive_and_rename_notices_name_claudedeck_and_the_chat() {
    use super::menu::{archive_failed_notice, archived_notice, rename_failed_notice};
    assert_eq!(
        archived_notice("My chat"),
        "ClaudeDeck archived \"My chat\". Find it under Archived in the Claude app."
    );
    assert_eq!(
        archive_failed_notice("My chat"),
        "ClaudeDeck could not archive \"My chat\". Archive it in the Claude app."
    );
    assert_eq!(
        rename_failed_notice("My chat"),
        "ClaudeDeck could not rename \"My chat\". Rename it in the Claude app."
    );
}

#[test]
fn only_archive_and_rename_have_a_fallback_letter_and_never_delete() {
    use super::keys::{menu_letter, KEY_A, KEY_R};
    assert_eq!(menu_letter("Archive"), Some(KEY_A));
    assert_eq!(menu_letter("Rename"), Some(KEY_R));
    assert_eq!(menu_letter("Delete"), None);
    assert_eq!(menu_letter("Pin"), None);
}

#[test]
fn a_chord_goes_down_in_order_and_up_in_reverse() {
    use super::keys::{chord_inputs, F10, SHIFT};
    let inputs = chord_inputs(&[SHIFT, F10]);
    let keys: Vec<(u16, bool)> = inputs
        .iter()
        .map(|input| unsafe { (input.Anonymous.ki.wVk, input.Anonymous.ki.dwFlags != 0) })
        .collect();
    assert_eq!(
        keys,
        [(SHIFT, false), (F10, false), (F10, true), (SHIFT, true)]
    );
}

#[test]
fn archiving_and_renaming_start_nothing_in_unit_tests() {
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let again = sender.clone();
    archive_chat("Any chat", move |text| {
        let _ = sender.send(text);
    });
    rename_chat("Any chat", "New", move |text| {
        let _ = again.send(text);
    });
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
}
