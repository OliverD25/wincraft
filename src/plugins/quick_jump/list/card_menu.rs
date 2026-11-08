//! The right-click menu of a Quick access card, in the look of WindowDeck's
//! card menu. The Menu key and Shift+F10 open it for the selected card.
//! Closing the real window is offered only here, as in WindowDeck, and only
//! while the window is open: it is always the user's own choice.

use egui::Ui;

use super::super::matching::{SlotView, State};
use super::super::pins::{number_of_slot, SLOTS};
use super::super::shared::Request;

/// A line of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Jump,
    PutAt,
    Edit,
    Unpin,
    Separator,
    CloseWindow,
}

/// What the user picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Jump,
    /// Move the card to this place (from 0).
    PutAt(usize),
    Edit,
    Unpin,
    CloseWindow,
}

impl Choice {
    /// What the choice asks of the engine, for the card at place `slot`.
    /// Edit details only opens the panel.
    pub fn request(self, slot: usize) -> Option<Request> {
        match self {
            Choice::Jump => Some(Request::Jump(slot)),
            Choice::PutAt(to) => Some(Request::Move { from: slot, to }),
            Choice::Edit => None,
            Choice::Unpin => Some(Request::Remove(slot)),
            Choice::CloseWindow => Some(Request::CloseWindow(slot)),
        }
    }
}

pub const JUMP: &str = "Jump to it";
pub const PUT_AT: &str = "Put at number";
pub const EDIT: &str = "Edit details";
pub const UNPIN: &str = "Unpin";
pub const CLOSE: &str = "Close the window";

/// The lines of the menu for `card`, in order.
pub fn items(card: &SlotView) -> Vec<Item> {
    let mut items = vec![Item::Jump, Item::PutAt, Item::Edit, Item::Unpin];
    if is_open(card) {
        items.extend([Item::Separator, Item::CloseWindow]);
    }
    items
}

/// Only a window that is open now can be closed.
fn is_open(card: &SlotView) -> bool {
    matches!(card.state, State::Open | State::InFront) && card.hwnd.is_some()
}

/// The numbers "Put at number" offers: all ten, the card's own marked,
/// and those past the end of the list greyed out.
pub fn numbers(slot: usize, count: usize) -> Vec<(usize, bool, bool)> {
    (0..SLOTS)
        .map(|place| (number_of_slot(place), place == slot, place < count))
        .collect()
}

/// Draws the menu for the card at place `slot` of `count`.
pub fn show(ui: &mut Ui, card: &SlotView, slot: usize, count: usize) -> Option<Choice> {
    let mut chosen = None;
    for item in items(card) {
        match item {
            Item::Jump => {
                if ui.button(JUMP).clicked() {
                    chosen = Some(Choice::Jump);
                }
            }
            Item::PutAt => {
                ui.menu_button(PUT_AT, |ui| {
                    for (place, (number, current, enabled)) in
                        numbers(slot, count).into_iter().enumerate()
                    {
                        let label = ui.add_enabled(
                            enabled,
                            egui::Button::selectable(current, number.to_string()),
                        );
                        if label.clicked() && !current {
                            chosen = Some(Choice::PutAt(place));
                        }
                    }
                });
            }
            Item::Edit => {
                if ui
                    .add(egui::Button::new(EDIT).shortcut_text("F2"))
                    .clicked()
                {
                    chosen = Some(Choice::Edit);
                }
            }
            Item::Unpin => {
                if ui
                    .add(egui::Button::new(UNPIN).shortcut_text("Del"))
                    .clicked()
                {
                    chosen = Some(Choice::Unpin);
                }
            }
            Item::Separator => {
                ui.separator();
            }
            Item::CloseWindow => {
                if ui.button(CLOSE).clicked() {
                    chosen = Some(Choice::CloseWindow);
                }
            }
        }
    }
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(state: State, hwnd: Option<isize>) -> SlotView {
        SlotView {
            state,
            hwnd,
            ..SlotView::default()
        }
    }

    #[test]
    fn the_menu_lists_jump_put_at_edit_unpin_and_close_for_an_open_window() {
        assert_eq!(
            items(&card(State::Open, Some(7))),
            vec![
                Item::Jump,
                Item::PutAt,
                Item::Edit,
                Item::Unpin,
                Item::Separator,
                Item::CloseWindow
            ]
        );
        assert_eq!(items(&card(State::InFront, Some(7))).len(), 6);
    }

    #[test]
    fn close_the_window_shows_only_while_the_window_is_open() {
        let closed = items(&card(State::NotOpen, None));
        assert!(!closed.contains(&Item::CloseWindow));
        assert!(!closed.contains(&Item::Separator));
        assert!(!items(&card(State::Open, None)).contains(&Item::CloseWindow));
    }

    #[test]
    fn each_choice_asks_the_engine_for_the_same_as_its_key() {
        assert_eq!(
            Choice::PutAt(4).request(1),
            Some(Request::Move { from: 1, to: 4 })
        );
        assert_eq!(Choice::Unpin.request(2), Some(Request::Remove(2)));
        assert_eq!(Choice::Jump.request(3), Some(Request::Jump(3)));
        assert_eq!(
            Choice::CloseWindow.request(0),
            Some(Request::CloseWindow(0))
        );
        assert_eq!(Choice::Edit.request(0), None);
    }

    #[test]
    fn put_at_number_offers_one_to_ten_with_the_own_number_marked() {
        let numbers = numbers(2, 4);
        assert_eq!(numbers.len(), 10);
        assert_eq!(numbers[0], (1, false, true));
        assert_eq!(numbers[2], (3, true, true), "the card's own number");
        assert_eq!(numbers[3], (4, false, true));
        assert_eq!(numbers[4], (5, false, false), "past the end of the list");
        assert_eq!(numbers[9], (10, false, false));
    }
}
