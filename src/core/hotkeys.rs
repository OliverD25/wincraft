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
    let mut out = format_modifiers(hotkey.modifiers);
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(&format_key(hotkey.vk));
    out
}

pub fn format_modifiers(modifiers: u32) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(4);
    for (flag, name) in [
        (MOD_WIN, "Win"),
        (MOD_CONTROL, "Ctrl"),
        (MOD_ALT, "Alt"),
        (MOD_SHIFT, "Shift"),
    ] {
        if modifiers & flag != 0 {
            parts.push(name);
        }
    }
    parts.join("+")
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

/// Every key that is not a plain letter, digit or function key. This is the
/// single source for both the parser and the ShortcutDetector scan, so a key
/// added here becomes usable in config.json and probed at the same time.
pub const NAMED_KEYS: &[(&str, u32)] = &[
    ("Space", 0x20),
    ("Esc", 0x1B),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Backspace", 0x08),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("PrintScreen", 0x2C),
    ("Pause", 0x13),
    ("ScrollLock", 0x91),
    ("NumLock", 0x90),
    ("Numpad0", 0x60),
    ("Numpad1", 0x61),
    ("Numpad2", 0x62),
    ("Numpad3", 0x63),
    ("Numpad4", 0x64),
    ("Numpad5", 0x65),
    ("Numpad6", 0x66),
    ("Numpad7", 0x67),
    ("Numpad8", 0x68),
    ("Numpad9", 0x69),
    ("NumpadMultiply", 0x6A),
    ("NumpadPlus", 0x6B),
    ("NumpadMinus", 0x6D),
    ("NumpadDecimal", 0x6E),
    ("NumpadDivide", 0x6F),
    (";", 0xBA),
    ("=", 0xBB),
    (",", 0xBC),
    ("-", 0xBD),
    (".", 0xBE),
    ("/", 0xBF),
    ("`", 0xC0),
    ("[", 0xDB),
    ("\\", 0xDC),
    ("]", 0xDD),
    ("'", 0xDE),
];

/// Every key the parser understands, in the order the detector shows them.
pub fn all_keys() -> Vec<u32> {
    let mut keys: Vec<u32> = Vec::with_capacity(26 + 10 + 24 + NAMED_KEYS.len());
    keys.extend((b'A'..=b'Z').map(u32::from));
    keys.extend((b'0'..=b'9').map(u32::from));
    keys.extend((0..24).map(|n| 0x70 + n));
    keys.extend(NAMED_KEYS.iter().map(|(_, vk)| *vk));
    keys
}

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
        assert_eq!(
            parse("win+alt+f1").unwrap(),
            parse(" WIN + Alt + F1 ").unwrap()
        );
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
    fn every_key_in_the_table_round_trips() {
        for vk in all_keys() {
            let hotkey = Hotkey {
                modifiers: MOD_NOREPEAT | MOD_CONTROL,
                vk,
            };
            let text = format(hotkey);
            assert!(
                !text.starts_with("Ctrl+0x"),
                "vk {vk:#04X} has no name, format gave {text}"
            );
            assert_eq!(parse(&text).unwrap(), hotkey, "round trip of {text}");
        }
    }

    #[test]
    fn key_names_are_unique() {
        for (index, (name, vk)) in NAMED_KEYS.iter().enumerate() {
            for (other_name, other_vk) in &NAMED_KEYS[index + 1..] {
                assert!(
                    !name.eq_ignore_ascii_case(other_name),
                    "duplicate key name {name}"
                );
                assert_ne!(vk, other_vk, "{name} and {other_name} share a code");
            }
        }
    }

    #[test]
    fn knows_the_keys_the_detector_probes() {
        for text in [
            "Ctrl+Backspace",
            "Ctrl+PrintScreen",
            "Ctrl+ScrollLock",
            "Ctrl+NumLock",
            "Ctrl+Numpad7",
            "Ctrl+NumpadPlus",
            "Ctrl+NumpadDivide",
            "Ctrl+;",
            "Ctrl+=",
            "Ctrl+,",
            "Ctrl+-",
            "Ctrl+.",
            "Ctrl+/",
            "Ctrl+`",
            "Ctrl+[",
            "Ctrl+\\",
            "Ctrl+]",
            "Ctrl+'",
        ] {
            let hotkey = parse(text).expect(text);
            assert_eq!(format(hotkey), text);
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
