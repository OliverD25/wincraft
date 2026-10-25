//! Win+Alt+A, then a digit: the Arrange strip for taskbar button N, the
//! button Win+N opens. The digit may come in the strip, after the keys were
//! let go, or while Win+Alt are still held, when Windows would take it for
//! its own jump list. Nothing here calls Windows.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_WIN};

use crate::core::chord::{Stroke, Verdict};
use crate::core::taskbar::Button;

/// How long a first digit waits for a second one, when there are more than
/// ten buttons: 1 then 1 is button 11.
pub const SECOND_DIGIT_MS: u64 = 400;
/// How long after Win+Alt+A a digit typed with Win+Alt still held belongs
/// to WinCraft instead of Windows' jump list.
pub const HELD_DIGIT_MS: u64 = 1500;

/// The digits typed so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Digits {
    first: Option<(u8, u64)>,
}

impl Digits {
    /// `digit` typed at `now` (milliseconds on any clock that only goes
    /// forward), with `count` buttons on the taskbar: the button number to
    /// open now, or None while a second digit may still follow. 1 to 9 are
    /// buttons 1 to 9 and 0 is button 10, as with Win+number.
    pub fn press(&mut self, digit: u8, now: u64, count: usize) -> Option<usize> {
        if let Some((first, at)) = self.first.take() {
            if now.saturating_sub(at) <= SECOND_DIGIT_MS {
                return Some(first as usize * 10 + digit as usize);
            }
        }
        if digit >= 1 && count > 10 && digit as usize * 10 <= count {
            self.first = Some((digit, now));
            return None;
        }
        Some(if digit == 0 { 10 } else { digit as usize })
    }

    /// The first digit alone: the wait for a second one is over.
    pub fn finish(&mut self) -> Option<usize> {
        self.first.take().map(|(digit, _)| digit as usize)
    }

    pub fn waiting(&self) -> bool {
        self.first.is_some()
    }
}

/// Button `number` (from 1) and the window it stands for: its place among
/// the buttons of the same app, from 1. With "never combine" each window
/// has a button of its own, in the order of the app's row, so the place is
/// the number on the window's card; a combined button is the app's first
/// window.
pub fn button_number(buttons: &[Button], number: usize) -> Option<(&Button, usize)> {
    let button = buttons.get(number.checked_sub(1)?)?;
    let place = buttons[..number - 1]
        .iter()
        .filter(|other| other.app == button.app)
        .count()
        + 1;
    Some((button, place))
}

/// The digit of a top-row digit key.
pub fn digit_of(vk: u32) -> Option<u8> {
    (0x30..=0x39).contains(&vk).then(|| (vk - 0x30) as u8)
}

/// What the keyboard hook does with a key during the wait after Win+Alt+A:
/// Win+Alt+digit, and nothing else, is WinCraft's while the strip is
/// opening or open. `strip_open` is whether its window is on screen;
/// `seen` whether it has been since the wait started, so a strip that has
/// closed again lets the digit go to Windows.
pub fn held_digit(stroke: Stroke, strip_open: bool, seen: bool) -> Verdict {
    if strip_open || !seen {
        if stroke.modifiers == MOD_WIN | MOD_ALT && digit_of(stroke.vk).is_some() {
            Verdict::Take
        } else {
            Verdict::Pass
        }
    } else {
        Verdict::End
    }
}

