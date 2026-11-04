//! The pinned windows as an ordered list of at most ten, and every change
//! to it, with what Undo needs to put a change back. A pin's number is its
//! place in the list: unpinning closes the gap. Nothing here calls Windows,
//! so all of it is tested.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The most windows the list holds: one per digit.
pub const SLOTS: usize = 10;

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
pub fn digit_of_slot(slot: usize) -> u8 {
    if slot == 9 {
        0
    } else {
        slot as u8 + 1
    }
}

/// The number the user reads for a place: 1 to 10.
pub fn number_of_slot(slot: usize) -> usize {
    slot + 1
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
    /// Shown in the tip and the Quick access window instead of the app
    /// name.
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

/// The list as it was before a change, so Undo can put it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Undo {
    before: Option<Vec<Pin>>,
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
    /// Ten windows are in the list; nothing changed.
    Full,
}

impl Pinned {
    pub fn changed(&self) -> bool {
        matches!(self, Pinned::New { .. } | Pinned::Moved { .. })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slots {
    pins: Vec<Pin>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    /// The list, in order (0.20.74 on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pins: Option<Vec<Pin>>,
    /// The ten fixed slots of the versions before, empty ones as null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slots: Option<Vec<Option<Pin>>>,
}

impl Slots {
    pub fn get(&self, slot: usize) -> Option<&Pin> {
        self.pins.get(slot)
    }

    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Pin> {
        self.pins.get_mut(slot)
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &Pin)> {
        self.pins.iter().enumerate()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut Pin)> {
        self.pins.iter_mut().enumerate()
    }

    pub fn count(&self) -> usize {
        self.pins.len()
    }

    pub fn is_full(&self) -> bool {
        self.pins.len() >= SLOTS
    }

    /// The place of this live window in the list.
    pub fn slot_of_window(&self, hwnd: isize) -> Option<usize> {
        self.pins.iter().position(|pin| pin.hwnd == Some(hwnd))
    }

    fn record(&self) -> Undo {
        Undo {
            before: Some(self.pins.clone()),
        }
    }

    /// Adds `pin` to the end of the list. A window that is in the list
    /// already stays where it is, and a full list takes nothing.
    pub fn add(&mut self, pin: Pin) -> (Pinned, Undo) {
        if let Some(at) = pin.hwnd.and_then(|hwnd| self.slot_of_window(hwnd)) {
            return (Pinned::Same { at }, Undo::default());
        }
        if self.is_full() {
            return (Pinned::Full, Undo::default());
        }
        let undo = self.record();
        self.pins.push(pin);
        (
            Pinned::New {
                at: self.pins.len() - 1,
            },
            undo,
        )
    }

    /// Puts `pin` at place `slot`: the pin there and the ones after it move
    /// down one. A window that is in the list already moves from its old
    /// place, with what the user set for it. A place past the end is the
    /// end. A full list takes no new window.
    pub fn pin(&mut self, slot: usize, pin: Pin) -> (Pinned, Undo) {
        let from = pin.hwnd.and_then(|hwnd| self.slot_of_window(hwnd));
        match from {
            Some(from) => {
                let to = slot.min(self.pins.len() - 1);
                if from == to {
                    return (Pinned::Same { at: to }, Undo::default());
                }
                let undo = self.record();
                let moving = self.pins.remove(from);
                self.pins.insert(to, moving);
                (Pinned::Moved { from, to }, undo)
            }
            None if self.is_full() => (Pinned::Full, Undo::default()),
            None => {
                let to = slot.min(self.pins.len());
                let undo = self.record();
                self.pins.insert(to, pin);
                (Pinned::New { at: to }, undo)
            }
        }
    }

    /// Takes a pin out; the ones after it move up one. None when there is
    /// no pin at that place.
    pub fn remove(&mut self, slot: usize) -> Option<(Pin, Undo)> {
        if slot >= self.pins.len() {
            return None;
        }
        let undo = self.record();
        Some((self.pins.remove(slot), undo))
    }

    /// Moves a pin to another place, the others closing up around it. A
    /// place past the end is the end. None when nothing moved.
    pub fn move_to(&mut self, from: usize, to: usize) -> Option<Undo> {
        if from >= self.pins.len() {
            return None;
        }
        let to = to.min(self.pins.len() - 1);
        if from == to {
            return None;
        }
        let undo = self.record();
        let moving = self.pins.remove(from);
        self.pins.insert(to, moving);
        Some(undo)
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
            self.pins = before;
        }
    }

    pub fn to_json(&self) -> String {
        let saved = Saved {
            pins: Some(self.pins.clone()),
            slots: None,
        };
        serde_json::to_string_pretty(&saved).unwrap_or_default()
    }

    /// The list from a saved file, and whether the file was the old ten
    /// fixed slots: those keep their order and the empty ones drop out.
    /// Anything it cannot read gives no pins.
    pub fn read(text: &str) -> (Self, bool) {
        let saved: Saved =
            serde_json::from_str(crate::core::config::strip_bom(text)).unwrap_or(Saved {
                pins: None,
                slots: None,
            });
        let (pins, old): (Vec<Pin>, bool) = match (saved.pins, saved.slots) {
            (Some(pins), _) => (pins, false),
            (None, Some(slots)) => (slots.into_iter().flatten().collect(), true),
            (None, None) => (Vec::new(), false),
        };
        let pins = pins
            .into_iter()
            .filter(|pin| !pin.exe.is_empty())
            .take(SLOTS)
            .collect();
        (Self { pins }, old)
    }

    /// Reads the saved list at `path`. A file of the old ten slots is
    /// copied to `backup` first (once: a backup already there stays) and
    /// written again as the list.
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
                    "QuickJump: the ten slots are a list now ({} pins); the old file is kept as {}",
                    slots.count(),
                    backup.display()
                ),
                Err(err) => log::warn!("QuickJump: could not save the pins as a list: {err}"),
            }
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

    /// A list of these apps, each with a window handle from 1 on.
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
    fn add_goes_to_the_end_and_undo_takes_it_away() {
        let mut slots = list(&["A", "B"]);
        let before = slots.clone();
        let (done, undo) = slots.add(pin("C", Some(30)));
        assert_eq!(done, Pinned::New { at: 2 });
        assert_eq!(names(&slots), ["A", "B", "C"]);
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
        // A window already in a full list can still move.
        let (done, _) = slots.pin(0, pin("J", Some(10)));
        assert_eq!(done, Pinned::Moved { from: 9, to: 0 });
        assert_eq!(slots.count(), SLOTS);
    }

    #[test]
    fn put_at_a_number_pushes_that_pin_and_the_later_ones_down() {
        let mut slots = list(&["A", "B", "C"]);
        let before = slots.clone();
        let (done, undo) = slots.pin(1, pin("N", Some(40)));
        assert_eq!(done, Pinned::New { at: 1 });
        assert_eq!(names(&slots), ["A", "N", "B", "C"]);
        slots.undo(undo);
        assert_eq!(slots, before);
    }

    #[test]
    fn put_past_the_end_goes_to_the_end() {
        let mut slots = list(&["A", "B", "C"]);
        let (done, _) = slots.pin(6, pin("N", Some(40)));
        assert_eq!(done, Pinned::New { at: 3 });
        assert_eq!(names(&slots), ["A", "B", "C", "N"]);
    }

    #[test]
    fn put_of_a_window_in_the_list_moves_it_with_its_label() {
        let mut slots = list(&["A", "B", "C", "D"]);
        if let Some(pin) = slots.get_mut(3) {
            pin.label = Some("Work".to_string());
        }
        let before = slots.clone();
        let (done, undo) = slots.pin(0, pin("D", Some(4)));
        assert_eq!(done, Pinned::Moved { from: 3, to: 0 });
        assert_eq!(names(&slots), ["Work", "A", "B", "C"]);
        slots.undo(undo);
        assert_eq!(slots, before);
        // Down the list, and past its end.
        let (done, _) = slots.pin(8, pin("A", Some(1)));
        assert_eq!(done, Pinned::Moved { from: 0, to: 3 });
        assert_eq!(names(&slots), ["B", "C", "Work", "A"]);
        let (done, undo) = slots.pin(3, pin("A", Some(1)));
        assert_eq!(done, Pinned::Same { at: 3 });
        assert_eq!(undo, Undo::default());
    }

    #[test]
    fn unpin_closes_the_gap_and_undo_opens_it_again() {
        let mut slots = list(&["A", "B", "C"]);
        let before = slots.clone();
        let (gone, undo) = slots.remove(1).expect("removed");
        assert_eq!(gone.name(), "B");
        assert_eq!(names(&slots), ["A", "C"]);
        assert!(slots.remove(2).is_none());
        slots.undo(undo);
        assert_eq!(slots, before);
    }

    #[test]
    fn a_move_reorders_and_undo_puts_the_order_back() {
        let mut slots = list(&["A", "B", "C"]);
        let before = slots.clone();
        let undo = slots.move_to(0, 2).expect("moved");
        assert_eq!(names(&slots), ["B", "C", "A"]);
        slots.undo(undo);
        assert_eq!(slots, before);
        assert!(slots.move_to(1, 1).is_none());
        assert!(slots.move_to(5, 0).is_none(), "no pin there");
        slots.move_to(0, 9).expect("past the end is the end");
        assert_eq!(names(&slots), ["B", "C", "A"]);
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
    fn pins_survive_a_round_trip_without_their_window_handles() {
        let mut slots = Slots::default();
        let mut claude = pin("Claude", Some(7));
        claude.title = "Project".to_string();
        claude.app_id = Some("Claude.App".to_string());
        claude.start = false;
        slots.add(claude);
        slots.add(pin("Chrome", Some(8)));
        let (back, old) = Slots::read(&slots.to_json());
        assert!(!old);
        assert_eq!(back.count(), 2);
        let first = back.get(0).expect("number 1");
        assert_eq!(first.title, "Project");
        assert_eq!(first.app_id.as_deref(), Some("Claude.App"));
        assert!(!first.start);
        assert_eq!(first.hwnd, None);
        assert_eq!(back.get(1).map(Pin::name), Some("Chrome"));
        assert!(!slots.to_json().contains("\"slots\""));
    }

    #[test]
    fn the_old_ten_slots_become_the_list_in_their_order() {
        let text = r#"{"slots":[null,{"exe":"C:\\b.exe","app_name":"B"},null,null,
            {"exe":"C:\\e.exe","app_name":"E","label":"Mail"},null,null,null,null,
            {"exe":"C:\\z.exe","app_name":"Z"}]}"#;
        let (slots, old) = Slots::read(text);
        assert!(old);
        assert_eq!(names(&slots), ["B", "Mail", "Z"]);
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
    fn loading_an_old_file_keeps_a_copy_and_saves_the_list() {
        let dir = temp_dir("migrate");
        let path = dir.join("quick_jump.state.json");
        let backup = dir.join(BACKUP_NAME);
        let old = r#"{"slots":[null,{"exe":"C:\\b.exe","app_name":"B"},null]}"#;
        std::fs::write(&path, old).expect("write");
        let slots = Slots::load(&path, &backup);
        assert_eq!(names(&slots), ["B"]);
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
