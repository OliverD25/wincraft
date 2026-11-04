//! What each key does while QuickJump waits after its shortcut (Win+`):
//! a digit jumps, Alt and a digit puts the window in front at that number,
//! ` opens Quick access, A adds and U unpins the window in front, Esc
//! cancels. Any other key is kept from the app in front too, and the tip
//! says nothing is assigned to it. The shortcut itself once more goes on
//! to Windows, which hands it back as the hotkey: the "twice".

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

use crate::core::chord::{Stroke, Verdict};
use crate::core::shortcuts;
use crate::core::taskbar_number::digit_of;
use crate::core::traits::Hotkey;

const ESCAPE: u32 = 0x1B;
const NUMPAD_0: u32 = 0x60;
const Z: u32 = 0x5A;
const A: u32 = 0x41;
const U: u32 = 0x55;
/// The key left of 1 on a US keyboard: ` and ~.
pub const BACKTICK: u32 = 0xC0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A digit on the top row or the number pad, alone or with Win or
    /// Shift still held from the shortcut: jump to that number.
    Digit(u8),
    /// Alt and a digit: put the window in front at that number.
    AltDigit(u8),
    /// ` alone: open Quick access.
    Access,
    /// One of QuickJump's own shortcuts once more.
    Again,
    /// A: add the window in front to the end.
    Add,
    /// U: unpin the window in front.
    Unpin,
    Cancel,
    /// Nothing is assigned to it; it is kept from the app all the same.
    Other,
}

fn digit(vk: u32) -> Option<u8> {
    digit_of(vk).or_else(|| {
        (NUMPAD_0..=NUMPAD_0 + 9)
            .contains(&vk)
            .then(|| (vk - NUMPAD_0) as u8)
    })
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
    let ctrl = stroke.modifiers & MOD_CONTROL != 0;
    let alt = stroke.modifiers & MOD_ALT != 0;
    if ctrl {
        return Key::Other;
    }
    if let Some(digit) = digit(stroke.vk) {
        return if alt {
            Key::AltDigit(digit)
        } else {
            Key::Digit(digit)
        };
    }
    if alt {
        return Key::Other;
    }
    match stroke.vk {
        BACKTICK => Key::Access,
        A => Key::Add,
        U => Key::Unpin,
        _ => Key::Other,
    }
}

pub fn verdict(key: Key) -> Verdict {
    match key {
        Key::Again => Verdict::Pass,
        _ => Verdict::Take,
    }
}

/// The hook's rule while QuickJump waits for the next key. The first
/// presses of the jump and pin shortcuts pass. It runs on the hook's own
/// thread, so it owns them.
pub fn rule(jump: &str, pin: &str) -> impl Fn(Stroke) -> Verdict + Send {
    let again = firsts(jump, pin);
    move |stroke| verdict(kind(stroke, &again))
}

/// The first presses of the jump and pin shortcuts.
pub fn firsts(jump: &str, pin: &str) -> Vec<Hotkey> {
    [jump, pin]
        .iter()
        .filter_map(|text| shortcuts::parse(text))
        .map(|keys| keys.first)
        .collect()
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
    fn digits_jump_with_the_shortcuts_keys_held_or_let_go() {
        assert_eq!(kind(stroke(0x33, 0), &[]), Key::Digit(3));
        assert_eq!(kind(stroke(0x30, MOD_WIN), &[]), Key::Digit(0));
        assert_eq!(kind(stroke(0x35, MOD_WIN | MOD_SHIFT), &[]), Key::Digit(5));
        assert_eq!(kind(stroke(0x67, 0), &[]), Key::Digit(7), "number pad");
        assert_eq!(kind(stroke(0x33, MOD_CONTROL), &[]), Key::Other);
    }

    #[test]
    fn alt_and_a_digit_puts_the_window_at_that_number() {
        assert_eq!(kind(stroke(0x31, MOD_ALT), &[]), Key::AltDigit(1));
        assert_eq!(kind(stroke(0x30, MOD_WIN | MOD_ALT), &[]), Key::AltDigit(0));
        assert_eq!(kind(stroke(0x62, MOD_ALT), &[]), Key::AltDigit(2));
        assert_eq!(kind(stroke(0x31, MOD_ALT | MOD_CONTROL), &[]), Key::Other);
    }

    #[test]
    fn the_letters_and_backtick_have_their_jobs_and_other_keys_none() {
        assert_eq!(kind(stroke(A, 0), &[]), Key::Add);
        assert_eq!(kind(stroke(A, MOD_WIN), &[]), Key::Add, "Win still held");
        assert_eq!(kind(stroke(U, 0), &[]), Key::Unpin);
        assert_eq!(kind(stroke(BACKTICK, 0), &[]), Key::Access);
        assert_eq!(kind(stroke(A, MOD_ALT), &[]), Key::Other);
        assert_eq!(kind(stroke(U, MOD_CONTROL), &[]), Key::Other);
        assert_eq!(kind(stroke(0x58, 0), &[]), Key::Other, "X");
        assert_eq!(kind(stroke(ESCAPE, 0), &[]), Key::Cancel);
        assert_eq!(kind(stroke(ESCAPE, MOD_WIN), &[]), Key::Cancel);
    }

    #[test]
    fn the_shortcut_again_goes_on_to_windows_and_every_other_key_is_kept() {
        let rule = rule("Win+`", "Win+Alt+Shift+Q");
        let twice = stroke(BACKTICK, MOD_WIN);
        assert_eq!(rule(twice), Verdict::Pass);
        assert_eq!(
            rule(stroke(0x51, MOD_WIN | MOD_ALT | MOD_SHIFT)),
            Verdict::Pass
        );
        assert_eq!(rule(stroke(BACKTICK, 0)), Verdict::Take, "` alone");
        assert_eq!(rule(stroke(0x51, 0)), Verdict::Take, "Q, nothing assigned");
        assert_eq!(rule(stroke(0x32, MOD_WIN)), Verdict::Take);
        assert_eq!(rule(stroke(0x32, MOD_ALT)), Verdict::Take);
        assert_eq!(rule(stroke(ESCAPE, 0)), Verdict::Take);
        let none = super::rule("", "");
        assert_eq!(none(twice), Verdict::Take);
        assert_eq!(firsts("Win+`", "").len(), 1);
    }

    #[test]
    fn only_ctrl_z_is_kept_for_undo() {
        assert_eq!(undo_rule(stroke(Z, MOD_CONTROL)), Verdict::Take);
        assert_eq!(undo_rule(stroke(Z, 0)), Verdict::Pass);
        assert_eq!(undo_rule(stroke(Z, MOD_CONTROL | MOD_SHIFT)), Verdict::Pass);
        assert_eq!(undo_rule(stroke(0x41, 0)), Verdict::Pass);
    }
}
