//! The ten slots and every change to them, with what Undo needs to put a
//! change back. Nothing here calls Windows, so all of it is tested.

use serde::{Deserialize, Serialize};

pub const SLOTS: usize = 10;

/// The slot a digit reaches: 1 to 9 are slots 1 to 9, and 0 is slot 10, as
/// with Win+number. Slots are counted from 0 in the code.
pub fn slot_of_digit(digit: u8) -> Option<usize> {
    match digit {
        0 => Some(9),
        1..=9 => Some(digit as usize - 1),
        _ => None,
    }
}

/// The digit that reaches a slot.
pub fn digit_of_slot(slot: usize) -> u8 {
    if slot == 9 {
        0
    } else {
        slot as u8 + 1
    }
}

fn yes() -> bool {
    true
}

/// A pinned window: how to find it again, and what to call it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// The program's full path, which starts it when it is not open.
    pub exe: String,
    /// The AppUserModelID, which tells apart a browser's web apps and
    /// starts a Store app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    /// What the app is called: "Claude".
    pub app_name: String,
    /// The window's title must hold this; empty for any window of the app.
    #[serde(default)]
    pub title: String,
    /// Shown in the strip and the list instead of the app name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A jump to a pin with no open window starts the app.
    #[serde(default = "yes")]
    pub start: bool,
    /// The window while it lives; never saved, as Windows reuses handles.
    #[serde(skip)]
    pub hwnd: Option<isize>,
}

impl Pin {
    /// The label when there is one, else the app's name.
    pub fn name(&self) -> &str {
        self.label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .unwrap_or(&self.app_name)
    }
}

/// What was in the slots a change touched, so Undo can put it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Undo {
    before: Vec<(usize, Option<Pin>)>,
}

impl Undo {
    pub fn is_empty(&self) -> bool {
        self.before.is_empty()
    }
}

/// What a pin did, for the notice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pinned {
    /// Into an empty slot.
    New,
    /// Over another pin, which is gone now.
    Replaced(Pin),
    /// The window was in another slot and moved.
    Moved { from: usize },
    /// The window is in that slot already; nothing changed.
    Same,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slots {
    slots: [Option<Pin>; SLOTS],
}

#[derive(Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    slots: Vec<Option<Pin>>,
}

