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

/// 0 window; 1 sidebar group (children 2..); then buttons and others.
fn sidebar_tree(buttons: &[(&'static str, Kind)]) -> Fake {
    let mut items = vec![
        item("Claude", Kind::Other, &[1]),
        item(SIDEBAR, Kind::Group, &[]),
    ];
    for (name, kind) in buttons {
        let id = items.len();
        items[1].children.push(id);
        items.push(item(name, *kind, &[]));
    }
    Fake(items)
}

#[test]
fn the_button_is_found_by_its_exact_title() {
    let tree = sidebar_tree(&[
        ("Other chat", Kind::Button),
        ("WinCraft engine", Kind::Button),
    ]);
    let found = find_button(&tree, &1, "  WinCraft engine ").expect("a match");
    assert_eq!(found.node, 3);
    assert_eq!(found.matches, 1);
    assert!(find_button(&tree, &1, "WinCraft").is_none(), "not a prefix");
}

#[test]
fn a_name_on_something_that_is_not_a_button_is_ignored() {
    let tree = sidebar_tree(&[("Same", Kind::Other), ("Same", Kind::Group)]);
    assert!(find_button(&tree, &1, "Same").is_none());
}

#[test]
fn of_two_buttons_with_one_title_the_first_is_used() {
    let tree = sidebar_tree(&[("Twin", Kind::Button), ("Twin", Kind::Button)]);
    let found = find_button(&tree, &1, "Twin").unwrap();
    assert_eq!(found.node, 2);
    assert_eq!(found.matches, 2);
}

#[test]
fn a_missing_title_gives_none() {
    let tree = sidebar_tree(&[("A", Kind::Button)]);
    assert!(find_button(&tree, &1, "B").is_none());
}

/// A chain of groups `depth` deep under the sidebar, a button at the end.
fn chain(depth: usize) -> Fake {
    let mut items = vec![item(SIDEBAR, Kind::Group, &[1])];
    for level in 1..depth {
        items.push(item("g", Kind::Group, &[level + 1]));
    }
    items.push(item("Deep", Kind::Button, &[]));
    Fake(items)
}

#[test]
fn the_button_walk_stops_at_its_depth_limit() {
    assert!(find_button(&chain(BUTTON_DEPTH), &0, "Deep").is_some());
    assert!(find_button(&chain(BUTTON_DEPTH + 1), &0, "Deep").is_none());
}

#[test]
fn the_button_walk_stops_at_its_item_limit() {
    let mut items = vec![item(SIDEBAR, Kind::Group, &[])];
    for n in 0..BUTTON_ITEMS + 10 {
        items[0].children.push(n + 1);
        items.push(item(
            if n == BUTTON_ITEMS + 5 { "Late" } else { "x" },
            Kind::Button,
            &[],
        ));
    }
    assert!(find_button(&Fake(items), &0, "Late").is_none());
}

#[test]
fn the_sidebar_is_the_group_with_that_name() {
    let tree = sidebar_tree(&[("A", Kind::Button)]);
    assert_eq!(find_sidebar(&tree, &0), Some(1));
    let none = Fake(vec![
        item("Claude", Kind::Other, &[1]),
        item(SIDEBAR, Kind::Other, &[]),
    ]);
    assert_eq!(find_sidebar(&none, &0), None, "a non-group is not it");
    let popped_out = Fake(vec![item("Chat", Kind::Other, &[])]);
    assert_eq!(find_sidebar(&popped_out, &0), None);
}

#[test]
fn the_sidebar_walk_stops_at_its_depth_limit() {
    let mut items = vec![item("w", Kind::Other, &[1])];
    for level in 1..SIDEBAR_DEPTH + 2 {
        items.push(item("g", Kind::Other, &[level + 1]));
    }
    items.push(item(SIDEBAR, Kind::Group, &[]));
    assert_eq!(find_sidebar(&Fake(items), &0), None);
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
