//! What QuickJump's notices say. The notice itself is the shared one in
//! `core::notice`.

use crate::core::notice::Glyph;
pub use crate::core::notice::{click_point, Clock, Message, Notice, UNDO_MS};
pub use crate::core::notice::{WM_APP_NOTICE_CLICK, WM_APP_NOTICE_HOVER};

use super::pins::{number_of_slot, Pinned};

/// QuickJump's own class for its notice window.
pub const CLASS_NAME: &str = "WinCraftQuickJumpNotice";

fn message(glyph: Glyph, text: String) -> Message {
    Message::new("QuickJump", glyph, text)
}

/// After adding or placing a window. One that is where it was asked to go
/// already, or a full list, says so without Undo, since nothing changed.
pub fn pinned(name: &str, done: &Pinned) -> Message {
    match done {
        Pinned::New { at } => message(
            Glyph::Pin,
            format!("Added {name} as number {}", number_of_slot(*at)),
        )
        .with_undo_key("Ctrl+Z"),
        Pinned::Moved { from, to } => moved(name, *from, *to),
        Pinned::Same { at } => message(Glyph::Info, already(*at)),
        Pinned::Full => message(Glyph::Info, FULL.to_string()),
    }
}

/// What the tip and a notice say when Quick access holds ten windows.
pub const FULL: &str = "Quick access is full";

/// What the tip and a notice say when the window is in the list already.
pub fn already(at: usize) -> String {
    format!("Already number {}", number_of_slot(at))
}

pub fn moved(name: &str, from: usize, to: usize) -> Message {
    message(
        Glyph::Pin,
        format!(
            "Moved {name} from {} to {}",
            number_of_slot(from),
            number_of_slot(to)
        ),
    )
    .with_undo_key("Ctrl+Z")
}

pub fn removed(name: &str, slot: usize) -> Message {
    message(
        Glyph::Trash,
        format!("Unpinned {name} (was number {})", number_of_slot(slot)),
    )
    .with_undo_key("Ctrl+Z")
}

pub fn changed(name: &str, slot: usize) -> Message {
    message(
        Glyph::Pin,
        format!("Changed how number {} finds {name}", number_of_slot(slot)),
    )
    .with_undo_key("Ctrl+Z")
}

/// `jump_keys` is the jump shortcut as the user set it, or empty for none.
pub fn empty(slot: usize, jump_keys: &str) -> Message {
    let message = message(
        Glyph::Info,
        format!("Number {} is empty", number_of_slot(slot)),
    );
    if jump_keys.is_empty() {
        message.detail("Open Quick access to see the pinned windows.".to_string())
    } else {
        message.detail(format!("{jump_keys}, then A adds the window in front."))
    }
}

pub fn starting(name: &str, slot: usize) -> Message {
    message(
        Glyph::Loader,
        format!(
            "Starting {name} for number {}\u{2026}",
            number_of_slot(slot)
        ),
    )
}

pub fn not_open(name: &str, slot: usize) -> Message {
    message(
        Glyph::Info,
        format!("{name} at number {} is not open", number_of_slot(slot)),
    )
    .detail("Its pin is set not to start the app.".to_string())
}

pub fn not_started(name: &str) -> Message {
    message(Glyph::Info, format!("{name} did not open a window"))
        .detail("QuickJump stopped waiting for it.".to_string())
}

pub fn no_keys() -> Message {
    message(
        Glyph::Info,
        "QuickJump cannot read the next key".to_string(),
    )
    .detail("Windows refused it, so Quick access opens instead.".to_string())
}

/// What "Add this window" says when no window is in front to add.
pub fn no_window() -> Message {
    message(Glyph::Info, "There is no window to pin".to_string())
}

pub fn undone(what: &str) -> Message {
    message(Glyph::Info, "Undone".to_string()).detail(what.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_messages_name_the_app_and_the_numbers() {
        let added = pinned("Claude", &Pinned::New { at: 2 });
        assert_eq!(added.text, "Added Claude as number 3");
        assert!(added.has_button());
        let moved = pinned("Claude", &Pinned::Moved { from: 5, to: 2 });
        assert_eq!(moved.text, "Moved Claude from 6 to 3");
        assert!(moved.has_button());
        let same = pinned("Claude", &Pinned::Same { at: 9 });
        assert!(!same.has_button());
        assert_eq!(same.text, "Already number 10");
        assert_eq!(pinned("Claude", &Pinned::Full).text, FULL);
        assert!(!pinned("Claude", &Pinned::Full).has_button());
        assert_eq!(
            pinned("Claude", &Pinned::Full).detail,
            None,
            "the design shows no second line"
        );
        assert_eq!(removed("Code", 3).text, "Unpinned Code (was number 4)");
        let empty = empty(4, "Win+`");
        assert_eq!(empty.text, "Number 5 is empty");
        assert_eq!(
            empty.detail.as_deref(),
            Some("Win+`, then A adds the window in front.")
        );
        assert_eq!(
            starting("Claude", 1).text,
            "Starting Claude for number 2\u{2026}"
        );
        assert!(super::empty(4, "").detail.is_some());
        for message in [
            not_open("Code", 0),
            not_started("Code"),
            no_keys(),
            no_window(),
            undone("Added Claude as number 3"),
            changed("Code", 0),
        ] {
            assert!(!message.text.is_empty());
        }
    }
}
