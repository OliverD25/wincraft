use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

use crate::core::traits::Hotkey;

pub fn parse(text: &str) -> Result<Hotkey, String> {
    let mut modifiers = MOD_NOREPEAT;
    let mut key: Option<u32> = None;

    for part in text.split('+') {
        let token = part.trim();
        if token.is_empty() {
            return Err(format!("empty part in hotkey \"{text}\""));
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "windows" => modifiers |= MOD_WIN,
            other => {
                if key.is_some() {
                    return Err(format!("hotkey \"{text}\" names more than one key"));
                }
                key = Some(parse_key(other).ok_or_else(|| format!("unknown key \"{token}\""))?);
            }
        }
    }

    match key {
        Some(vk) => Ok(Hotkey { modifiers, vk }),
        None => Err(format!("hotkey \"{text}\" has no key, only modifiers")),
    }
}

pub fn format(hotkey: Hotkey) -> String {
    let mut out = String::new();
    for (flag, name) in [
        (MOD_WIN, "Win"),
        (MOD_CONTROL, "Ctrl"),
        (MOD_ALT, "Alt"),
        (MOD_SHIFT, "Shift"),
    ] {
        if hotkey.modifiers & flag != 0 {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(&format_key(hotkey.vk));
    out
}

fn parse_key(token: &str) -> Option<u32> {
    if token.len() == 1 {
        let c = token.as_bytes()[0].to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    if let Some(digits) = token.strip_prefix('f') {
        if let Ok(n) = digits.parse::<u32>() {
            if (1..=24).contains(&n) {
                return Some(0x70 + n - 1);
            }
        }
    }
    NAMED_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token))
        .map(|(_, vk)| *vk)
}

fn format_key(vk: u32) -> String {
    if let Some((name, _)) = NAMED_KEYS.iter().find(|(_, code)| *code == vk) {
        return (*name).to_string();
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x70 + 1);
    }
    if (b'0' as u32..=b'9' as u32).contains(&vk) || (b'A' as u32..=b'Z' as u32).contains(&vk) {
        return (vk as u8 as char).to_string();
    }
    format!("0x{vk:02X}")
}

const NAMED_KEYS: &[(&str, u32)] = &[
    ("Space", 0x20),
    ("Esc", 0x1B),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shipped_defaults() {
        let hk = parse("Win+Alt+F1").unwrap();
        assert_eq!(hk.modifiers, MOD_NOREPEAT | MOD_WIN | MOD_ALT);
        assert_eq!(hk.vk, 0x70);
        assert_eq!(parse("Win+Alt+F12").unwrap().vk, 0x7B);
    }

    #[test]
    fn parsing_ignores_case_and_spaces() {
        assert_eq!(parse("win+alt+f1").unwrap(), parse(" WIN + Alt + F1 ").unwrap());
        assert_eq!(parse("Control+A").unwrap(), parse("Ctrl+a").unwrap());
    }

    #[test]
    fn round_trips_through_format_and_parse() {
        for text in [
            "Win+Alt+F1",
            "Win+Alt+F12",
            "Win+Ctrl+Alt+Shift+F24",
            "Ctrl+Shift+A",
            "Alt+9",
            "Win+Space",
            "Ctrl+PageDown",
            "Win+Alt+Left",
            "Shift+Delete",
        ] {
            let hk = parse(text).expect(text);
            assert_eq!(format(hk), text, "format of {text}");
            assert_eq!(parse(&format(hk)).unwrap(), hk);
        }
    }

    #[test]
    fn rejects_broken_strings() {
        assert!(parse("Win+Alt").is_err());
        assert!(parse("Win+Alt+F1+F2").is_err());
        assert!(parse("Win+Alt+Banana").is_err());
        assert!(parse("Win++F1").is_err());
        assert!(parse("F25").is_err());
    }
}
