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
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};

/// Unassigned by Windows, so no keyboard sends it and no app binds it.
/// AutoHotkey masks with the same key, which is why a hook always passes it:
/// a mask another tool sends must not count as a key the user pressed.
pub const VK: u32 = 0xE8;

/// The `dwExtraInfo` of every key sent here: "WCK1".
pub const TAG: usize = 0x5743_4B31;

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
