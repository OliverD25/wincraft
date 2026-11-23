//! The right-click menus of ClaudeDeck: one for a chat, one for a group.
//! The Menu key and Shift+F10 open them for the selected line. Every item
//! changes only ClaudeDeck's own list, except the Quick access number
//! items, which ask QuickJump to change its pins; none closes, archives or
//! renames a real chat. A note is ClaudeDeck's own too.

use egui::{Color32, Ui};

use crate::core::quick_pins;

use super::model::{ChatLine, Fold};
use super::store::Colour;

pub const OPEN: &str = "Open the chat";
pub const SHOW_IN_APP: &str = "Show in the Claude app";
pub const PIN: &str = "Pin to the top";
pub const UNPIN: &str = "Unpin";
pub const ADD_NOTE: &str = "Add a note\u{2026}";
pub const EDIT_NOTE: &str = "Edit the note\u{2026}";
pub const REMOVE_NOTE: &str = "Remove the note";
pub const GIVE_NUMBER: &str = "Give a Quick access number";
pub const FIRST_FREE: &str = "First free number";
pub const REMOVE_NUMBER: &str = "Remove the Quick access number";
/// Followed by the chat's digit: "Quick access number 3".
pub const HAS_NUMBER: &str = "Quick access number";
pub const HIDE: &str = "Hide this chat";
pub const SHOW: &str = "Show this chat";
pub const COPY: &str = "Copy the name";
pub const FORK: &str = "Fork in a terminal";
pub const RENAME_CHAT: &str = "Rename the chat…";
pub const FOLD_FORKS: &str = "Fold its forks";
pub const UNFOLD_FORKS: &str = "Unfold its forks";
pub const MOVE_TO: &str = "Move to group";
pub const NEW_WITH: &str = "New group with this chat";
pub const RENAME: &str = "Rename the group";
pub const FOLD_GROUP: &str = "Fold the group";
pub const UNFOLD_GROUP: &str = "Unfold the group";
pub const FOLD_ALL: &str = "Fold all groups";
pub const UNFOLD_ALL: &str = "Unfold all groups";
pub const COLOUR: &str = "Colour";
pub const NO_COLOUR: &str = "No colour";
pub const NEW_GROUP: &str = "New group";
pub const MOVE_UP: &str = "Move up";
pub const MOVE_DOWN: &str = "Move down";
pub const UNGROUP: &str = "Ungroup (chats go back to their folder)";

/// A line of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Open,
    ShowInApp,
    Pin,
    Unpin,
    AddNote,
    EditNote,
    RemoveNote,
    /// QuickJump's number of the chat: its place, None for no number.
    Quick(Option<usize>),
    RemoveQuick,
    Hide,
    Show,
    Copy,
    RenameChat,
    Fork,
    FoldForks,
    UnfoldForks,
    MoveTo,
    NewWith,
    Rename,
    FoldGroup,
    UnfoldGroup,
    FoldAll,
    UnfoldAll,
    Colour,
    NewGroup,
    MoveUp,
    MoveDown,
    Ungroup,
    Separator,
}

/// What the user picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Open,
    /// The Claude app's main window shows the chat.
    ShowInApp,
    Pin(bool),
    /// Open the note box on the row (to add or to edit).
    Note,
    RemoveNote,
    /// Give the chat this Quick access number (a place from 0), or the
    /// first free one when None.
    QuickAt(Option<usize>),
    RemoveQuick,
    Hide(bool),
    Copy,
    /// The chat's name, changed in the Claude app too.
    RenameChat,
    /// A fork of the chat in a new terminal.
    Fork,
    FoldForks,
    MoveTo(String),
    NewWith,
    Rename,
    FoldGroup,
    FoldAll(bool),
    Colour(Option<Colour>),
    NewGroup,
    /// Up when true.
    Step(bool),
    Ungroup,
}

/// A chat of the Claude app on this PC whose id the app has a link for.
/// A chat in a terminal has no such id; a stand-in line is not a chat.
pub fn can_show_in_app(line: &ChatLine) -> bool {
    !line.stand_in
        && line.chat.terminal.is_none()
        && crate::core::claude_app::chat_link(&line.chat.id).is_some()
}

