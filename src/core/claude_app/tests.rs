//! The walks on made-up trees. No UI Automation and no window is used.

use super::*;

use std::cell::{Cell, RefCell};

struct Item {
    name: &'static str,
    kind: Kind,
    children: RefCell<Vec<usize>>,
    selected: Cell<Option<bool>>,
    collapsed: Cell<Option<bool>>,
    /// Children that show up under a parent when this item is selected or
    /// expanded: (parent, child).
    reveals: Vec<(usize, usize)>,
}

#[derive(Default)]
struct Fake {
    items: Vec<Item>,
    /// What was selected, expanded or invoked, by name.
    acts: RefCell<Vec<String>>,
}

impl Fake {
    fn new(items: Vec<Item>) -> Fake {
        Fake {
            items,
            acts: RefCell::default(),
        }
    }

    fn act(&self, node: &usize, what: &str) {
        let item = &self.items[*node];
        self.acts.borrow_mut().push(format!("{what} {}", item.name));
        item.selected.set(Some(true));
        item.collapsed.set(item.collapsed.get().map(|_| false));
        for (parent, child) in &item.reveals {
            self.items[*parent].children.borrow_mut().push(*child);
        }
    }

    fn acted(&self) -> Vec<String> {
        self.acts.borrow().clone()
    }
}

impl Tree for Fake {
    type Node = usize;

    fn children(&self, node: &usize) -> Vec<usize> {
        self.items[*node].children.borrow().clone()
    }

    fn name(&self, node: &usize) -> Option<String> {
        Some(self.items[*node].name.to_string())
    }

    fn kind(&self, node: &usize) -> Kind {
        self.items[*node].kind
    }

    fn selected(&self, node: &usize) -> Option<bool> {
        self.items[*node].selected.get()
    }

    fn collapsed(&self, node: &usize) -> Option<bool> {
        self.items[*node].collapsed.get()
    }

    fn select(&self, node: &usize) -> Result<(), String> {
        self.act(node, "select");
        Ok(())
    }

    fn expand(&self, node: &usize) -> Result<(), String> {
        self.act(node, "expand");
        Ok(())
    }

    fn invoke(&self, node: &usize) -> Result<(), String> {
        self.act(node, "invoke");
        Ok(())
    }
}

