//! The right-click menus of ClaudeDeck: one for a chat, one for a group.
//! The Menu key and Shift+F10 open them for the selected line. Every item
//! changes only ClaudeDeck's own list; none closes, archives or renames a
//! real chat.

use egui::{Color32, Ui};

use super::model::{ChatLine, Fold};
use super::store::Colour;

pub const OPEN: &str = "Open the chat";
pub const PIN: &str = "Pin to the top";
pub const UNPIN: &str = "Unpin";
pub const HIDE: &str = "Hide this chat";
pub const SHOW: &str = "Show this chat";
pub const COPY: &str = "Copy the name";
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
    Pin,
    Unpin,
    Hide,
    Show,
    Copy,
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
    Pin(bool),
    Hide(bool),
    Copy,
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

/// The menu of a chat row. `in_group` is false for the rows under the cold
/// and hidden lines. Pinning and moving belong to the chat at the top of a
/// family; a fork stays under its chat.
pub fn chat_items(line: &ChatLine, in_group: bool) -> Vec<Item> {
    let mut items = Vec::new();
    if !line.stand_in {
        items.push(Item::Open);
    }
    if line.hidden {
        items.push(Item::Show);
        if !line.stand_in {
            items.push(Item::Copy);
        }
        return items;
    }
    let top = line.depth == 0 && in_group;
    if top {
        items.push(if line.pinned { Item::Unpin } else { Item::Pin });
    }
    items.push(Item::Hide);
    if !line.stand_in {
        items.push(Item::Copy);
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
            Item::Pin => button(ui, PIN, "").then_some(Choice::Pin(true)),
            Item::Unpin => button(ui, UNPIN, "").then_some(Choice::Pin(false)),
            Item::Hide => button(ui, HIDE, "").then_some(Choice::Hide(true)),
            Item::Show => button(ui, SHOW, "").then_some(Choice::Hide(false)),
            Item::Copy => button(ui, COPY, "Ctrl+C").then_some(Choice::Copy),
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