/// The menu of a chat row. `in_group` is false for the rows under the cold
/// and hidden lines. Pinning and moving belong to the chat at the top of a
/// family; a fork stays under its chat.
pub fn chat_items(line: &ChatLine, in_group: bool) -> Vec<Item> {
    // A fork running in a terminal: only bring it forward or copy its name.
    if line.chat.terminal.is_some() {
        return vec![Item::Open, Item::Copy];
    }
    let mut items = Vec::new();
    if !line.stand_in {
        items.push(Item::Open);
        if can_show_in_app(line) {
            items.push(Item::ShowInApp);
        }
    }
    if line.hidden {
        items.push(Item::Show);
        if !line.stand_in {
            items.extend([Item::Copy, Item::RenameChat]);
            items.extend(fork_item(line));
        }
        return items;
    }
    let top = line.depth == 0 && in_group;
    if top {
        items.push(if line.pinned { Item::Unpin } else { Item::Pin });
    }
    if super::note::can_note(line) {
        if line.note.is_some() {
            items.extend([Item::EditNote, Item::RemoveNote]);
        } else {
            items.push(Item::AddNote);
        }
    }
    items.push(Item::Hide);
    if !line.stand_in {
        items.extend([Item::Copy, Item::RenameChat]);
        items.extend(fork_item(line));
    }
    let mut more = Vec::new();
    match line.fold {
        Fold::Open => more.push(Item::FoldForks),
        Fold::Folded(_) => more.push(Item::UnfoldForks),
        Fold::None => {}
    }
    if top {
        more.extend([Item::MoveTo, Item::NewWith]);
    }
    if !more.is_empty() {
        items.push(Item::Separator);
        items.extend(more);
    }
    items
}

/// Adds the Quick access number items after Pin to the top and the note
/// items (or after Show in the Claude app for a chat that has neither)
/// when `number` says
/// QuickJump is on: Some(the chat's place, if any). Only a chat of the
/// Claude app on this PC has a link a Quick access pin can open.
pub fn with_quick(
    mut items: Vec<Item>,
    line: &ChatLine,
    number: Option<Option<usize>>,
) -> Vec<Item> {
    let Some(number) = number.filter(|_| can_show_in_app(line)) else {
        return items;
    };
    let after = items
        .iter()
        .rposition(|item| {
            matches!(
                item,
                Item::Pin | Item::Unpin | Item::AddNote | Item::EditNote | Item::RemoveNote
            )
        })
        .or_else(|| items.iter().position(|item| *item == Item::ShowInApp));
    let Some(after) = after else {
        return items;
    };
    let mut quick = vec![Item::Quick(number)];
    if number.is_some() {
        quick.push(Item::RemoveQuick);
    }
    items.splice(after + 1..after + 1, quick);
    items
}

/// "Quick access number 3".
pub fn has_number(slot: usize) -> String {
    format!("{HAS_NUMBER} {}", quick_pins::digit_of_slot(slot))
}

/// The numbers 1-9 and 0, each with what holds it; a chat with no number
/// gets "First free number" on top while one is free.
fn numbers(ui: &mut Ui, own: Option<usize>) -> Option<Choice> {
    let held = quick_pins::held().unwrap_or_default();
    let mut pick = None;
    if own.is_none() && quick_pins::first_free(&held).is_some() && ui.button(FIRST_FREE).clicked() {
        pick = Some(Choice::QuickAt(None));
    }
    if own.is_none() {
        ui.separator();
    }
    for slot in 0..quick_pins::SLOTS {
        let can = quick_pins::can_pick(slot, held.len(), own);
        let line = quick_pins::menu_line(slot, &held);
        if ui.add_enabled(can, egui::Button::new(line)).clicked() {
            pick = Some(Choice::QuickAt(Some(slot)));
        }
    }
    pick
}

fn fork_item(line: &ChatLine) -> Option<Item> {
    line.chat.fork.as_ref().map(|_| Item::Fork)
}

/// The menu of a group header.
pub fn group_items(folded: bool) -> Vec<Item> {
    vec![
        Item::Rename,
        if folded {
            Item::UnfoldGroup
        } else {
            Item::FoldGroup
        },
        Item::FoldAll,
        Item::UnfoldAll,
        Item::Colour,
        Item::Separator,
        Item::NewGroup,
        Item::MoveUp,
        Item::MoveDown,
        Item::Separator,
        Item::Ungroup,
    ]
}

/// What the menu needs besides its items.
pub struct Context<'a> {
    /// The other groups a chat can move to: id and name.
    pub groups: &'a [(String, String)],
    /// Whether the chat has a window to open.
    pub can_open: bool,
    /// The colour of each choice of "Colour", by the theme.
    pub colour: &'a dyn Fn(Colour) -> Color32,
}

fn button(ui: &mut Ui, label: &str, keys: &str) -> bool {
    let mut button = egui::Button::new(label);
    if !keys.is_empty() {
        button = button.shortcut_text(keys);
    }
    ui.add(button).clicked()
}

