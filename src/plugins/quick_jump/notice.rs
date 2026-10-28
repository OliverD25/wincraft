//! What QuickJump's notices say. The notice itself is the shared one in
//! `core::notice`.

use crate::core::notice::Glyph;
pub use crate::core::notice::{click_point, Clock, Message, Notice, UNDO_MS};
pub use crate::core::notice::{WM_APP_NOTICE_CLICK, WM_APP_NOTICE_HOVER};

use super::pins::{digit_of_slot, Pinned};

/// QuickJump's own class for its notice window.
pub const CLASS_NAME: &str = "WinCraftQuickJumpNotice";

fn message(glyph: Glyph, text: String) -> Message {
    Message::new("QuickJump", glyph, text)
}

/// After a pin. A pin of the window into the slot it already has says so
/// without Undo, since nothing changed.
pub fn pinned(name: &str, slot: usize, done: &Pinned) -> Message {
    let digit = digit_of_slot(slot);
    match done {
        Pinned::New => {
            message(Glyph::Pin, format!("Pinned {name} to {digit}")).with_undo_key("Ctrl+Z")
        }
        Pinned::Replaced(old) => message(
            Glyph::Pin,
            format!("Pinned {name} to {digit} (was {})", old.name()),
        )
        .with_undo_key("Ctrl+Z"),
        Pinned::Moved { from } => moved(name, *from, slot),
        Pinned::Same => message(Glyph::Info, format!("{name} is already in {digit}")),
    }
}

pub fn moved(name: &str, from: usize, to: usize) -> Message {
    message(
        Glyph::Pin,
        format!(
            "Moved {name} from {} to {}",
            digit_of_slot(from),
            digit_of_slot(to)
        ),
    )
    .with_undo_key("Ctrl+Z")
}

pub fn removed(slot: usize) -> Message {
    message(
        Glyph::Trash,
        format!("Removed slot {}", digit_of_slot(slot)),
    )
    .with_undo_key("Ctrl+Z")
}

pub fn changed(name: &str, slot: usize) -> Message {
    message(
        Glyph::Pin,
        format!("Changed how {} finds {name}", digit_of_slot(slot)),
    )
    .with_undo_key("Ctrl+Z")
}

/// `pin_keys` is the pin shortcut as the user set it, or empty for none.
pub fn empty(slot: usize, pin_keys: &str) -> Message {
    let digit = digit_of_slot(slot);
    let message = message(Glyph::Info, format!("Slot {digit} is empty"));
    if pin_keys.is_empty() {
        message.detail("Open the pin list to pin a window.".to_string())
    } else {
        message.detail(format!(
            "{pin_keys}, then {digit} pins the window in front."
        ))
    }
}

pub fn starting(name: &str, slot: usize) -> Message {
    message(
        Glyph::Loader,
        format!("Starting {name} for slot {}\u{2026}", digit_of_slot(slot)),
    )
}

pub fn not_open(name: &str, slot: usize) -> Message {
    message(
        Glyph::Info,
        format!("{name} in slot {} is not open", digit_of_slot(slot)),
    )
    .detail("Its pin is set not to start the app.".to_string())
}

pub fn not_started(name: &str) -> Message {
    message(Glyph::Info, format!("{name} did not open a window"))
        .detail("QuickJump stopped waiting for it.".to_string())
}

pub fn nothing_to_pin() -> Message {
    message(Glyph::Info, "There is no window to pin".to_string())
        .detail("Click the window you want, then try again.".to_string())
}

pub fn no_keys() -> Message {
    message(
        Glyph::Info,
        "QuickJump cannot read the next key".to_string(),
    )
    .detail("Windows refused it, so the pin list opens instead.".to_string())
}

pub fn undone(what: &str) -> Message {
    message(Glyph::Info, "Undone".to_string()).detail(what.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::pins::tests::pin;
    use super::*;

    #[test]
    fn the_messages_name_the_app_and_the_digits() {
        assert_eq!(pinned("Claude", 2, &Pinned::New).text, "Pinned Claude to 3");
        let was = pinned("Claude", 2, &Pinned::Replaced(pin("Chrome", None)));
        assert_eq!(was.text, "Pinned Claude to 3 (was Chrome)");
        assert!(was.undo);
        let moved = pinned("Claude", 2, &Pinned::Moved { from: 5 });
        assert_eq!(moved.text, "Moved Claude from 6 to 3");
        assert!(moved.undo);
        assert!(!pinned("Claude", 9, &Pinned::Same).undo);
        assert_eq!(
            pinned("Claude", 9, &Pinned::Same).text,
            "Claude is already in 0"
        );
        assert_eq!(removed(3).text, "Removed slot 4");
        let empty = empty(4, "Win+Alt+Shift+Q");
        assert_eq!(empty.text, "Slot 5 is empty");
        assert_eq!(
            empty.detail.as_deref(),
            Some("Win+Alt+Shift+Q, then 5 pins the window in front.")
        );
        assert_eq!(
            starting("Claude", 1).text,
            "Starting Claude for slot 2\u{2026}"
        );
        assert!(super::empty(4, "").detail.is_some());
        for message in [
            not_open("Code", 0),
            not_started("Code"),
            nothing_to_pin(),
            no_keys(),
            undone("Pinned Claude to 3"),
            changed("Code", 0),
        ] {
            assert!(!message.text.is_empty());
        }
    }
}
