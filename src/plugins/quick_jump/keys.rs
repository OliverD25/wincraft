//! What the keyboard hook does with each key while QuickJump waits: a
//! digit and Esc are kept from the app in front, the jump or pin shortcut
//! goes on to Windows (which hands it back as the hotkey, the "twice"),
//! and any other key goes through and ends the wait.

use std::cell::RefCell;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

use crate::core::chord::{Stroke, Verdict};
use crate::core::shortcuts;
use crate::core::taskbar_number::digit_of;
use crate::core::traits::Hotkey;

const ESCAPE: u32 = 0x1B;
const NUMPAD_0: u32 = 0x60;
const Z: u32 = 0x5A;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A digit on the top row or the number pad, alone or with Win, Alt or
    /// Shift still held from the shortcut.
    Digit(u8),
    /// One of QuickJump's own shortcuts once more.
    Again,
    Cancel,
    Other,
}

pub fn kind(stroke: Stroke, again: &[Hotkey]) -> Key {
    let held = MOD_WIN | MOD_ALT | MOD_CONTROL | MOD_SHIFT;
    if again
        .iter()
        .any(|hotkey| stroke.vk == hotkey.vk && stroke.modifiers == hotkey.modifiers & held)
    {
        return Key::Again;
    }
    if stroke.vk == ESCAPE {
        return Key::Cancel;
    }
    let digit = digit_of(stroke.vk).or_else(|| {
        (NUMPAD_0..=NUMPAD_0 + 9)
            .contains(&stroke.vk)
            .then(|| (stroke.vk - NUMPAD_0) as u8)
    });
    match digit {
        Some(digit) if stroke.modifiers & MOD_CONTROL == 0 => Key::Digit(digit),
        _ => Key::Other,
    }
}

pub fn verdict(key: Key) -> Verdict {
    match key {
        Key::Digit(_) | Key::Cancel => Verdict::Take,
        Key::Again => Verdict::Pass,
        Key::Other => Verdict::End,
    }
}

thread_local! {
    /// The first press of the jump and pin shortcuts, which pass while the
    /// strip waits.
    static AGAIN: RefCell<Vec<Hotkey>> = const { RefCell::new(Vec::new()) };
}

/// Keeps the shortcuts' first presses for the hook's rule.
pub fn set_again(jump: &str, pin: &str) {
    let presses: Vec<Hotkey> = [jump, pin]
        .iter()
        .filter_map(|text| shortcuts::parse(text))
        .map(|keys| keys.first)
        .collect();
    AGAIN.with(|cell| *cell.borrow_mut() = presses);
}

/// The hook's rule while the strip waits for a digit.
pub fn rule(stroke: Stroke) -> Verdict {
    AGAIN.with(|cell| verdict(kind(stroke, &cell.borrow())))
}

/// The hook's rule while a notice with Undo shows: Ctrl+Z is kept for
/// Undo, every other key goes on as usual.
pub fn undo_rule(stroke: Stroke) -> Verdict {
    if stroke.vk == Z && stroke.modifiers == MOD_CONTROL {
        Verdict::Take
    } else {
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(vk: u32, modifiers: u32) -> Stroke {
        Stroke { vk, modifiers }
    }

    #[test]
    fn digits_count_with_the_shortcuts_keys_still_held() {
        assert_eq!(kind(stroke(0x33, 0), &[]), Key::Digit(3));
        assert_eq!(kind(stroke(0x30, MOD_WIN | MOD_ALT), &[]), Key::Digit(0));
        assert_eq!(
            kind(stroke(0x35, MOD_WIN | MOD_ALT | MOD_SHIFT), &[]),
            Key::Digit(5)
        );
        assert_eq!(kind(stroke(0x67, 0), &[]), Key::Digit(7), "number pad");
        assert_eq!(kind(stroke(0x33, MOD_CONTROL), &[]), Key::Other);
        assert_eq!(kind(stroke(ESCAPE, 0), &[]), Key::Cancel);
        assert_eq!(kind(stroke(0x41, 0), &[]), Key::Other);
    }

    #[test]
    fn the_shortcut_again_goes_on_to_windows_and_other_keys_end_the_wait() {
        set_again("Win+Alt+Q", "Win+Alt+Shift+Q");
        let q = stroke(0x51, MOD_WIN | MOD_ALT);
        assert_eq!(rule(q), Verdict::Pass);
        assert_eq!(
            rule(stroke(0x51, MOD_WIN | MOD_ALT | MOD_SHIFT)),
            Verdict::Pass
        );
        assert_eq!(rule(stroke(0x51, 0)), Verdict::End);
        assert_eq!(rule(stroke(0x32, MOD_WIN | MOD_ALT)), Verdict::Take);
        assert_eq!(rule(stroke(ESCAPE, 0)), Verdict::Take);
        set_again("", "");
        assert_eq!(rule(q), Verdict::End);
    }

    #[test]
    fn only_ctrl_z_is_kept_for_undo() {
        assert_eq!(undo_rule(stroke(Z, MOD_CONTROL)), Verdict::Take);
        assert_eq!(undo_rule(stroke(Z, 0)), Verdict::Pass);
        assert_eq!(undo_rule(stroke(Z, MOD_CONTROL | MOD_SHIFT)), Verdict::Pass);
        assert_eq!(undo_rule(stroke(0x41, 0)), Verdict::Pass);
    }
}
