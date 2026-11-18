//! What ClaudeDeck asks of QuickJump through `core::quick_pins`: give a
//! Claude chat a Quick access number, or take it away. The same rules as
//! Put at number: a chat that has a number moves and leaves it free, and a
//! pin at the new number is pushed down. The chat need not have a window
//! open; its pin then opens the chat by its link.

use crate::core::quick_pins::{self, Ask, Held};
use crate::core::window_identity::{bare_title, exe_key};

use super::matching::Live;
use super::notice;
use super::pins::{Pin, Pinned, Slots, Undo};
use super::Engine;

const CLAUDE_EXE: &str = "claude.exe";
const CLAUDE_NAME: &str = "Claude";

/// What each number holds, for ClaudeDeck. Only a pin that knows its chat
/// gives a session.
pub fn held(slots: &Slots) -> Vec<Held> {
    slots
        .iter()
        .map(|(slot, pin)| Held {
            slot,
            name: shown_name(pin),
            session: pin.claude_session.clone(),
        })
        .collect()
}

/// The label, else the title (a chat's name), else the app's name.
fn shown_name(pin: &Pin) -> String {
    let label = pin.label.as_deref().map(str::trim).unwrap_or_default();
    let title = bare_title(&pin.title).trim();
    [label, title]
        .into_iter()
        .find(|text| !text.is_empty())
        .unwrap_or(&pin.app_name)
        .to_string()
}

/// The place of the pin of this chat.
pub fn slot_of_chat(slots: &Slots, session: &str) -> Option<usize> {
    slots
        .iter()
        .find(|(_, pin)| pin.claude_session.as_deref() == Some(session))
        .map(|(slot, _)| slot)
}

/// A pin for the chat: from its open window when there is one, else built
/// from the chat's name with the Claude app's program, taken from an open
/// window of the app or another chat pin when one is known.
pub fn chat_pin(
    session: &str,
    name: &str,
    window: Option<&Live>,
    live: &[Live],
    slots: &Slots,
) -> Pin {
    if let Some(window) = window {
        let mut pin = window.to_pin();
        pin.claude_session = Some(session.to_string());
        return pin;
    }
    let from_window = live
        .iter()
        .find(|window| exe_key(&window.exe) == CLAUDE_EXE)
        .map(|window| (window.exe.clone(), window.app_id.clone()));
    let from_pin = || {
        slots
            .iter()
            .find(|(_, pin)| pin.claude_session.is_some() && exe_key(&pin.exe) == CLAUDE_EXE)
            .map(|(_, pin)| (pin.exe.clone(), pin.app_id.clone()))
    };
    let (exe, app_id) = from_window
        .or_else(from_pin)
        .unwrap_or_else(|| (CLAUDE_EXE.to_string(), None));
    Pin {
        exe,
        app_id,
        app_name: CLAUDE_NAME.to_string(),
        title: name.trim().to_string(),
        label: None,
        start: true,
        claude_session: Some(session.to_string()),
        title_auto: true,
        hwnd: None,
        sure: false,
    }
}

/// Gives the chat of `pin` the number at `at`, or the lowest free one.
/// A chat that has a number moves there; "first free" leaves it where it
/// is.
pub fn put(slots: &mut Slots, pin: Pin, at: Option<usize>) -> (Pinned, Undo) {
    let session = pin.claude_session.clone().unwrap_or_default();
    match (slot_of_chat(slots, &session), at) {
        (Some(from), Some(to)) => match slots.move_to(from, to) {
            Some(undo) => (Pinned::Moved { from, to }, undo),
            None => (Pinned::Same { at: from }, Undo::default()),
        },
        (Some(from), None) => (Pinned::Same { at: from }, Undo::default()),
        (None, Some(to)) => slots.pin(to, pin),
        (None, None) => slots.add(pin),
    }
}

impl Engine {
    /// ClaudeDeck asked for changes.
    pub(super) fn deck_asks(&mut self) {
        for ask in quick_pins::take_asks() {
            match ask {
                Ask::Put {
                    session,
                    name,
                    window,
                    at,
                } => self.deck_put(&session, &name, window, at),
                Ask::Remove { session } => match slot_of_chat(&self.slots, &session) {
                    Some(slot) => self.unpin(slot),
                    None => log::info!("QuickJump: the chat to unpin has no number"),
                },
            }
        }
    }

