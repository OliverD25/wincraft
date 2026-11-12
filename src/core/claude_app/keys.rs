//! The few keys WinCraft sends to the Claude app while its home window is
//! in front: the key that opens a chat's menu, Esc, Enter, and text.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY,
};

pub const SHIFT: u16 = 0x10;
pub const CONTROL: u16 = 0x11;
pub const F10: u16 = 0x79;
pub const ESCAPE: u16 = 0x1B;
pub const RETURN: u16 = 0x0D;
pub const KEY_A: u16 = 0x41;
pub const KEY_R: u16 = 0x52;

fn key(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk as VIRTUAL_KEY,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> bool {
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    sent as usize == inputs.len()
}

/// The inputs of a chord: the keys down in order, then up in reverse.
pub fn chord_inputs(keys: &[u16]) -> Vec<INPUT> {
    let down = keys.iter().map(|vk| key(*vk, 0, 0));
    let up = keys.iter().rev().map(|vk| key(*vk, 0, KEYEVENTF_KEYUP));
    down.chain(up).collect()
}

/// Presses the keys together, as `Shift+F10`.
pub fn chord(keys: &[u16]) -> bool {
    send(&chord_inputs(keys))
}

/// Types the text, one character at a time.
pub fn type_text(text: &str) -> bool {
    let inputs: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|unit| {
            [
                key(0, unit, KEYEVENTF_UNICODE),
                key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();
    inputs.is_empty() || send(&inputs)
}

/// The letter the app shows beside a menu item, for the items WinCraft
/// uses. Never Delete: nothing here may press its letter.
pub fn menu_letter(item: &str) -> Option<u16> {
    match item {
        "Rename" => Some(KEY_R),
        _ => None,
    }
}
