//! The pinned windows at the ten Quick access numbers, and every change to
//! them, with what Undo needs to put a change back. A pin keeps its number
//! until the user moves it: unpinning leaves the number free, and numbers
//! may be empty. Nothing here calls Windows, so all of it is tested.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The most windows the list holds: one per digit.
pub const SLOTS: usize = crate::core::quick_pins::SLOTS;

/// Where the file before 0.20.74 (ten fixed slots) is kept once it was
/// turned into the list.
pub const BACKUP_NAME: &str = "quick_jump.state.before-0.20.74.json";

/// The place a digit reaches: 1 to 9 are numbers 1 to 9, and 0 is number
/// 10, as with Win+number. Places are counted from 0 in the code.
pub fn slot_of_digit(digit: u8) -> Option<usize> {
    match digit {
        0 => Some(9),
        1..=9 => Some(digit as usize - 1),
        _ => None,
    }
}

/// The digit that reaches a place.
pub use crate::core::quick_pins::digit_of_slot;

/// The number the user reads for a place: 1 to 10.
pub fn number_of_slot(slot: usize) -> usize {
    slot + 1
}

fn yes() -> bool {
    true
}

fn is_yes(value: &bool) -> bool {
    *value
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
    /// Shown in the tip and the Quick access window instead of the app
    /// name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A jump to a pin with no open window starts the app.
    #[serde(default = "yes")]
    pub start: bool,
    /// The Claude chat the window showed (its `local_` sessionId): it
    /// finds the chat's window before the title does, and a jump with no
    /// window open opens the chat by its link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session: Option<String>,
    /// `title` follows the window's title while the window is open: it was
    /// the window's own title, not text the user typed. Old pins follow.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub title_auto: bool,
    /// The window while it lives; never saved, as Windows reuses handles.
    #[serde(skip)]
    pub hwnd: Option<isize>,
    /// `hwnd` is surely this pin's window: it was pinned, found by its chat
    /// or by its title alone, never by the "app's only window" rule.
    #[serde(skip)]
    pub sure: bool,
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

/// The list as it was before a change, so Undo can put it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Undo {
    before: Option<Vec<Option<Pin>>>,
}

/// What adding or placing a window did, for the notice and the tip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pinned {
    /// The window is new in the list, at this place.
    New { at: usize },
    /// The window was in the list and moved.
    Moved { from: usize, to: usize },
    /// The window is at that place already; nothing changed.
    Same { at: usize },
    /// All ten numbers are taken; nothing changed.
    Full,
}

impl Pinned {
    pub fn changed(&self) -> bool {
        matches!(self, Pinned::New { .. } | Pinned::Moved { .. })
    }
}

/// Puts `item` at place `at`. A free place simply takes it. A taken place
/// pushes its holder down one, and with it the run of taken places right
/// after, up to the first free place. With no free place after `at`, the
/// run right before `at` moves up one into the nearest free place instead.
/// False, with nothing changed, when no place is free.
pub fn put_at<T>(places: &mut [Option<T>], at: usize, item: T) -> bool {
    if at >= places.len() {
        return false;
    }
    if places[at].is_some() {
        if let Some(free) = (at + 1..places.len()).find(|&p| places[p].is_none()) {
            places[at..=free].rotate_right(1);
        } else if let Some(free) = (0..at).rev().find(|&p| places[p].is_none()) {
            places[free..=at].rotate_left(1);
        } else {
            return false;
        }
    }
    places[at] = Some(item);
    true
}