    fn deck_put(&mut self, session: &str, name: &str, window: Option<isize>, at: Option<usize>) {
        self.refresh();
        let open = window.and_then(|hwnd| self.live.iter().find(|live| live.hwnd == hwnd));
        let pin = chat_pin(session, name, open, &self.live, &self.slots);
        let (done, undo) = put(&mut self.slots, pin, at);
        let shown = match done {
            Pinned::New { at } | Pinned::Moved { to: at, .. } => self
                .slots
                .get(at)
                .map_or_else(|| name.to_string(), |pin| pin.name().to_string()),
            _ => name.to_string(),
        };
        let message = notice::pinned(&shown, &done);
        log::info!("QuickJump: from ClaudeDeck: {}", message.text);
        if !done.changed() {
            self.notify(message, None);
            return;
        }
        self.save();
        self.refresh();
        let text = message.text.clone();
        self.notify(message, Some((undo, text)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::quick_jump::pins::tests::{list, pin};
    use crate::plugins::quick_jump::pins::SLOTS;

    fn chat(session: &str, name: &str) -> Pin {
        chat_pin(session, name, None, &[], &Slots::default())
    }

    fn sessions(slots: &Slots) -> Vec<Option<String>> {
        slots
            .iter()
            .map(|(_, pin)| pin.claude_session.clone())
            .collect()
    }

    #[test]
    fn pin_at_a_number_moves_a_chat_that_has_another_number() {
        let mut slots = list(&["a", "b"]);
        put(&mut slots, chat("local_x", "X"), None);
        assert_eq!(slot_of_chat(&slots, "local_x"), Some(2));
        let (done, undo) = put(&mut slots, chat("local_x", "X"), Some(0));
        assert_eq!(done, Pinned::Moved { from: 2, to: 0 });
        assert_eq!(slots.count(), 3, "moved, not added twice");
        assert_eq!(slot_of_chat(&slots, "local_x"), Some(0));
        slots.undo(undo);
        assert_eq!(slot_of_chat(&slots, "local_x"), Some(2));
    }

    #[test]
    fn pin_at_a_taken_number_pushes_that_pin_down() {
        let mut slots = list(&["a", "b"]);
        let (done, _) = put(&mut slots, chat("local_x", "X"), Some(1));
        assert_eq!(done, Pinned::New { at: 1 });
        assert_eq!(slots.get(2).map(|pin| pin.app_name.as_str()), Some("b"));
    }

    #[test]
    fn first_free_takes_the_lowest_free_number_and_keeps_a_numbered_chat() {
        let mut slots = list(&["a"]);
        let (done, _) = put(&mut slots, chat("local_x", "X"), None);
        assert_eq!(done, Pinned::New { at: 1 });
        let (again, _) = put(&mut slots, chat("local_x", "X"), None);
        assert_eq!(again, Pinned::Same { at: 1 });
        // A free number after the last pin is kept, not packed.
        let (far, _) = put(&mut slots, chat("local_y", "Y"), Some(8));
        assert_eq!(far, Pinned::New { at: 8 });
        slots.remove(0);
        let (first, _) = put(&mut slots, chat("local_z", "Z"), None);
        assert_eq!(first, Pinned::New { at: 0 });
    }

    #[test]
    fn at_9_with_3_pins_gives_number_9() {
        let mut slots = list(&["a", "b", "c"]);
        let (done, _) = put(&mut slots, chat("local_x", "X"), Some(8));
        assert_eq!(done, Pinned::New { at: 8 });
        assert_eq!(held(&slots).last().map(|held| held.slot), Some(8));
        let (moved, _) = put(&mut slots, chat("local_x", "X"), Some(5));
        assert_eq!(moved, Pinned::Moved { from: 8, to: 5 });
        assert_eq!(slot_of_chat(&slots, "local_x"), Some(5));
    }

    #[test]
    fn a_full_list_takes_no_new_chat() {
        let apps: Vec<String> = (0..SLOTS).map(|n| format!("app{n}")).collect();
        let names: Vec<&str> = apps.iter().map(String::as_str).collect();
        let mut slots = list(&names);
        let (done, _) = put(&mut slots, chat("local_x", "X"), None);
        assert_eq!(done, Pinned::Full);
        assert_eq!(slot_of_chat(&slots, "local_x"), None);
    }

    #[test]
    fn a_chat_with_no_window_gets_a_pin_that_opens_it_by_its_link() {
        let made = chat("local_x", " WinCraft Rust Master ");
        assert_eq!(made.exe, "claude.exe");
        assert_eq!(made.app_name, "Claude");
        assert_eq!(made.title, "WinCraft Rust Master");
        assert_eq!(made.claude_session.as_deref(), Some("local_x"));
        assert!(made.title_auto && made.start);
        assert_eq!(made.hwnd, None);
        // The program comes from another chat pin when one is known.
        let mut slots = Slots::default();
        let mut known = pin("claude", None);
        known.claude_session = Some("local_old".into());
        slots.add(known);
        let made = chat_pin("local_x", "X", None, &[], &slots);
        assert_eq!(made.exe, r"C:\Apps\claude.exe");
    }

    #[test]
    fn the_published_list_has_a_session_only_for_chat_pins() {
        let mut slots = list(&["notepad"]);
        put(&mut slots, chat("local_x", "WinCraft Rust Master"), None);
        let published = held(&slots);
        assert_eq!(published[0].session, None);
        assert_eq!(published[1].session.as_deref(), Some("local_x"));
        assert_eq!(published[1].name, "WinCraft Rust Master");
        assert_eq!(sessions(&slots), vec![None, Some("local_x".to_string())]);
    }
}
