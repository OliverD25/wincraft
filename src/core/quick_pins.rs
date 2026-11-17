//! The link between QuickJump and ClaudeDeck, so neither plugin calls the
//! other. QuickJump writes what each Quick access number holds here every
//! time its pins change; ClaudeDeck reads it to show a chat's number and
//! asks QuickJump through here to give a chat a number or take it away.
//! QuickJump applies the asks on its own thread. While QuickJump is off,
//! nothing is written and every ask is dropped.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// The most numbers Quick access has: one per digit.
pub const SLOTS: usize = 10;

/// Posted to QuickJump's window when an ask waits.
pub const WM_APP_QUICK_PINS: u32 = WM_APP + 44;

/// What a number in the menu says when no pin holds it.
pub const FREE: &str = "free";

/// What one Quick access number holds, in number order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    /// What the pin is called.
    pub name: String,
    /// The Claude chat (its `local_` sessionId) when the pin is a chat.
    pub session: Option<String>,
}

/// What ClaudeDeck asks QuickJump to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Give the chat a number: `at` (a place counted from 0), or the first
    /// free number when None. `window` is the chat's open window.
    Put {
        session: String,
        name: String,
        window: Option<isize>,
        at: Option<usize>,
    },
    /// Take the chat's number away.
    Remove { session: String },
}

/// None while QuickJump is off.
static HELD: Mutex<Option<Vec<Held>>> = Mutex::new(None);
static ASKS: Mutex<Vec<Ask>> = Mutex::new(Vec::new());
/// QuickJump's jump shortcut as the user set it; empty for none.
static KEYS: Mutex<String> = Mutex::new(String::new());
/// QuickJump's own window; 0 while it is off.
static OWNER: AtomicIsize = AtomicIsize::new(0);

/// The digit that reaches a place: 1 to 9, and 0 for the tenth, as with
/// Win+number.
pub fn digit_of_slot(slot: usize) -> u8 {
    if slot == 9 {
        0
    } else {
        slot as u8 + 1
    }
}

/// QuickJump is on: asks go to `owner`, and this is what it holds.
pub fn open(owner: isize, held: Vec<Held>) {
    OWNER.store(owner, Ordering::Release);
    publish(held, "");
}

/// QuickJump's pins or its jump shortcut changed.
pub fn publish(held: Vec<Held>, jump_keys: &str) {
    if OWNER.load(Ordering::Acquire) == 0 {
        return;
    }
    if let Ok(mut latest) = HELD.lock() {
        *latest = Some(held);
    }
    if let Ok(mut keys) = KEYS.lock() {
        *keys = jump_keys.to_string();
    }
}

/// The tip of a chat's number box: "Win+` then 3 jumps here".
pub fn tip(slot: usize) -> String {
    let keys = KEYS.lock().map(|keys| keys.clone()).unwrap_or_default();
    tip_text(&keys, slot)
}

pub fn tip_text(jump_keys: &str, slot: usize) -> String {
    let digit = digit_of_slot(slot);
    if jump_keys.trim().is_empty() {
        format!("Quick access number {digit}")
    } else {
        format!("{} then {digit} jumps here", jump_keys.trim())
    }
}

/// QuickJump is off: no numbers, and asks still waiting are dropped.
pub fn close() {
    OWNER.store(0, Ordering::Release);
    if let Ok(mut latest) = HELD.lock() {
        *latest = None;
    }
    take_asks();
}

/// What each number holds; None while QuickJump is off.
pub fn held() -> Option<Vec<Held>> {
    HELD.lock().ok().and_then(|latest| latest.clone())
}

/// The chat's place: None while QuickJump is off, Some(None) when the
/// chat has no number.
pub fn number_of(session: &str) -> Option<Option<usize>> {
    let latest = HELD.lock().ok()?;
    latest.as_deref().map(|held| slot_of(held, session))
}

/// The place of the chat in `held`.
pub fn slot_of(held: &[Held], session: &str) -> Option<usize> {
    held.iter()
        .position(|held| held.session.as_deref() == Some(session))
}

