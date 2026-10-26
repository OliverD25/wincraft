//! Keys WinCraft sends itself.
//!
//! The mask key: when a keyboard hook hides a key while Alt or Win is held,
//! Windows still sees the modifier go down and up with nothing in between.
//! A lone Win tap opens the Start menu, and a lone Alt tap opens the menu of
//! the window in front. So the hook sends an unassigned key (`VK`) in place
//! of the hidden one, the same trick AutoHotkey and PowerToys use.
//!
//! Every key sent here carries `TAG`, so a hook of WinCraft's own can tell
//! it from a key the user pressed and let it through.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_LWIN, VK_RWIN,
};

/// Unassigned by Windows, so no keyboard sends it and no app binds it.
/// AutoHotkey masks with the same key, which is why a hook always passes it:
/// a mask another tool sends must not count as a key the user pressed.
pub const VK: u32 = 0xE8;

/// The `dwExtraInfo` of every key sent here: "WCK1".
pub const TAG: usize = 0x5743_4B31;

/// Whether a key event came from `send` or `replay_win_digit`.
pub fn is_ours(extra_info: usize) -> bool {
    extra_info == TAG
}

fn input(vk: u32, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk as VIRTUAL_KEY,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: TAG,
            },
        },
    }
}

fn send_all(inputs: &[INPUT]) -> bool {
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    sent as usize == inputs.len()
}

/// Sends the mask key down and up. `who` names the caller for the log
/// line when Windows refuses it.
pub fn send(who: &str) {
    if !send_all(&[input(VK, false), input(VK, true)]) {
        log::warn!("{who}: the mask key could not be sent");
    }
}

/// The keys to send for Win+`vk` that a hook kept but Windows should have
/// after all: the digit alone while Win is still held, else Win around it.
pub fn win_digit_keys(vk: u32, win_held: bool) -> Vec<(u32, bool)> {
    let mut keys = Vec::new();
    if !win_held {
        keys.push((u32::from(VK_LWIN), false));
    }
    keys.push((vk, false));
    keys.push((vk, true));
    if !win_held {
        keys.push((u32::from(VK_LWIN), true));
    }
    keys
}

/// Sends Win+`vk` to Windows again, for a Win+number a hook kept and then
/// found it should not have.
pub fn replay_win_digit(vk: u32) {
    let down =
        |key: VIRTUAL_KEY| (unsafe { GetAsyncKeyState(i32::from(key)) } as u16 & 0x8000) != 0;
    let held = down(VK_LWIN) || down(VK_RWIN);
    let inputs: Vec<INPUT> = win_digit_keys(vk, held)
        .into_iter()
        .map(|(vk, up)| input(vk, up))
        .collect();
    if !send_all(&inputs) {
        log::warn!("Win+number could not be handed back to Windows");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_keys_with_the_tag_are_ours() {
        assert!(is_ours(TAG));
        assert!(!is_ours(0));
        assert!(!is_ours(TAG + 1));
    }

    #[test]
    fn a_replay_adds_win_only_when_it_is_no_longer_held() {
        let one = 0x31;
        assert_eq!(win_digit_keys(one, true), [(one, false), (one, true)]);
        let win = u32::from(VK_LWIN);
        assert_eq!(
            win_digit_keys(one, false),
            [(win, false), (one, false), (one, true), (win, true)]
        );
    }
}