impl Slots {
    pub fn get(&self, slot: usize) -> Option<&Pin> {
        self.slots.get(slot).and_then(Option::as_ref)
    }

    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Pin> {
        self.slots.get_mut(slot).and_then(Option::as_mut)
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &Pin)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(slot, pin)| pin.as_ref().map(|pin| (slot, pin)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut Pin)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(slot, pin)| pin.as_mut().map(|pin| (slot, pin)))
    }

    pub fn count(&self) -> usize {
        self.iter().count()
    }

    /// The slot that holds this live window.
    pub fn slot_of_window(&self, hwnd: isize) -> Option<usize> {
        self.iter()
            .find(|(_, pin)| pin.hwnd == Some(hwnd))
            .map(|(slot, _)| slot)
    }

    fn record(&self, touched: &[usize]) -> Undo {
        let mut before: Vec<(usize, Option<Pin>)> = Vec::new();
        for slot in touched {
            if !before.iter().any(|(seen, _)| seen == slot) {
                before.push((*slot, self.slots[*slot].clone()));
            }
        }
        Undo { before }
    }

    /// Pins `pin` to `slot`. A window that is in another slot already moves
    /// there with what the user set for it, and its old slot empties.
    pub fn pin(&mut self, slot: usize, pin: Pin) -> (Pinned, Undo) {
        let from = pin.hwnd.and_then(|hwnd| self.slot_of_window(hwnd));
        if from == Some(slot) {
            return (Pinned::Same, Undo::default());
        }
        let undo = self.record(&[slot, from.unwrap_or(slot)]);
        let old = self.slots[slot].take();
        let done = match from {
            Some(from) => {
                self.slots[slot] = Some(self.slots[from].take().unwrap_or(pin));
                Pinned::Moved { from }
            }
            None => {
                self.slots[slot] = Some(pin);
                match old {
                    Some(old) => Pinned::Replaced(old),
                    None => Pinned::New,
                }
            }
        };
        (done, undo)
    }

    /// Empties a slot; None when it was empty.
    pub fn remove(&mut self, slot: usize) -> Option<(Pin, Undo)> {
        let undo = self.record(&[slot]);
        self.slots.get_mut(slot)?.take().map(|pin| (pin, undo))
    }

    /// Moves a slot's pin to another slot, swapping with the pin there.
    /// None when nothing moved.
    pub fn move_to(&mut self, from: usize, to: usize) -> Option<Undo> {
        if from == to || from >= SLOTS || to >= SLOTS || self.slots[from].is_none() {
            return None;
        }
        let undo = self.record(&[from, to]);
        self.slots.swap(from, to);
        Some(undo)
    }

    /// Changes a pin with `change`; the Undo puts the old one back.
    pub fn update(&mut self, slot: usize, change: impl FnOnce(&mut Pin)) -> Option<Undo> {
        let undo = self.record(&[slot]);
        change(self.get_mut(slot)?);
        Some(undo)
    }

    /// Puts back what a change replaced.
    pub fn undo(&mut self, undo: Undo) {
        for (slot, pin) in undo.before {
            self.slots[slot] = pin;
        }
    }

    pub fn to_json(&self) -> String {
        let saved = Saved {
            slots: self.slots.to_vec(),
        };
        serde_json::to_string_pretty(&saved).unwrap_or_default()
    }

    /// Slots from a saved file; anything it cannot read gives no pins.
    pub fn from_json(text: &str) -> Self {
        let saved: Saved = serde_json::from_str(crate::core::config::strip_bom(text))
            .unwrap_or(Saved { slots: Vec::new() });
        let mut slots = Self::default();
        for (slot, pin) in saved.slots.into_iter().take(SLOTS).enumerate() {
            slots.slots[slot] = pin.filter(|pin| !pin.exe.is_empty());
        }
        slots
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn pin(app: &str, hwnd: Option<isize>) -> Pin {
        Pin {
            exe: format!(r"C:\Apps\{app}.exe"),
            app_id: None,
            app_name: app.to_string(),
            title: String::new(),
            label: None,
            start: true,
            hwnd,
        }
    }

    #[test]
    fn digit_0_is_slot_10_and_the_others_their_own_number() {
        assert_eq!(slot_of_digit(1), Some(0));
        assert_eq!(slot_of_digit(9), Some(8));
        assert_eq!(slot_of_digit(0), Some(9));
        assert_eq!(slot_of_digit(10), None);
        for slot in 0..SLOTS {
            assert_eq!(slot_of_digit(digit_of_slot(slot)), Some(slot));
        }
    }

    #[test]
    fn a_pin_into_an_empty_slot_undoes_to_empty() {
        let mut slots = Slots::default();
        let (done, undo) = slots.pin(2, pin("Claude", Some(7)));
        assert_eq!(done, Pinned::New);
        assert_eq!(slots.get(2).map(Pin::name), Some("Claude"));
        assert_eq!(slots.count(), 1);
        slots.undo(undo);
        assert_eq!(slots, Slots::default());
    }

    #[test]
    fn a_pin_into_a_full_slot_replaces_and_undo_brings_the_old_one_back() {
        let mut slots = Slots::default();
        slots.pin(2, pin("Chrome", Some(5)));
        let before = slots.clone();
        let (done, undo) = slots.pin(2, pin("Claude", Some(7)));
        assert_eq!(done, Pinned::Replaced(pin("Chrome", Some(5))));
        assert_eq!(slots.get(2).map(Pin::name), Some("Claude"));
        slots.undo(undo);
        assert_eq!(slots, before);
    }

    #[test]
    fn a_window_pinned_elsewhere_moves_with_its_label() {
        let mut slots = Slots::default();
        let mut labelled = pin("Claude", Some(7));
        labelled.label = Some("Work chat".to_string());
        slots.pin(5, labelled);
        slots.pin(2, pin("Chrome", Some(9)));
        let before = slots.clone();
        let (done, undo) = slots.pin(2, pin("Claude", Some(7)));
        assert_eq!(done, Pinned::Moved { from: 5 });
        assert!(slots.get(5).is_none());
        assert_eq!(slots.get(2).map(Pin::name), Some("Work chat"));
        assert_eq!(slots.count(), 1);
        slots.undo(undo);
        assert_eq!(slots, before);
    }

    #[test]
    fn pinning_a_window_to_its_own_slot_changes_nothing() {
        let mut slots = Slots::default();
        slots.pin(3, pin("Claude", Some(7)));
        let (done, undo) = slots.pin(3, pin("Claude", Some(7)));
        assert_eq!(done, Pinned::Same);
        assert!(undo.is_empty());
    }

    #[test]
    fn a_move_swaps_with_a_full_slot_and_undo_swaps_back() {
        let mut slots = Slots::default();
        slots.pin(0, pin("A", None));
        slots.pin(1, pin("B", None));
        let before = slots.clone();
        let undo = slots.move_to(0, 1).expect("moved");
        assert_eq!(slots.get(0).map(Pin::name), Some("B"));
        assert_eq!(slots.get(1).map(Pin::name), Some("A"));
        slots.undo(undo);
        assert_eq!(slots, before);
        // Into an empty slot it is a plain move.
        slots.move_to(1, 9).expect("moved");
        assert!(slots.get(1).is_none());
        assert_eq!(slots.get(9).map(Pin::name), Some("B"));
        assert!(slots.move_to(4, 5).is_none(), "an empty slot does not move");
        assert!(slots.move_to(9, 9).is_none());
        assert!(slots.move_to(9, 10).is_none());
    }

    #[test]
    fn remove_empties_the_slot_and_undo_fills_it_again() {
        let mut slots = Slots::default();
        slots.pin(4, pin("Code", None));
        let (gone, undo) = slots.remove(4).expect("removed");
        assert_eq!(gone.name(), "Code");
        assert!(slots.get(4).is_none());
        assert!(slots.remove(4).is_none());
        slots.undo(undo);
        assert_eq!(slots.get(4).map(Pin::name), Some("Code"));
    }

    #[test]
    fn an_update_can_be_undone_and_a_blank_label_shows_the_app() {
        let mut slots = Slots::default();
        slots.pin(0, pin("Code", None));
        let undo = slots
            .update(0, |pin| pin.label = Some("  ".to_string()))
            .expect("updated");
        assert_eq!(slots.get(0).map(Pin::name), Some("Code"));
        slots.undo(undo);
        assert_eq!(slots.get(0).and_then(|pin| pin.label.clone()), None);
        assert!(slots.update(5, |_| {}).is_none());
    }

    #[test]
    fn pins_survive_a_round_trip_without_their_window_handles() {
        let mut slots = Slots::default();
        let mut claude = pin("Claude", Some(7));
        claude.title = "Project".to_string();
        claude.app_id = Some("Claude.App".to_string());
        claude.start = false;
        slots.pin(1, claude);
        slots.pin(9, pin("Chrome", Some(8)));
        let back = Slots::from_json(&slots.to_json());
        assert_eq!(back.count(), 2);
        let first = back.get(1).expect("slot 2");
        assert_eq!(first.title, "Project");
        assert_eq!(first.app_id.as_deref(), Some("Claude.App"));
        assert!(!first.start);
        assert_eq!(first.hwnd, None);
        assert_eq!(back.get(9).map(Pin::name), Some("Chrome"));
    }

    #[test]
    fn a_broken_or_odd_file_reads_as_what_it_can() {
        assert_eq!(Slots::from_json("not json"), Slots::default());
        assert_eq!(Slots::from_json("{}"), Slots::default());
        let text =
            r#"{"slots":[{"exe":"C:\\a.exe","app_name":"A"},null,{"exe":"","app_name":"B"}]}"#;
        let slots = Slots::from_json(&format!("\u{feff}{text}"));
        assert_eq!(slots.count(), 1);
        assert!(
            slots.get(0).expect("slot 1").start,
            "start is on by default"
        );
    }
}