/// Moves the item at `from` to place `to` as a list moves it: the taken
/// places between them keep their places and the items on them shift one
/// towards `from`, so the gaps stay where they are. A free `to` simply
/// takes the item. False, with nothing changed, when there is nothing to
/// move.
pub fn reorder_at<T>(places: &mut [Option<T>], from: usize, to: usize) -> bool {
    if from == to || to >= places.len() || places.get(from).is_none_or(Option::is_none) {
        return false;
    }
    let (low, high) = (from.min(to), from.max(to));
    let taken: Vec<usize> = (low..=high).filter(|&p| places[p].is_some()).collect();
    if places[to].is_none() {
        places.swap(from, to);
    } else if from < to {
        taken
            .windows(2)
            .for_each(|pair| places.swap(pair[0], pair[1]));
    } else {
        taken
            .windows(2)
            .rev()
            .for_each(|pair| places.swap(pair[0], pair[1]));
    }
    true
}

/// The ten numbers, each empty or holding a pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slots {
    places: Vec<Option<Pin>>,
}

impl Default for Slots {
    fn default() -> Self {
        Self {
            places: vec![None; SLOTS],
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The pins in number order, with no gaps, as WinCraft 0.20.74 to
    /// 0.20.138 read them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pins: Option<Vec<Pin>>,
    /// The place of each pin in `pins`, counted from 0. Older versions
    /// leave it out and number the pins 1, 2, 3 and on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    numbers: Option<Vec<usize>>,
    /// The ten fixed slots of the versions before 0.20.74, empty ones as
    /// null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slots: Option<Vec<Option<Pin>>>,
}

impl Slots {
    pub fn get(&self, slot: usize) -> Option<&Pin> {
        self.places.get(slot)?.as_ref()
    }

    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Pin> {
        self.places.get_mut(slot)?.as_mut()
    }

    /// The pins with their places, in number order; empty places skipped.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Pin)> {
        self.places
            .iter()
            .enumerate()
            .filter_map(|(slot, pin)| Some((slot, pin.as_ref()?)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut Pin)> {
        self.places
            .iter_mut()
            .enumerate()
            .filter_map(|(slot, pin)| Some((slot, pin.as_mut()?)))
    }

    pub fn count(&self) -> usize {
        self.places.iter().flatten().count()
    }

    pub fn is_full(&self) -> bool {
        self.count() >= SLOTS
    }

    /// The lowest free place.
    pub fn first_free(&self) -> Option<usize> {
        self.places.iter().position(Option::is_none)
    }

    /// The place of this live window.
    pub fn slot_of_window(&self, hwnd: isize) -> Option<usize> {
        self.iter()
            .find(|(_, pin)| pin.hwnd == Some(hwnd))
            .map(|(slot, _)| slot)
    }

    fn record(&self) -> Undo {
        Undo {
            before: Some(self.places.clone()),
        }
    }

    /// Adds `pin` at the lowest free number. A window that is pinned
    /// already stays where it is, and a full list takes nothing.
    pub fn add(&mut self, pin: Pin) -> (Pinned, Undo) {
        if let Some(at) = pin.hwnd.and_then(|hwnd| self.slot_of_window(hwnd)) {
            return (Pinned::Same { at }, Undo::default());
        }
        let Some(at) = self.first_free() else {
            return (Pinned::Full, Undo::default());
        };
        let undo = self.record();
        self.places[at] = Some(pin);
        (Pinned::New { at }, undo)
    }

    /// Puts `pin` at place `slot` by the rule of `put_at`. A window that is
    /// pinned already leaves its old place free first and moves with what
    /// the user set for it. A full list takes no new window.
    pub fn pin(&mut self, slot: usize, pin: Pin) -> (Pinned, Undo) {
        let to = slot.min(SLOTS - 1);
        match pin.hwnd.and_then(|hwnd| self.slot_of_window(hwnd)) {
            Some(from) if from == to => (Pinned::Same { at: to }, Undo::default()),
            Some(from) => match self.move_to(from, to) {
                Some(undo) => (Pinned::Moved { from, to }, undo),
                None => (Pinned::Same { at: from }, Undo::default()),
            },
            None if self.is_full() => (Pinned::Full, Undo::default()),
            None => {
                let undo = self.record();
                put_at(&mut self.places, to, pin);
                (Pinned::New { at: to }, undo)
            }
        }
    }

    /// Takes a pin out; its number is free now and no other pin moves.
    /// None when there is no pin at that place.
    pub fn remove(&mut self, slot: usize) -> Option<(Pin, Undo)> {
        self.get(slot)?;
        let undo = self.record();
        Some((self.places[slot].take()?, undo))
    }

    /// Moves a pin to another place: its old place is free, then it is put
    /// there by the rule of `put_at`. None when nothing moved.
    pub fn move_to(&mut self, from: usize, to: usize) -> Option<Undo> {
        let to = to.min(SLOTS - 1);
        if from == to {
            return None;
        }
        self.get(from)?;
        let undo = self.record();
        let moving = self.places[from].take()?;
        put_at(&mut self.places, to, moving);
        Some(undo)
    }

    /// Quick access's drag and Ctrl+arrows, by the rule of `reorder_at`.
    /// None when nothing moved.
    pub fn reorder(&mut self, from: usize, to: usize) -> Option<Undo> {
        if from == to || to >= SLOTS {
            return None;
        }
        self.get(from)?;
        let undo = self.record();
        reorder_at(&mut self.places, from, to);
        Some(undo)
    }

    /// Quick access's "Re-pin": `pin` (a window) takes the place of the pin
    /// at `slot`, with that pin's label and start switch. A window that is
    /// pinned already leaves its old place free. None when there is no pin
    /// at `slot` or the window is that pin's already; else the place.
    pub fn repin(&mut self, slot: usize, mut pin: Pin) -> Option<(usize, Undo)> {
        self.get(slot)?;
        let from = pin.hwnd.and_then(|hwnd| self.slot_of_window(hwnd));
        if from == Some(slot) {
            return None;
        }
        let undo = self.record();
        let old = self.places[slot].take()?;
        let moving = from.and_then(|from| self.places[from].take());
        pin.label = old.label.or_else(|| moving.and_then(|moving| moving.label));
        pin.start = old.start;
        self.places[slot] = Some(pin);
        Some((slot, undo))
    }

    /// Changes a pin with `change`; the Undo puts the old one back.
    pub fn update(&mut self, slot: usize, change: impl FnOnce(&mut Pin)) -> Option<Undo> {
        let undo = self.record();
        change(self.get_mut(slot)?);
        Some(undo)
    }

    /// Puts back the list as it was before a change.
    pub fn undo(&mut self, undo: Undo) {
        if let Some(before) = undo.before {
            self.places = before;
        }
    }

    pub fn to_json(&self) -> String {
        let saved = Saved {
            pins: Some(self.iter().map(|(_, pin)| pin.clone()).collect()),
            numbers: Some(self.iter().map(|(slot, _)| slot).collect()),
            slots: None,
        };
        serde_json::to_string_pretty(&saved).unwrap_or_default()
    }

    /// The pins from a saved file, and whether the file was the old ten
    /// fixed slots, which keep their places. Pins saved without valid
    /// `numbers` take 1, 2, 3 and on. Anything it cannot read gives no
    /// pins.
    pub fn read(text: &str) -> (Self, bool) {
        let saved: Saved =
            serde_json::from_str(crate::core::config::strip_bom(text)).unwrap_or(Saved {
                pins: None,
                numbers: None,
                slots: None,
            });
        let (placed, old): (Vec<(usize, Pin)>, bool) = match (saved.pins, saved.slots) {
            (Some(pins), _) => {
                let numbers = saved
                    .numbers
                    .filter(|numbers| valid_numbers(numbers, pins.len()))
                    .unwrap_or_else(|| (0..pins.len()).collect());
                (numbers.into_iter().zip(pins).collect(), false)
            }
            (None, Some(slots)) => {
                let placed = slots.into_iter().enumerate();
                (
                    placed.filter_map(|(at, pin)| Some((at, pin?))).collect(),
                    true,
                )
            }
            (None, None) => (Vec::new(), false),
        };
        let mut slots = Self::default();
        for (at, pin) in placed {
            if at < SLOTS && !pin.exe.is_empty() {
                slots.places[at] = Some(pin);
            }
        }
        (slots, old)
    }

    /// Reads the saved pins at `path`. A file of the old ten slots is
    /// copied to `backup` first (once: a backup already there stays) and
    /// written again in the new form.
    pub fn load(path: &Path, backup: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let (slots, old) = Self::read(&text);
        if old {
            if !backup.exists() {
                if let Err(err) = std::fs::write(backup, &text) {
                    log::warn!("QuickJump: could not keep a copy of the old pins: {err}");
                    return slots;
                }
            }
            match crate::core::config::write_atomic(path, &slots.to_json()) {
                Ok(()) => log::info!(
                    "QuickJump: the old pins file is saved in the new form ({} pins); the old file is kept as {}",
                    slots.count(),
                    backup.display()
                ),
                Err(err) => log::warn!("QuickJump: could not save the pins in the new form: {err}"),
            }
        }
        slots
    }
}

/// One number per pin, each a place that exists, none twice.
fn valid_numbers(numbers: &[usize], pins: usize) -> bool {
    let mut seen = [false; SLOTS];
    numbers.len() == pins
        && numbers
            .iter()
            .all(|&at| at < SLOTS && !std::mem::replace(&mut seen[at], true))
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
            claude_session: None,
            title_auto: true,
            hwnd,
            sure: false,
        }
    }

    /// A list of these apps at numbers 1, 2, 3 and on, each with a window
    /// handle from 1 on.
    pub fn list(apps: &[&str]) -> Slots {
        let mut slots = Slots::default();
        for (index, app) in apps.iter().enumerate() {
            slots.add(pin(app, Some(index as isize + 1)));
        }
        slots
    }

    fn names(slots: &Slots) -> Vec<&str> {
        slots.iter().map(|(_, pin)| pin.name()).collect()
    }

    /// "1 A, 3 C": each pin's number and name.
    fn numbered(slots: &Slots) -> Vec<String> {
        slots
            .iter()
            .map(|(slot, pin)| format!("{} {}", number_of_slot(slot), pin.name()))
            .collect()
    }

    #[test]
    fn digit_0_is_number_10_and_the_others_their_own_number() {
        assert_eq!(slot_of_digit(1), Some(0));
        assert_eq!(slot_of_digit(9), Some(8));
        assert_eq!(slot_of_digit(0), Some(9));
        assert_eq!(slot_of_digit(10), None);
        for slot in 0..SLOTS {
            assert_eq!(slot_of_digit(digit_of_slot(slot)), Some(slot));
        }
        assert_eq!(number_of_slot(9), 10);
    }

    #[test]
    fn add_takes_the_lowest_free_number_and_undo_takes_it_away() {
        let mut slots = list(&["A", "B", "C"]);
        slots.remove(1);
        let before = slots.clone();
        let (done, undo) = slots.add(pin("D", Some(30)));
        assert_eq!(done, Pinned::New { at: 1 });
        assert_eq!(numbered(&slots), ["1 A", "2 D", "3 C"]);
        slots.undo(undo);
        assert_eq!(slots, before);
    }

    #[test]
    fn add_of_a_window_in_the_list_says_its_number_and_changes_nothing() {
        let mut slots = list(&["A", "B"]);
        let (done, undo) = slots.add(pin("B", Some(2)));
        assert_eq!(done, Pinned::Same { at: 1 });
        assert_eq!(undo, Undo::default());
        assert_eq!(slots.count(), 2);
    }

    #[test]
    fn a_full_list_takes_nothing_more() {
        let apps = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];
        let mut slots = list(&apps);
        assert!(slots.is_full());
        let before = slots.clone();
        let (done, undo) = slots.add(pin("K", Some(99)));
        assert_eq!(done, Pinned::Full);
        assert_eq!(undo, Undo::default());
        let (done, _) = slots.pin(0, pin("K", Some(99)));
        assert_eq!(done, Pinned::Full);
        assert_eq!(slots, before);
        // A window already in a full list can still move: the others in
        // between move up into the number it left.
        let (done, _) = slots.pin(0, pin("J", Some(10)));
        assert_eq!(done, Pinned::Moved { from: 9, to: 0 });
        assert_eq!(
            names(&slots),
            ["J", "A", "B", "C", "D", "E", "F", "G", "H", "I"]
        );
    }

