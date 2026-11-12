//! Small rules for the keyboard and the tray count: which row the hotkey
//! selects, which rows get the digits 1 to 9, and what the tray tooltip
//! says.

use egui::Key;

use super::model::{Chat, Line, Mark};
use super::window::Pick;

/// The digits 1 to 9, in order.
pub const DIGITS: [Key; 9] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

/// Whether the chat waits for the user: it needs approval or asks.
pub fn waits(mark: Mark) -> bool {
    matches!(mark, Mark::NeedsApproval | Mark::AsksYou)
}

/// The row the hotkey selects: the shown chat that has waited longest
/// (approval first when two began at the same time), else the row used
/// last if it is still shown, else the first chat.
pub fn open_pick(lines: &[Line], last: Option<&Pick>) -> Option<Pick> {
    let chats = || {
        lines.iter().filter_map(|line| match line {
            Line::Chat(chat) if !chat.stand_in && !chat.hidden => Some(&chat.chat),
            _ => None,
        })
    };
    let longest = chats()
        .filter(|chat| waits(chat.mark))
        .min_by_key(|chat| (chat.since.unwrap_or(u64::MAX), chat.mark.rank()));
    if let Some(chat) = longest {
        return Some(Pick::Chat(chat.id.clone()));
    }
    let shown = |pick: &Pick| {
        lines.iter().any(|line| match (line, pick) {
            (Line::Chat(c), Pick::Chat(id)) => &c.chat.id == id,
            (Line::Group(g), Pick::Group(id)) => &g.key == id,
            _ => false,
        })
    };
    if let Some(last) = last.filter(|pick| shown(pick)) {
        return Some(last.clone());
    }
    chats().next().map(|chat| Pick::Chat(chat.id.clone()))
}

/// The digit a chat row shows: the first nine chat rows on screen get 1
/// to 9, top to bottom. `seen` counts the ones already numbered.
pub fn digit_for(on_screen: bool, stand_in: bool, seen: &mut usize) -> Option<u8> {
    if !on_screen || stand_in || *seen >= DIGITS.len() {
        return None;
    }
    *seen += 1;
    u8::try_from(*seen).ok()
}

/// How many listed chats wait for the user.
pub fn waiting(chats: &[Chat], only_open: bool) -> usize {
    chats
        .iter()
        .filter(|chat| !only_open || chat.window.is_some())
        .filter(|chat| waits(chat.mark))
        .count()
}

/// The ids of the chats that wait now, to see when a new one starts.
pub fn waiting_ids(chats: &[Chat]) -> Vec<String> {
    chats
        .iter()
        .filter(|chat| waits(chat.mark))
        .map(|chat| chat.id.clone())
        .collect()
}

/// Whether a chat waits now that did not wait before.
pub fn new_wait(before: &[String], now: &[String]) -> bool {
    now.iter().any(|id| !before.contains(id))
}

/// The tray tooltip for this many waiting chats; it always names
/// WinCraft. None leaves the plain tooltip.
pub fn tray_tip(on: bool, count: usize) -> Option<String> {
    match count {
        _ if !on => None,
        0 => None,
        1 => Some("WinCraft \u{2014} 1 chat waits for you".to_string()),
        n => Some(format!("WinCraft \u{2014} {n} chats wait for you")),
    }
}
