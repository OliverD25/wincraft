//! The walks and the rename checks on made-up trees. No UI Automation and
//! no window is used.

use super::guards::{self, Stop};
use super::*;

use std::cell::{Cell, RefCell};

struct Item {
    name: &'static str,
    kind: Kind,
    children: RefCell<Vec<usize>>,
    selected: Cell<Option<bool>>,
    focused: Cell<Option<bool>>,
    value: Option<&'static str>,
}

#[derive(Default)]
struct Fake {
    items: Vec<Item>,
    /// What was expanded or invoked, by name.
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

    fn focused(&self, node: &usize) -> Option<bool> {
        self.items[*node].focused.get()
    }

    fn value(&self, node: &usize) -> Option<String> {
        self.items[*node].value.map(str::to_string)
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
        focused: Cell::new(None),
        value: None,
    }
}

fn selected(item: Item) -> Item {
    item.selected.set(Some(true));
    item
}

/// 0 window; 1 the empty Sidebar group; 2 a list group; then the given
/// leaves under the list group. As the live app: the chat buttons are not
/// inside the Sidebar group.
fn home(leaves: &[(&'static str, Kind)]) -> Fake {
    home_of(
        leaves
            .iter()
            .map(|(name, kind)| item(name, *kind, &[]))
            .collect(),
    )
}

fn home_of(leaves: Vec<Item>) -> Fake {
    let mut items = vec![
        item("Claude Home", Kind::Other, &[1, 2]),
        item(SIDEBAR, Kind::Group, &[]),
        item("", Kind::Group, &[]),
    ];
    for leaf in leaves {
        let id = items.len();
        items[2].children.borrow_mut().push(id);
        items.push(leaf);
    }
    Fake::new(items)
}

fn first_button(tree: &Fake, title: &str) -> Option<usize> {
    find_named(tree, &0, Kind::Button, title).map(|found| found.node)
}

#[test]
fn a_button_outside_the_sidebar_group_is_found_by_its_exact_title() {
    let tree = home(&[
        ("Other chat", Kind::Button),
        ("WinCraft engine", Kind::Button),
    ]);
    assert_eq!(first_button(&tree, "WinCraft engine"), Some(4));
    assert_eq!(first_button(&tree, "WinCraft"), None, "not a prefix");
}

#[test]
fn a_name_on_something_that_is_not_a_button_is_ignored() {
    let tree = home(&[("Same", Kind::Other), ("Same", Kind::Group)]);
    assert_eq!(first_button(&tree, "Same"), None);
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
    assert!(first_button(&chain(DEPTH), "Deep").is_some());
    assert!(first_button(&chain(DEPTH + 1), "Deep").is_none());
    assert_eq!(chain(DEPTH).find_all(&0, Kind::Button, "Deep"), [DEPTH]);
    assert!(chain(DEPTH + 1)
        .find_all(&0, Kind::Button, "Deep")
        .is_empty());
}

#[test]
fn the_walk_stops_at_its_item_limit() {
    let mut items = vec![item("root", Kind::Group, &[])];
    for n in 0..ITEMS + 10 {
        items[0].children.borrow_mut().push(n + 1);
        let name = if n == ITEMS + 5 { "Late" } else { "x" };
        items.push(item(name, Kind::Button, &[]));
    }
    let tree = Fake::new(items);
    assert!(first_button(&tree, "Late").is_none());
    assert!(tree.find_all(&0, Kind::Button, "Late").is_empty());
}

#[test]
fn every_button_of_one_name_is_found_in_reading_order() {
    let tree = home(&[
        ("Twin", Kind::Button),
        ("Twin", Kind::Other),
        ("Twin", Kind::Button),
    ]);
    assert_eq!(tree.find_all(&0, Kind::Button, "Twin"), [3, 5]);
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
fn a_radio_button_named_code_marks_the_home_window() {
    let tree = Fake::new(vec![
        item("w", Kind::Other, &[1]),
        item(CODE_VIEW, Kind::Radio, &[]),
    ]);
    assert!(is_home(&tree, &0));
}

#[test]
fn the_rename_notice_names_claudedeck_and_the_chat() {
    use super::menu::rename_failed_notice;
    assert_eq!(
        rename_failed_notice("My chat"),
        "ClaudeDeck could not rename \"My chat\" in the Claude app. Rename it there."
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
    rename_chat("local_any", "Any chat", "New", None, move |text| {
        let _ = sender.send(text);
    });
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn the_chat_menu_opens_through_its_more_options_button() {
    use super::menu::{more_options_name, open_with_more_options};
    let tree = Fake::new(vec![
        item("Claude Home", Kind::Other, &[1, 2]),
        item("My chat", Kind::Button, &[]),
        item("More options for My chat", Kind::Button, &[]),
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

// The rename checks. Each one only reads; none presses anything.

#[test]
fn the_only_button_with_the_chats_exact_name_is_its_button() {
    let tree = home(&[("Other", Kind::Button), ("My chat", Kind::Button)]);
    assert_eq!(guards::chat_button(&tree, &0, " My chat "), Ok(4));
    assert!(tree.acted().is_empty());
}

#[test]
fn a_chat_not_in_the_list_stops_the_rename() {
    let tree = home(&[("My chat 2", Kind::Button), ("My chat", Kind::Other)]);
    assert_eq!(
        guards::chat_button(&tree, &0, "My chat"),
        Err(Stop::NotInList)
    );
}

#[test]
fn of_several_buttons_with_the_name_only_the_selected_one_is_taken() {
    let tree = home_of(vec![
        item("Twin", Kind::Button, &[]),
        selected(item("Twin", Kind::Button, &[])),
    ]);
    assert_eq!(guards::chat_button(&tree, &0, "Twin"), Ok(4));
}

#[test]
fn several_buttons_with_the_name_and_none_selected_stop_the_rename() {
    let tree = home(&[("Twin", Kind::Button), ("Twin", Kind::Button)]);
    assert_eq!(
        guards::chat_button(&tree, &0, "Twin"),
        Err(Stop::SeveralNamed(2))
    );
    let both = home_of(vec![
        selected(item("Twin", Kind::Button, &[])),
        selected(item("Twin", Kind::Button, &[])),
    ]);
    assert_eq!(
        guards::chat_button(&both, &0, "Twin"),
        Err(Stop::SeveralNamed(2))
    );
}

#[test]
fn keys_go_to_the_app_only_when_the_chats_button_has_the_focus_and_the_app_is_in_front() {
    assert_eq!(guards::ready_for_keys(Some(true), true), Ok(()));
    assert_eq!(
        guards::ready_for_keys(Some(false), true),
        Err(Stop::NotFocused)
    );
    assert_eq!(guards::ready_for_keys(None, true), Err(Stop::NotFocused));
    assert_eq!(
        guards::ready_for_keys(Some(true), false),
        Err(Stop::NotFocused)
    );
}

#[test]
fn the_focus_check_reads_the_trees_focus() {
    let tree = home_of(vec![item("My chat", Kind::Button, &[])]);
    let button = guards::chat_button(&tree, &0, "My chat").unwrap();
    assert!(guards::ready_for_keys(tree.focused(&button), true).is_err());
    tree.items[button].focused.set(Some(true));
    assert!(guards::ready_for_keys(tree.focused(&button), true).is_ok());
}

#[test]
fn the_rename_item_must_have_the_exact_label_in_some_window_of_the_app() {
    let list = home(&[("Rename", Kind::Button), ("Renamed", Kind::MenuItem)]);
    assert_eq!(guards::menu_item(&list, &[0], "Rename"), None);
    let menu = Fake::new(vec![
        item("Claude Home", Kind::Other, &[]),
        item("menu", Kind::Other, &[2, 3]),
        item("Pin", Kind::MenuItem, &[]),
        item("Rename", Kind::MenuItem, &[]),
    ]);
    assert_eq!(guards::menu_item(&menu, &[0, 1], "Rename"), Some(3));
    assert_eq!(guards::menu_item(&menu, &[0], "Rename"), None);
}

#[test]
fn the_name_is_typed_only_into_a_focused_edit_box_that_holds_the_chats_name() {
    let mut name_box = item("", Kind::Edit, &[]);
    name_box.value = Some("My chat");
    let mut message_box = item("Message", Kind::Edit, &[]);
    message_box.value = Some("");
    let mut not_an_edit = item("My chat", Kind::Button, &[]);
    not_an_edit.value = Some("My chat");
    let tree = Fake::new(vec![name_box, message_box, not_an_edit]);
    assert_eq!(guards::name_box(&tree, Some(0), " My chat "), Ok(0));
    assert_eq!(
        guards::name_box(&tree, Some(1), "My chat"),
        Err(Stop::NoNameBox)
    );
    assert_eq!(
        guards::name_box(&tree, Some(2), "My chat"),
        Err(Stop::NoNameBox)
    );
    assert_eq!(
        guards::name_box(&tree, None, "My chat"),
        Err(Stop::NoNameBox)
    );
}

#[test]
fn every_stop_has_a_reason_for_the_log() {
    let stops = [
        Stop::NoLink,
        Stop::LinkFailed,
        Stop::NoHomeWindow,
        Stop::NotInList,
        Stop::SeveralNamed(3),
        Stop::NotFocused,
        Stop::NoHeaderButton,
        Stop::NoRenameItem,
        Stop::NoNameBox,
        Stop::KeysRefused,
    ];
    for stop in stops {
        assert!(!stop.reason().is_empty(), "{stop:?}");
    }
    assert!(Stop::SeveralNamed(3).reason().starts_with("3 buttons"));
}