    #[test]
    fn put_at_9_with_4_pins_lands_at_9_and_the_others_keep_their_numbers() {
        let mut slots = list(&["A", "B", "C", "D"]);
        let before = slots.clone();
        let (done, undo) = slots.pin(8, pin("N", Some(40)));
        assert_eq!(done, Pinned::New { at: 8 });
        assert_eq!(numbered(&slots), ["1 A", "2 B", "3 C", "4 D", "9 N"]);
        slots.undo(undo);
        assert_eq!(slots, before);
    }

    #[test]
    fn unpin_leaves_the_number_free_and_moves_nothing_else() {
        let mut slots = list(&["A", "B", "C", "D"]);
        let before = slots.clone();
        let (gone, undo) = slots.remove(1).expect("removed");
        assert_eq!(gone.name(), "B");
        assert_eq!(numbered(&slots), ["1 A", "3 C", "4 D"]);
        assert!(slots.get(1).is_none());
        assert!(slots.remove(1).is_none(), "number 2 is empty");
        slots.undo(undo);
        assert_eq!(slots, before);
    }

    #[test]
    fn put_at_a_taken_number_pushes_the_run_down_to_the_first_free_number() {
        let mut slots = Slots::default();
        for (at, app) in [(4, "E"), (5, "F"), (6, "G"), (9, "J")] {
            slots.pin(at, pin(app, Some(at as isize + 1)));
        }
        let (done, _) = slots.pin(4, pin("N", Some(40)));
        assert_eq!(done, Pinned::New { at: 4 });
        assert_eq!(numbered(&slots), ["5 N", "6 E", "7 F", "8 G", "10 J"]);
        // The pin at the number moves down to a free one right after it.
        let mut slots = list(&["A", "B"]);
        slots.pin(1, pin("N", Some(40)));
        assert_eq!(numbered(&slots), ["1 A", "2 N", "3 B"]);
    }

