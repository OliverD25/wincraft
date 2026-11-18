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
    Repin,
    Edit,
    Unpin,
    Separator,
    CloseWindow,
}

/// What the user picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Jump,
    /// Put the card at this number's place (from 0).
    PutAt(usize),
    Repin,
    Edit,
    Unpin,
    CloseWindow,
}

impl Choice {
    /// What the choice asks of the engine, for the pin at place `slot`.
    /// Edit details only opens the panel.
    pub fn request(self, slot: usize) -> Option<Request> {
        match self {
            Choice::Jump => Some(Request::Jump(slot)),
            Choice::PutAt(to) => Some(Request::PutAt { from: slot, to }),
            Choice::Repin => Some(Request::Repin(slot)),
            Choice::Edit => None,
            Choice::Unpin => Some(Request::Remove(slot)),
            Choice::CloseWindow => Some(Request::CloseWindow(slot)),
        }
    }
}

pub const JUMP: &str = "Jump to it";
pub const PUT_AT: &str = "Put at number";
pub const REPIN: &str = "Re-pin the window I was in";
pub const EDIT: &str = "Edit details";
pub const UNPIN: &str = "Unpin";
pub const CLOSE: &str = "Close the window";

/// The lines of the menu for `card`, in order.
pub fn items(card: &SlotView) -> Vec<Item> {
    let mut items = vec![Item::Jump, Item::PutAt];
    if can_repin(card) {
        items.push(Item::Repin);
    }
    items.extend([Item::Edit, Item::Unpin]);
    if is_open(card) {
        items.extend([Item::Separator, Item::CloseWindow]);
    }
    items
}

/// Only a card that is not open can take another window: R and the menu.
pub fn can_repin(card: &SlotView) -> bool {
    !is_open(card)
}

/// What R asks of the engine for the card at place `slot`; nothing on an
/// open card.
pub fn repin_request(card: &SlotView, slot: usize) -> Option<Request> {
    can_repin(card).then_some(Request::Repin(slot))
}

/// Only a window that is open now can be closed.
fn is_open(card: &SlotView) -> bool {
    matches!(card.state, State::Open | State::InFront) && card.hwnd.is_some()
}

/// The numbers "Put at number" offers: all ten, free or taken, with the
/// card's own marked.
pub fn numbers(slot: usize) -> Vec<(usize, bool)> {
    (0..SLOTS)
        .map(|place| (number_of_slot(place), place == slot))
        .collect()
}

/// Draws the menu for `card`.
pub fn show(ui: &mut Ui, card: &SlotView) -> Option<Choice> {
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
                    for (place, (number, current)) in numbers(card.slot).into_iter().enumerate() {
                        let label = ui.add(egui::Button::selectable(current, number.to_string()));
                        if label.clicked() && !current {
                            chosen = Some(Choice::PutAt(place));
                        }
                    }
                });
            }
            Item::Repin => {
                if ui
                    .add(egui::Button::new(REPIN).shortcut_text("R"))
                    .clicked()
                {
                    chosen = Some(Choice::Repin);
                }
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
    fn re_pin_shows_and_works_only_on_a_card_that_is_not_open() {
        let closed = card(State::NotOpen, None);
        assert_eq!(
            items(&closed),
            vec![
                Item::Jump,
                Item::PutAt,
                Item::Repin,
                Item::Edit,
                Item::Unpin
            ]
        );
        assert_eq!(repin_request(&closed, 3), Some(Request::Repin(3)));
        assert_eq!(Choice::Repin.request(3), Some(Request::Repin(3)));
        let open = card(State::Open, Some(7));
        assert!(!items(&open).contains(&Item::Repin));
        assert_eq!(repin_request(&open, 3), None, "R does nothing here");
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
            Some(Request::PutAt { from: 1, to: 4 })
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
        let numbers = numbers(2);
        assert_eq!(numbers.len(), 10);
        assert_eq!(numbers[0], (1, false));
        assert_eq!(numbers[2], (3, true), "the card's own number");
        assert_eq!(numbers[9], (10, false), "free numbers can be picked too");
    }
}
