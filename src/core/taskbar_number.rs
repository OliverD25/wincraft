//! Win+Alt+A, then a digit: the Arrange strip for taskbar button N, the
//! button Win+N opens. Win+Alt+A only starts a wait, with a number over each
//! taskbar button; the digit may come with Win+Alt still held, when Windows
//! would take it for its own jump list, or after they were let go. A digit
//! in an open strip jumps the same way. Nothing here calls Windows.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

use crate::core::chord::{Stroke, Verdict};
use crate::core::taskbar::Button;
use crate::core::traits::Hotkey;

/// How soon a second digit must follow the first to make a two-digit
/// number, when there are more than ten buttons: 1 then 1 is button 11.
pub const SECOND_DIGIT_MS: u64 = 400;
/// How long Win+Alt+A waits for a digit.
pub const WAIT_MS: u64 = 3000;
/// How long the wait after Win+Alt+A goes without a digit before the
/// badges and the line show. Someone who knows the number has typed it by
/// then and never sees them.
pub const REVEAL_MS: u64 = 700;
const ESCAPE: u32 = 0x1B;

/// The digits typed so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Digits {
    first: Option<(u8, u64)>,
}

impl Digits {
    /// `digit` typed at `now` (milliseconds on any clock that only goes
    /// forward), with `count` buttons on the taskbar: the button number to
    /// open now. 1 to 9 are buttons 1 to 9 and 0 is button 10, as with
    /// Win+number. A first digit opens its button at once, never waiting; a
    /// second one soon after that makes a number up to `count` jumps on to
    /// that button.
    pub fn press(&mut self, digit: u8, now: u64, count: usize) -> usize {
        if let Some((first, at)) = self.first.take() {
            let number = first as usize * 10 + digit as usize;
            if now.saturating_sub(at) <= SECOND_DIGIT_MS && number <= count {
                return number;
            }
        }
        if digit >= 1 && count > 10 && digit as usize * 10 <= count {
            self.first = Some((digit, now));
        }
        if digit == 0 {
            10
        } else {
            digit as usize
        }
    }

    /// Whether a second digit typed at `now` could still make a two-digit
    /// number.
    pub fn may_follow(&self, now: u64) -> bool {
        self.first
            .is_some_and(|(_, at)| now.saturating_sub(at) <= SECOND_DIGIT_MS)
    }
}

/// When the badges and the line show during the wait after Win+Alt+A:
/// only once the wait has gone `REVEAL_MS` without a digit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reveal {
    since: Option<u64>,
}

impl Reveal {
    /// A wait that started at `now` (milliseconds on any clock that only
    /// goes forward).
    pub fn start(now: u64) -> Self {
        Self { since: Some(now) }
    }

    /// A digit came or the wait ended: this wait shows nothing any more.
    pub fn cancel(&mut self) {
        self.since = None;
    }

    /// Whether the badges are still to come.
    pub fn pending(&self) -> bool {
        self.since.is_some()
    }

    /// How long until the badges are due; `None` when they never will be.
    pub fn left(&self, now: u64) -> Option<u64> {
        self.since
            .map(|since| (since + REVEAL_MS).saturating_sub(now))
    }