    #[test]
    fn a_taken_run_to_the_end_is_pushed_up_into_the_free_number_before() {
        let mut slots = Slots::default();
        for (at, app) in [(0, "A"), (7, "H"), (8, "I"), (9, "J")] {
            slots.pin(at, pin(app, Some(at as isize + 1)));
        }
        let (done, _) = slots.pin(7, pin("N", Some(40)));
        assert_eq!(done, Pinned::New { at: 7 });
        assert_eq!(numbered(&slots), ["1 A", "7 H", "8 N", "9 I", "10 J"]);
    }

    #[test]
    fn a_pinned_window_put_elsewhere_frees_its_old_number() {
        let mut slots = list(&["A", "B", "C", "D"]);
        if let Some(pin) = slots.get_mut(1) {
            pin.label = Some("Work".to_string());
        }
        let before = slots.clone();
        let (done, undo) = slots.pin(8, pin("B", Some(2)));
        assert_eq!(done, Pinned::Moved { from: 1, to: 8 });
        assert_eq!(numbered(&slots), ["1 A", "3 C", "4 D", "9 Work"]);
        slots.undo(undo);
        assert_eq!(slots, before);
        // To a taken number: its holder moves down into the freed one.
        let (done, _) = slots.pin(0, pin("B", Some(2)));
        assert_eq!(done, Pinned::Moved { from: 1, to: 0 });
        assert_eq!(numbered(&slots), ["1 Work", "2 A", "3 C", "4 D"]);
        let (done, undo) = slots.pin(0, pin("B", Some(2)));
        assert_eq!(done, Pinned::Same { at: 0 });
        assert_eq!(undo, Undo::default());
    }

