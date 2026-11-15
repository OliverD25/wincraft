//! The checks a rename passes before WinCraft presses anything or sends a
//! key to the Claude app. Each one either gives what the next step needs or
//! a [`Stop`], which ends the rename with a notice. They only read the tree,
//! so tests run them on made-up trees.

use super::{find_named, Kind, Tree};

/// Why a rename stopped before it was done. The log names it; the user
/// sees one notice for all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The chat has no link in the app (it runs only in a terminal).
    NoLink,
    /// Windows could not open the chat's link.
    LinkFailed,
    /// The app's main window did not show.
    NoHomeWindow,
    /// No button in the app's list has the chat's exact name.
    NotInList,
    /// Several buttons have the chat's name and none of them is selected.
    SeveralNamed(usize),
    /// The chat's button did not take the keyboard focus, or the app's
    /// window is not in front.
    NotFocused,
    /// The chat's own window has no rename button in its header.
    NoHeaderButton,
    /// The menu opened with no item of the exact rename label.
    NoRenameItem,
    /// No edit box with the chat's name has the keyboard focus.
    NoNameBox,
    /// Windows did not take a key or the text.
    KeysRefused,
}

impl Stop {
    /// What the log says about this stop.
    pub fn reason(self) -> String {
        match self {
            Stop::NoLink => "the chat has no link in the Claude app".into(),
            Stop::LinkFailed => "Windows could not open the chat's link".into(),
            Stop::NoHomeWindow => "the Claude app's main window did not show".into(),
            Stop::NotInList => "no button in the app's list has the chat's exact name".into(),
            Stop::SeveralNamed(count) => {
                format!("{count} buttons have the chat's name and none is selected")
            }
            Stop::NotFocused => {
                "the chat's button did not take the focus, or the app is not in front".into()
            }
            Stop::NoHeaderButton => "the chat's own window has no rename button".into(),
            Stop::NoRenameItem => "the menu has no item named Rename".into(),
            Stop::NoNameBox => "no edit box with the chat's name has the focus".into(),
            Stop::KeysRefused => "Windows did not take the keys".into(),
        }
    }
}

/// The chat's button in the app's list: the only button with its exact
/// name, or the selected one of several. Anything else stops: a rename of
/// the wrong chat is worse than no rename.
pub fn chat_button<T: Tree>(tree: &T, root: &T::Node, title: &str) -> Result<T::Node, Stop> {
    let mut named = tree.find_all(root, Kind::Button, title.trim());
    if named.is_empty() {
        return Err(Stop::NotInList);
    }
    if named.len() == 1 {
        return Ok(named.remove(0));
    }
    let count = named.len();
    let mut selected: Vec<T::Node> = named
        .into_iter()
        .filter(|node| tree.selected(node) == Some(true))
        .collect();
    if selected.len() == 1 {
        Ok(selected.remove(0))
    } else {
        Err(Stop::SeveralNamed(count))
    }
}

/// Whether a key may go to the chat's button: it has the keyboard focus
/// and the app's window is the one in front.
pub fn ready_for_keys(button_focused: Option<bool>, app_in_front: bool) -> Result<(), Stop> {
    if button_focused == Some(true) && app_in_front {
        Ok(())
    } else {
        Err(Stop::NotFocused)
    }
}

/// The menu item with this exact label in any of the app's windows (a menu
/// may be a window of its own).
pub fn menu_item<T: Tree>(tree: &T, roots: &[T::Node], label: &str) -> Option<T::Node> {
    roots
        .iter()
        .find_map(|root| find_named(tree, root, Kind::MenuItem, label))
        .map(|found| found.node)
}

/// The rename box: the element with the keyboard focus, only when it is an
/// edit box that holds the chat's name now. The app fills the box with the
/// old name, so an empty box (such as the message box) is never typed into.
pub fn name_box<T: Tree>(tree: &T, focused: Option<T::Node>, title: &str) -> Result<T::Node, Stop> {
    let focused = focused.ok_or(Stop::NoNameBox)?;
    let holds_name = tree
        .value(&focused)
        .is_some_and(|text| text.trim() == title.trim());
    if tree.kind(&focused) == Kind::Edit && holds_name {
        Ok(focused)
    } else {
        Err(Stop::NoNameBox)
    }
}