/// The line the strip shows for a button whose app it cannot show.
pub fn no_windows(number: usize) -> String {
    format!("Button {number} has no windows WinCraft can show.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_CONTROL, MOD_SHIFT};

    #[test]
    fn a_digit_is_its_button_and_0_is_button_10() {
        let mut digits = Digits::default();
        assert_eq!(digits.press(1, 0, 6), Some(1));
        assert_eq!(digits.press(9, 0, 6), Some(9));
        assert_eq!(digits.press(0, 0, 6), Some(10));
        // Ten buttons: no two-digit numbers, so no waiting.
        assert_eq!(digits.press(1, 0, 10), Some(1));
        assert!(!digits.waiting());
    }

    #[test]
    fn with_more_than_ten_buttons_a_first_digit_waits_for_a_second() {
        let mut digits = Digits::default();
        assert_eq!(digits.press(1, 1000, 14), None);
        assert!(digits.waiting());
        assert_eq!(digits.press(1, 1300, 14), Some(11));
        assert!(!digits.waiting());
        // A digit that cannot start a number up to 14 acts at once.
        assert_eq!(digits.press(2, 2000, 14), Some(2));
        assert_eq!(digits.press(0, 2000, 14), Some(10));
        // No second digit within 400 ms: the first acts alone.
        assert_eq!(digits.press(1, 3000, 14), None);
        assert_eq!(digits.finish(), Some(1));
        assert_eq!(digits.finish(), None);
        assert!(!digits.waiting());
        // A second digit that comes too late starts afresh.
        assert_eq!(digits.press(1, 5000, 25), None);
        assert_eq!(digits.press(3, 5401, 25), Some(3));
    }

    fn button(app: &str, left: i32) -> Button {
        Button {
            app: app.to_string(),
            rect: [left, 2040, left + 48, 2088],
        }
    }

    #[test]
    fn a_button_number_names_its_app_and_the_window_place() {
        let buttons = [
            button("Explorer", 0),
            button("Chrome", 48),
            button("Chrome", 96),
            button("Code", 144),
            button("Chrome", 192),
        ];
        let (first, place) = button_number(&buttons, 1).expect("button 1");
        assert_eq!((first.app.as_str(), place), ("Explorer", 1));
        let (third, place) = button_number(&buttons, 3).expect("button 3");
        assert_eq!(
            (third.app.as_str(), third.rect[0], place),
            ("Chrome", 96, 2)
        );
        // The third Chrome window, after the Code button between.
        assert_eq!(button_number(&buttons, 5).map(|(_, place)| place), Some(3));
        assert!(button_number(&buttons, 0).is_none());
        assert!(button_number(&buttons, 6).is_none());
    }

    #[test]
    fn only_win_alt_digit_is_taken_and_only_while_the_strip_is_there() {
        let stroke = |vk, modifiers| Stroke { vk, modifiers };
        let win_alt = MOD_WIN | MOD_ALT;
        assert_eq!(held_digit(stroke(0x33, win_alt), true, true), Verdict::Take);
        assert_eq!(
            held_digit(stroke(0x30, win_alt), false, false),
            Verdict::Take
        );
        // Other keys and other modifiers go to Windows.
        assert_eq!(held_digit(stroke(0x41, win_alt), true, true), Verdict::Pass);
        assert_eq!(held_digit(stroke(0x33, MOD_WIN), true, true), Verdict::Pass);
        assert_eq!(
            held_digit(stroke(0x33, win_alt | MOD_SHIFT), true, true),
            Verdict::Pass
        );
        assert_eq!(
            held_digit(stroke(0x33, win_alt | MOD_CONTROL), true, true),
            Verdict::Pass
        );
        assert_eq!(held_digit(stroke(0x33, 0), true, true), Verdict::Pass);
        // The strip closed again: Windows' jump list is back.
        assert_eq!(held_digit(stroke(0x33, win_alt), false, true), Verdict::End);
    }

    #[test]
    fn digits_come_from_the_top_row_only() {
        assert_eq!(digit_of(0x30), Some(0));
        assert_eq!(digit_of(0x39), Some(9));
        assert_eq!(digit_of(0x60), None, "the numeric keypad");
        assert_eq!(digit_of(0x41), None);
    }

    #[test]
    fn the_note_names_the_button_and_wincraft() {
        assert_eq!(no_windows(4), "Button 4 has no windows WinCraft can show.");
    }
}