    #[test]
    fn re_pin_puts_the_window_at_the_cards_number_with_its_label() {
        let mut slots = list(&["A", "B", "C"]);
        slots.get_mut(1).expect("B").hwnd = None;
        slots.get_mut(1).expect("B").label = Some("Mail".to_string());
        let before = slots.clone();
        let (at, undo) = slots.repin(1, pin("N", Some(40))).expect("re-pinned");
        assert_eq!(at, 1);
        assert_eq!(names(&slots), ["A", "Mail", "C"]);
        assert_eq!(slots.get(1).map(|p| p.app_name.as_str()), Some("N"));
        slots.undo(undo);
        assert_eq!(slots, before);
        // A window pinned elsewhere moves here and frees its number.
        let (at, _) = slots.repin(1, pin("C", Some(3))).expect("moved");
        assert_eq!(at, 1);
        assert_eq!(numbered(&slots), ["1 A", "2 Mail"]);
        assert!(slots.repin(5, pin("N", Some(40))).is_none(), "no pin there");
        assert!(
            slots.repin(0, pin("A", Some(1))).is_none(),
            "its own window"
        );
    }

    #[test]
    fn a_move_follows_the_put_rule_and_undo_puts_it_back() {
        let mut slots = list(&["A", "B", "C"]);
        let before = slots.clone();
        let undo = slots.move_to(0, 2).expect("moved");
        assert_eq!(numbered(&slots), ["2 B", "3 A", "4 C"]);
        slots.undo(undo);
        assert_eq!(slots, before);
        assert!(slots.move_to(1, 1).is_none());
        assert!(slots.move_to(5, 0).is_none(), "no pin there");
        slots.move_to(0, 99).expect("past the end is number 10");
        assert_eq!(numbered(&slots), ["2 B", "3 C", "10 A"]);
    }