/// Draws a menu and returns what was picked; a pick closes it.
pub fn show(ui: &mut Ui, items: &[Item], context: &Context) -> Option<Choice> {
    let mut chosen = None;
    for item in items {
        let pick = match item {
            Item::Open => ui
                .add_enabled(
                    context.can_open,
                    egui::Button::new(OPEN).shortcut_text("Enter"),
                )
                .clicked()
                .then_some(Choice::Open),
            Item::ShowInApp => button(ui, SHOW_IN_APP, "Ctrl+Enter").then_some(Choice::ShowInApp),
            Item::Pin => button(ui, PIN, "").then_some(Choice::Pin(true)),
            Item::Unpin => button(ui, UNPIN, "").then_some(Choice::Pin(false)),
            Item::AddNote => button(ui, ADD_NOTE, "N").then_some(Choice::Note),
            Item::EditNote => button(ui, EDIT_NOTE, "N").then_some(Choice::Note),
            Item::RemoveNote => button(ui, REMOVE_NOTE, "").then_some(Choice::RemoveNote),
            Item::Quick(own) => {
                let title = own.map_or_else(|| GIVE_NUMBER.to_string(), has_number);
                let mut pick = None;
                ui.menu_button(title, |ui| pick = numbers(ui, *own));
                pick
            }
            Item::RemoveQuick => button(ui, REMOVE_NUMBER, "").then_some(Choice::RemoveQuick),
            Item::Hide => button(ui, HIDE, "").then_some(Choice::Hide(true)),
            Item::Show => button(ui, SHOW, "").then_some(Choice::Hide(false)),
            Item::Copy => button(ui, COPY, "Ctrl+C").then_some(Choice::Copy),
            Item::RenameChat => button(ui, RENAME_CHAT, "F2").then_some(Choice::RenameChat),
            Item::Fork => button(ui, FORK, "").then_some(Choice::Fork),
            Item::FoldForks => button(ui, FOLD_FORKS, "").then_some(Choice::FoldForks),
            Item::UnfoldForks => button(ui, UNFOLD_FORKS, "").then_some(Choice::FoldForks),
            Item::MoveTo => {
                let mut pick = None;
                ui.add_enabled_ui(!context.groups.is_empty(), |ui| {
                    ui.menu_button(MOVE_TO, |ui| {
                        for (id, name) in context.groups {
                            if ui.button(name).clicked() {
                                pick = Some(Choice::MoveTo(id.clone()));
                            }
                        }
                    });
                });
                pick
            }
            Item::NewWith => button(ui, NEW_WITH, "").then_some(Choice::NewWith),
            Item::Rename => button(ui, RENAME, "F2").then_some(Choice::Rename),
            Item::FoldGroup => button(ui, FOLD_GROUP, "").then_some(Choice::FoldGroup),
            Item::UnfoldGroup => button(ui, UNFOLD_GROUP, "").then_some(Choice::FoldGroup),
            Item::FoldAll => button(ui, FOLD_ALL, "").then_some(Choice::FoldAll(true)),
            Item::UnfoldAll => button(ui, UNFOLD_ALL, "").then_some(Choice::FoldAll(false)),
            Item::Colour => {
                let mut pick = None;
                ui.menu_button(COLOUR, |ui| {
                    if ui.button(NO_COLOUR).clicked() {
                        pick = Some(Choice::Colour(None));
                    }
                    for colour in Colour::ALL {
                        // The shortcut space holds the colour's dot.
                        let button = egui::Button::new(colour.name()).shortcut_text("    ");
                        let response = ui.add(button);
                        let dot = egui::Rect::from_center_size(
                            egui::pos2(response.rect.right() - 14.0, response.rect.center().y),
                            egui::vec2(8.0, 8.0),
                        );
                        ui.painter().rect_filled(dot, 2.0, (context.colour)(colour));
                        if response.clicked() {
                            pick = Some(Choice::Colour(Some(colour)));
                        }
                    }
                });
                pick
            }
            Item::NewGroup => button(ui, NEW_GROUP, "").then_some(Choice::NewGroup),
            Item::MoveUp => button(ui, MOVE_UP, "Alt+Up").then_some(Choice::Step(true)),
            Item::MoveDown => button(ui, MOVE_DOWN, "Alt+Down").then_some(Choice::Step(false)),
            Item::Ungroup => button(ui, UNGROUP, "").then_some(Choice::Ungroup),
            Item::Separator => {
                ui.separator();
                None
            }
        };
        if pick.is_some() {
            chosen = pick;
        }
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}