    /// True once, the first time it is asked at or after the badges are
    /// due; after that they are no longer pending.
    pub fn take_due(&mut self, now: u64) -> bool {
        if self.left(now) == Some(0) {
            self.since = None;
            return true;
        }
        false
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

/// A key pressed during the wait after Win+Alt+A.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitKey {
    /// A top-row digit, alone or with Win or Alt still held.
    Digit(u8),
    /// The hotkey once more: the strip for the app in front.
    Again,
    Cancel,
    Other,
}

/// Which kind of key `stroke` is, while `hotkey` (Win+Alt+A, or what the
/// user set) waits.
pub fn wait_key(stroke: Stroke, hotkey: Hotkey) -> WaitKey {
    let held = MOD_WIN | MOD_ALT | MOD_CONTROL | MOD_SHIFT;
    if stroke.vk == hotkey.vk && stroke.modifiers == hotkey.modifiers & held {
        return WaitKey::Again;
    }
    if stroke.vk == ESCAPE {
        return WaitKey::Cancel;
    }
    match digit_of(stroke.vk) {
        Some(digit) if stroke.modifiers & !(MOD_WIN | MOD_ALT) == 0 => WaitKey::Digit(digit),
        _ => WaitKey::Other,
    }
}

/// What the keyboard hook does with each kind: a digit and Esc are kept, so
/// neither Windows' jump list nor the app in front sees them. The hotkey
/// goes on to Windows, which hands it back to WinCraft as the hotkey. Any
/// other key goes through and ends the wait.
pub fn wait_verdict(key: WaitKey) -> Verdict {
    match key {
        WaitKey::Digit(_) | WaitKey::Cancel => Verdict::Take,
        WaitKey::Again => Verdict::Pass,
        WaitKey::Other => Verdict::End,
    }
}

/// The number on taskbar button `number`'s badge: the digit that opens it,
/// 0 for button 10, and its two digits after that.
pub fn badge_text(number: usize) -> String {
    match number {
        10 => "0".to_string(),
        _ => number.to_string(),
    }
}

/// The line at the bottom of the screen while Win+Alt+A waits. `keys` is the
/// hotkey as the user set it.
pub fn wait_line(keys: &str) -> String {
    format!("WinCraft: press a taskbar number, or {keys} again for this app")
}

/// The line the strip shows for a button whose app it cannot show.
pub fn no_windows(number: usize) -> String {
    format!("Button {number} has no windows WinCraft can show.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digit_is_its_button_and_0_is_button_10() {
        let mut digits = Digits::default();
        assert_eq!(digits.press(1, 0, 6), 1);
        assert_eq!(digits.press(9, 0, 6), 9);
        assert_eq!(digits.press(0, 0, 6), 10);
        // Ten buttons: no two-digit numbers, so nothing can follow.
        assert_eq!(digits.press(1, 0, 10), 1);
        assert!(!digits.may_follow(0));
    }

    #[test]
    fn a_first_digit_opens_at_once_and_a_quick_second_one_jumps_on() {
        let mut digits = Digits::default();
        // 28 buttons: 1 opens button 1 at once, and 1 then 2 jumps to 12.
        assert_eq!(digits.press(1, 1000, 28), 1);
        assert!(digits.may_follow(1300));
        assert_eq!(digits.press(2, 1300, 28), 12);
        assert!(!digits.may_follow(1300));
        // 2 then 9 is 29, more than 28: the 9 is button 9.
        assert_eq!(digits.press(2, 2000, 28), 2);
        assert_eq!(digits.press(9, 2100, 28), 9);
        // A digit that cannot start a number up to 28 opens alone.
        assert_eq!(digits.press(3, 3000, 28), 3);
        assert!(!digits.may_follow(3000));
        assert_eq!(digits.press(0, 3000, 28), 10);
        // A second digit that comes too late is a button of its own.
        assert_eq!(digits.press(1, 5000, 28), 1);
        assert!(!digits.may_follow(5401));
        assert_eq!(digits.press(3, 5401, 28), 3);
    }

    #[test]
    fn the_badges_show_only_after_a_pause_without_a_digit() {
        let mut reveal = Reveal::start(1000);
        assert!(reveal.pending());
        assert!(!reveal.take_due(1000));
        assert!(!reveal.take_due(1699), "no badges before 700 ms");
        assert_eq!(reveal.left(1500), Some(200));
        assert!(reveal.take_due(1700), "badges at 700 ms");
        assert!(!reveal.pending());
        assert!(!reveal.take_due(1800), "once only");

        let mut quick = Reveal::start(1000);
        quick.cancel();
        assert!(!quick.pending(), "a digit before 700 ms");
        assert!(!quick.take_due(5000), "never shows them");
        assert_eq!(quick.left(5000), None);
        assert!(!Reveal::default().pending());
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

    const WIN_ALT_A: Hotkey = Hotkey {
        modifiers: MOD_WIN | MOD_ALT | 0x4000,
        vk: 0x41,
    };

    #[test]
    fn the_wait_takes_digits_and_esc_and_ends_on_any_other_key() {
        let key = |vk, modifiers| wait_key(Stroke { vk, modifiers }, WIN_ALT_A);
        let win_alt = MOD_WIN | MOD_ALT;
        // A digit with Win+Alt still held, or after they were let go.
        assert_eq!(key(0x33, win_alt), WaitKey::Digit(3));
        assert_eq!(key(0x30, 0), WaitKey::Digit(0));
        assert_eq!(key(0x35, MOD_ALT), WaitKey::Digit(5));
        assert_eq!(key(0x41, win_alt), WaitKey::Again);
        assert_eq!(key(0x1B, 0), WaitKey::Cancel);
        assert_eq!(key(0x1B, win_alt), WaitKey::Cancel);
        // A plain A, a digit with Ctrl or Shift, a letter: other keys.
        assert_eq!(key(0x41, 0), WaitKey::Other);
        assert_eq!(key(0x33, MOD_CONTROL), WaitKey::Other);
        assert_eq!(key(0x33, win_alt | MOD_SHIFT), WaitKey::Other);
        assert_eq!(key(0x45, win_alt), WaitKey::Other);

        assert_eq!(wait_verdict(WaitKey::Digit(3)), Verdict::Take);
        assert_eq!(wait_verdict(WaitKey::Cancel), Verdict::Take);
        assert_eq!(wait_verdict(WaitKey::Again), Verdict::Pass);
        assert_eq!(wait_verdict(WaitKey::Other), Verdict::End);
    }

    #[test]
    fn the_wait_follows_the_hotkey_the_user_set() {
        let ctrl_alt_j = Hotkey {
            modifiers: MOD_CONTROL | MOD_ALT,
            vk: 0x4A,
        };
        let stroke = Stroke {
            vk: 0x4A,
            modifiers: MOD_CONTROL | MOD_ALT,
        };
        assert_eq!(wait_key(stroke, ctrl_alt_j), WaitKey::Again);
        assert_eq!(wait_key(stroke, WIN_ALT_A), WaitKey::Other);
        assert_eq!(
            wait_line("Ctrl+Alt+J"),
            "WinCraft: press a taskbar number, or Ctrl+Alt+J again for this app"
        );
    }

    #[test]
    fn badges_show_the_digit_that_opens_each_button() {
        let texts: Vec<String> = (1..=12).map(badge_text).collect();
        assert_eq!(
            texts,
            ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "11", "12"]
        );
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