    #[test]
    fn a_reorder_shifts_the_cards_between_and_keeps_the_gaps() {
        let mut slots = list(&["A", "B", "C"]);
        slots.pin(5, pin("F", Some(6)));
        let before = slots.clone();
        let undo = slots.reorder(0, 5).expect("moved down");
        assert_eq!(numbered(&slots), ["1 B", "2 C", "3 F", "6 A"]);
        slots.undo(undo);
        assert_eq!(slots, before);
        slots.reorder(5, 1).expect("moved up");
        assert_eq!(numbered(&slots), ["1 A", "2 F", "3 B", "6 C"]);
        slots
            .reorder(1, 2)
            .expect("next to it: the two trade numbers");
        assert_eq!(numbered(&slots), ["1 A", "2 B", "3 F", "6 C"]);
        slots.reorder(2, 3).expect("to a free number");
        assert_eq!(numbered(&slots), ["1 A", "2 B", "4 F", "6 C"]);
        assert!(slots.reorder(2, 4).is_none(), "no pin at 3");
        assert!(slots.reorder(0, 0).is_none());
        assert!(slots.reorder(0, SLOTS).is_none());
    }

    #[test]
    fn an_update_can_be_undone_and_a_blank_label_shows_the_app() {
        let mut slots = list(&["Code"]);
        let undo = slots
            .update(0, |pin| pin.label = Some("  ".to_string()))
            .expect("updated");
        assert_eq!(slots.get(0).map(Pin::name), Some("Code"));
        slots.undo(undo);
        assert_eq!(slots.get(0).and_then(|pin| pin.label.clone()), None);
        assert!(slots.update(5, |_| {}).is_none());
    }