/// The first free place: numbers have no gaps, so it is the one after the
/// last pin. None when all ten are taken.
pub fn first_free(held: &[Held]) -> Option<usize> {
    (held.len() < SLOTS).then_some(held.len())
}

/// A line of the number menu: "3 · WinCraft Rust Master" or "4 · free".
pub fn menu_line(slot: usize, held: &[Held]) -> String {
    let what = held.get(slot).map_or(FREE, |held| held.name.as_str());
    format!("{} \u{b7} {what}", digit_of_slot(slot))
}

/// Whether the number menu lets the user pick `slot`. A chat with a
/// number (`own`) can move to another taken number. A chat without one
/// can take a taken number (that pin moves down one) or the first free
/// one; a free number after it would land on the first free one, so it is
/// not offered, and a full list takes no new chat.
pub fn can_pick(slot: usize, count: usize, own: Option<usize>) -> bool {
    match own {
        Some(own) => slot < count && slot != own,
        None => slot <= count && count < SLOTS,
    }
}

/// Hands an ask to QuickJump; dropped while QuickJump is off.
pub fn ask(ask: Ask) {
    let owner = OWNER.load(Ordering::Acquire);
    if owner == 0 {
        return;
    }
    if let Ok(mut asks) = ASKS.lock() {
        asks.push(ask);
    }
    unsafe { PostMessageW(owner as HWND, WM_APP_QUICK_PINS, 0, 0) };
}

pub fn take_asks() -> Vec<Ask> {
    ASKS.lock()
        .map(|mut asks| std::mem::take(&mut *asks))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(names: &[(&str, Option<&str>)]) -> Vec<Held> {
        names
            .iter()
            .map(|(name, session)| Held {
                name: name.to_string(),
                session: session.map(str::to_string),
            })
            .collect()
    }

    #[test]
    fn the_menu_line_names_what_holds_a_number_or_says_it_is_free() {
        let list = held(&[("Notepad", None), ("WinCraft Rust Master", Some("local_a"))]);
        assert_eq!(menu_line(1, &list), "2 \u{b7} WinCraft Rust Master");
        assert_eq!(menu_line(2, &list), "3 \u{b7} free");
        assert_eq!(menu_line(9, &list), "0 \u{b7} free");
    }

    #[test]
    fn only_a_chat_pin_gives_a_chat_its_number() {
        let list = held(&[("Notepad", None), ("Chat", Some("local_a"))]);
        assert_eq!(slot_of(&list, "local_a"), Some(1));
        assert_eq!(slot_of(&list, "local_b"), None);
        assert_eq!(slot_of(&list, ""), None);
    }

    #[test]
    fn the_first_free_number_follows_the_last_pin_and_a_full_list_has_none() {
        assert_eq!(first_free(&[]), Some(0));
        assert_eq!(first_free(&held(&[("A", None), ("B", None)])), Some(2));
        let full: Vec<Held> = (0..SLOTS).map(|_| Held::default()).collect();
        assert_eq!(first_free(&full), None);
    }

    #[test]
    fn the_menu_offers_taken_numbers_and_only_the_first_free_one() {
        // Three pins, the chat has none: 1-3 and 4 (free), not 5.
        assert!(can_pick(0, 3, None));
        assert!(can_pick(3, 3, None));
        assert!(!can_pick(4, 3, None));
        // A full list takes no new chat.
        assert!(!can_pick(0, SLOTS, None));
        // The chat is number 2: it can move to 1 or 3, not to 2 or a free one.
        assert!(can_pick(0, 3, Some(1)));
        assert!(!can_pick(1, 3, Some(1)));
        assert!(!can_pick(3, 3, Some(1)));
    }

    #[test]
    fn the_tip_names_the_jump_keys_and_the_digit() {
        assert_eq!(tip_text("Win+`", 2), "Win+` then 3 jumps here");
        assert_eq!(tip_text("", 9), "Quick access number 0");
    }

    #[test]
    fn the_tenth_place_is_digit_zero() {
        assert_eq!(digit_of_slot(0), 1);
        assert_eq!(digit_of_slot(8), 9);
        assert_eq!(digit_of_slot(9), 0);
    }
}