fn item(name: &'static str, kind: Kind, children: &[usize]) -> Item {
    Item {
        name,
        kind,
        children: RefCell::new(children.to_vec()),
        selected: Cell::new(None),
        collapsed: Cell::new(None),
        reveals: Vec::new(),
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
        items[2].children.borrow_mut().push(id);
        items.push(item(name, *kind, &[]));
    }
    Fake::new(items)
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
    Fake::new(items)
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
        items[0].children.borrow_mut().push(n + 1);
        let name = if n == ITEMS + 5 { "Late" } else { "x" };
        items.push(item(name, Kind::Button, &[]));
    }
    assert!(find_button(&Fake::new(items), &0, "Late").is_none());
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
    let narrow = Fake::new(vec![
        item("w", Kind::Other, &[1]),
        item(SHOW_SIDEBAR, Kind::Button, &[]),
    ]);
    assert!(is_home(&narrow, &0));
    let popped_out = Fake::new(vec![item("Chat", Kind::Other, &[])]);
    assert!(!is_home(&popped_out, &0));
    let not_a_group = Fake::new(vec![
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
fn the_rename_notice_names_claudedeck_and_the_chat() {
    use super::menu::rename_failed_notice;
    assert_eq!(
        rename_failed_notice("My chat"),
        "ClaudeDeck could not rename \"My chat\". Rename it in the Claude app."
    );
}

#[test]
fn only_rename_has_a_fallback_letter_and_never_delete() {
    use super::keys::{menu_letter, KEY_R};
    assert_eq!(menu_letter("Archive"), None);
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
fn renaming_starts_nothing_in_unit_tests() {
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    rename_chat("Any chat", "New", None, move |text| {
        let _ = sender.send(text);
    });
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
}

fn no_waiting() -> Waits {
    Waits {
        switch: Duration::ZERO,
        expand: Duration::ZERO,
        step: Duration::ZERO,
    }
}

fn with(mut item: Item, selected: Option<bool>, collapsed: Option<bool>) -> Item {
    item.selected = Cell::new(selected);
    item.collapsed = Cell::new(collapsed);
    item
}

/// 0 window; 1 radio Claude (selected); 2 radio Code (not selected, shows
/// the code chat list when selected); 3 list group; 4 the chat button.
fn normal_view() -> Fake {
    let mut code = with(item("Code", Kind::Radio, &[]), Some(false), None);
    code.reveals = vec![(3, 4)];
    Fake::new(vec![
        item("Claude Home", Kind::Other, &[1, 2, 3]),
        with(item("Claude", Kind::Radio, &[]), Some(true), None),
        code,
        item("", Kind::Group, &[]),
        item("My chat", Kind::Button, &[]),
    ])
}

#[test]
fn when_code_is_not_selected_it_is_selected_and_then_the_chat_is_found() {
    let tree = normal_view();
    let found = locate_chat(&tree, &0, "My chat", &no_waiting(), true)
        .unwrap()
        .expect("found after the switch");
    assert_eq!(found.button, 4);
    assert_eq!(tree.acted(), ["select Code"]);
}

#[test]
fn a_read_only_look_does_not_switch_the_view() {
    let tree = normal_view();
    let found = locate_chat(&tree, &0, "My chat", &no_waiting(), false).unwrap();
    assert!(found.is_none());
    assert!(tree.acted().is_empty());
}

#[test]
fn when_code_is_selected_nothing_is_pressed_to_find_a_visible_chat() {
    let tree = normal_view();
    tree.items[2].selected.set(Some(true));
    tree.items[3].children.borrow_mut().push(4);
    let found = locate_chat(&tree, &0, "My chat", &no_waiting(), true).unwrap();
    assert_eq!(found.unwrap().button, 4);
    assert!(tree.acted().is_empty());
}

#[test]
fn a_radio_button_named_code_marks_the_home_window() {
    assert!(is_home(&normal_view(), &0));
}

#[test]
fn the_hidden_list_is_opened_once_and_reported() {
    let mut show = with(item(SHOW_SIDEBAR, Kind::Button, &[]), None, Some(true));
    show.reveals = vec![(2, 3)];
    let tree = Fake::new(vec![
        item("Claude Home", Kind::Other, &[1, 2]),
        show,
        item("", Kind::Group, &[]),
        item("My chat", Kind::Button, &[]),
    ]);
    let found = locate_chat(&tree, &0, "My chat", &no_waiting(), true)
        .unwrap()
        .unwrap();
    assert_eq!(found.button, 3);
    assert_eq!(found.opened, Some(1));
    assert_eq!(tree.acted(), ["expand Show sidebar"]);
}

/// A list with folded sections, some that must not be touched, and one
/// folded section that hides the chat.
fn folded_list() -> Fake {
    let mut pinned = with(item("Pinned", Kind::Button, &[]), None, Some(true));
    pinned.reveals = vec![(1, 8)];
    Fake::new(vec![
        item("Claude Home", Kind::Other, &[1]),
        item("", Kind::Group, &[2, 3, 4, 5, 6, 7]),
        pinned,
        with(
            item("More options for Other", Kind::Button, &[]),
            None,
            Some(true),
        ),
        with(item("Menu", Kind::Button, &[]), None, Some(true)),
        with(
            item("Hide nested sessions Parent", Kind::Button, &[]),
            None,
            Some(true),
        ),
        with(item("Recents", Kind::Button, &[]), None, Some(false)),
        item("Plain button", Kind::Button, &[]),
        item("My chat", Kind::Button, &[]),
    ])
}

#[test]
fn only_folded_sections_are_expanded_each_once_then_the_chat_is_found() {
    let tree = folded_list();
    assert_eq!(collapsed_sections(&tree, &0), [2, 5]);
    let found = locate_chat(&tree, &0, "My chat", &no_waiting(), true)
        .unwrap()
        .unwrap();
    assert_eq!(found.button, 8);
    assert_eq!(
        tree.acted(),
        ["expand Pinned", "expand Hide nested sessions Parent"]
    );
}

#[test]
fn a_chat_that_is_nowhere_expands_the_sections_once_and_gives_none() {
    let tree = folded_list();
    let found = locate_chat(&tree, &0, "Missing", &no_waiting(), true).unwrap();
    assert!(found.is_none());
    assert_eq!(tree.acted().len(), 2);
    let again = locate_chat(&tree, &0, "Missing", &no_waiting(), true).unwrap();
    assert!(again.is_none());
    assert_eq!(
        tree.acted().len(),
        2,
        "expanded sections are not pressed again"
    );
}

#[test]
fn buttons_that_are_not_list_sections_are_left_alone() {
    for name in [
        "More options for X",
        "Hide sidebar",
        "Show sidebar",
        "View options",
        "Create project",
        "New from a template",
        "More navigation items",
        "Menu",
        "Terminal",
        "",
    ] {
        assert!(!is_section(name), "{name}");
    }
    for name in ["Pinned", "Recents", "Show nested sessions Parent"] {
        assert!(is_section(name), "{name}");
    }
}

#[test]
fn the_chat_menu_opens_through_its_more_options_button() {
    use super::menu::{more_options_name, open_with_more_options};
    let tree = Fake::new(vec![
        item("Claude Home", Kind::Other, &[1, 2]),
        item("My chat", Kind::Button, &[]),
        with(
            item("More options for My chat", Kind::Button, &[]),
            None,
            Some(true),
        ),
    ]);
    assert_eq!(more_options_name(" My chat "), "More options for My chat");
    assert_eq!(open_with_more_options(&tree, &0, "My chat"), Ok(true));
    assert_eq!(tree.acted(), ["expand More options for My chat"]);
    assert_eq!(open_with_more_options(&tree, &0, "Other"), Ok(false));
    assert_eq!(tree.acted().len(), 1);
}

#[test]
fn the_rename_button_of_the_open_chats_header_is_pressed() {
    use super::menu::{press_rename_header, rename_header_name};
    let tree = Fake::new(vec![
        item("Claude Home", Kind::Other, &[1]),
        item("My chat, rename session", Kind::Button, &[]),
    ]);
    assert_eq!(rename_header_name("My chat"), "My chat, rename session");
    let none = Duration::ZERO;
    assert_eq!(
        press_rename_header(&tree, &0, "My chat", none, none),
        Ok(true)
    );
    assert_eq!(tree.acted(), ["invoke My chat, rename session"]);
    assert_eq!(
        press_rename_header(&tree, &0, "Other", none, none),
        Ok(false)
    );
}

/// 0 the chat's own window, with its header; 1 the rename button; 2 the
/// home window; 3 the chat's button in the list; 4 its More options button.
fn own_and_list() -> Fake {
    Fake::new(vec![
        item("My chat", Kind::Other, &[1]),
        item("My chat, rename session", Kind::Button, &[]),
        item("Claude Home", Kind::Other, &[3, 4]),
        item("My chat", Kind::Button, &[]),
        with(
            item("More options for My chat", Kind::Button, &[]),
            None,
            Some(true),
        ),
    ])
}

#[test]
fn a_chat_with_its_own_window_is_renamed_there_and_the_list_is_not_touched() {
    use super::menu::{start_rename, RenameStart};
    let tree = own_and_list();
    let mut asked_for_list = 0;
    let mut list = || {
        asked_for_list += 1;
        Ok(Some(2))
    };
    let none = Duration::ZERO;
    let started = start_rename(&tree, Some(&0), &mut list, "My chat", none, none);
    assert_eq!(started, Ok(RenameStart::Header));
    assert_eq!(tree.acted(), ["invoke My chat, rename session"]);
    assert_eq!(asked_for_list, 0, "the home window was not even looked at");
}

#[test]
fn a_chat_with_no_window_is_renamed_through_more_options_and_never_opened() {
    use super::menu::{start_rename, RenameStart};
    let tree = own_and_list();
    let mut list = || Ok(Some(2));
    let none = Duration::ZERO;
    let started = start_rename(&tree, None, &mut list, "My chat", none, none);
    assert_eq!(started, Ok(RenameStart::Menu));
    assert_eq!(tree.acted(), ["expand More options for My chat"]);
    assert!(
        !tree
            .acted()
            .iter()
            .any(|act| act.starts_with("invoke My chat")),
        "the chat button is never pressed"
    );
}

#[test]
fn an_own_window_without_the_header_button_falls_back_to_the_list_without_opening_the_chat() {
    use super::menu::{start_rename, RenameStart};
    let tree = Fake::new(vec![
        item("My chat", Kind::Other, &[]),
        item("Claude Home", Kind::Other, &[2, 3]),
        item("My chat", Kind::Button, &[]),
        with(
            item("More options for My chat", Kind::Button, &[]),
            None,
            Some(true),
        ),
    ]);
    let mut list = || Ok(Some(1));
    let none = Duration::ZERO;
    let started = start_rename(&tree, Some(&0), &mut list, "My chat", none, none);
    assert_eq!(started, Ok(RenameStart::Menu));
    assert_eq!(tree.acted(), ["expand More options for My chat"]);
}

#[test]
fn a_chat_nested_under_a_folded_parent_is_found_after_the_parent_is_expanded() {
    let mut show = with(
        item("Show nested sessions Parent", Kind::Button, &[]),
        None,
        Some(true),
    );
    show.reveals = vec![(1, 3)];
    let tree = Fake::new(vec![
        item("Claude Home", Kind::Other, &[1]),
        item("", Kind::Group, &[2]),
        show,
        item("Nested chat", Kind::Button, &[]),
    ]);
    let found = locate_chat(&tree, &0, "Nested chat", &no_waiting(), true)
        .unwrap()
        .unwrap();
    assert_eq!(found.button, 3);
    assert_eq!(tree.acted(), ["expand Show nested sessions Parent"]);
}