    #[test]
    fn pins_survive_a_round_trip_with_their_numbers_and_without_handles() {
        let mut slots = Slots::default();
        let mut claude = pin("Claude", Some(7));
        claude.title = "Project".to_string();
        claude.app_id = Some("Claude.App".to_string());
        claude.start = false;
        slots.pin(2, claude);
        slots.pin(8, pin("Chrome", Some(8)));
        let json = slots.to_json();
        let (back, old) = Slots::read(&json);
        assert!(!old);
        assert_eq!(numbered(&back), ["3 Claude", "9 Chrome"]);
        let first = back.get(2).expect("number 3");
        assert_eq!(first.title, "Project");
        assert_eq!(first.app_id.as_deref(), Some("Claude.App"));
        assert!(!first.start);
        assert_eq!(first.hwnd, None);
        assert!(!json.contains("\"slots\""));
        // An older WinCraft reads the packed list.
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["pins"].as_array().map(Vec::len), Some(2));
        assert_eq!(value["numbers"], serde_json::json!([2, 8]));
    }

    #[test]
    fn pins_without_good_numbers_take_1_2_3() {
        let pins = r#"[{"exe":"C:\\a.exe","app_name":"A"},{"exe":"C:\\b.exe","app_name":"B"}]"#;
        for numbers in [
            "",
            r#","numbers":[1]"#,
            r#","numbers":[3,3]"#,
            r#","numbers":[0,10]"#,
        ] {
            let (slots, old) = Slots::read(&format!(r#"{{"pins":{pins}{numbers}}}"#));
            assert!(!old);
            assert_eq!(numbered(&slots), ["1 A", "2 B"], "{numbers}");
        }
        let (slots, _) = Slots::read(&format!(r#"{{"pins":{pins},"numbers":[9,4]}}"#));
        assert_eq!(numbered(&slots), ["5 B", "10 A"]);
    }

    #[test]
    fn the_old_ten_slots_keep_their_places() {
        let text = r#"{"slots":[null,{"exe":"C:\\b.exe","app_name":"B"},null,null,
            {"exe":"C:\\e.exe","app_name":"E","label":"Mail"},null,null,null,null,
            {"exe":"C:\\z.exe","app_name":"Z"}]}"#;
        let (slots, old) = Slots::read(text);
        assert!(old);
        assert_eq!(numbered(&slots), ["2 B", "5 Mail", "10 Z"]);
    }

    #[test]
    fn a_broken_or_odd_file_reads_as_what_it_can() {
        assert_eq!(Slots::read("not json").0, Slots::default());
        assert_eq!(Slots::read("{}").0, Slots::default());
        let text =
            r#"{"slots":[{"exe":"C:\\a.exe","app_name":"A"},null,{"exe":"","app_name":"B"}]}"#;
        let (slots, old) = Slots::read(&format!("\u{feff}{text}"));
        assert!(old);
        assert_eq!(slots.count(), 1);
        assert!(slots.get(0).expect("number 1").start, "on by default");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wincraft-quick-jump-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn loading_an_old_file_keeps_a_copy_and_saves_the_new_form() {
        let dir = temp_dir("migrate");
        let path = dir.join("quick_jump.state.json");
        let backup = dir.join(BACKUP_NAME);
        let old = r#"{"slots":[null,{"exe":"C:\\b.exe","app_name":"B"},null]}"#;
        std::fs::write(&path, old).expect("write");
        let slots = Slots::load(&path, &backup);
        assert_eq!(numbered(&slots), ["2 B"]);
        assert_eq!(std::fs::read_to_string(&backup).expect("backup"), old);
        let now = std::fs::read_to_string(&path).expect("saved");
        assert!(now.contains("\"pins\"") && !now.contains("\"slots\""));
        // Loading again changes nothing and keeps the first copy.
        std::fs::write(&backup, "first copy").expect("write");
        assert_eq!(Slots::load(&path, &backup), slots);
        assert_eq!(
            std::fs::read_to_string(&backup).expect("backup"),
            "first copy"
        );
        assert_eq!(Slots::load(&dir.join("none.json"), &backup).count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
