//! What an input language is called: the two-letter code, the language's name
//! in its own language, and the keyboard layout's name.
//!
//! An HKL packs two things. The low word is the language (a LANGID, 0x0422 for
//! Ukrainian). The high word is the keyboard layout: either another LANGID,
//! for a plain layout, or 0xF000 plus the layout's "Layout Id" from the
//! registry, for variants such as Ukrainian (Enhanced), whose HKL is
//! 0xF0A80422.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES, LOCALE_SENGLISHDISPLAYNAME,
    LOCALE_SISO639LANGNAME, LOCALE_SNATIVEDISPLAYNAME,
};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    RRF_RT_REG_SZ,
};

use crate::core::wide;

const LAYOUTS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts";

#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    /// "EN", "UK".
    pub code: String,
    /// "English (United States)", "українська (Україна)".
    pub name: String,
    /// "US", "Ukrainian (Enhanced)"; None when it cannot be found.
    pub layout: Option<String>,
}

/// How to find the layout's registry key (KLID) for an HKL.
#[derive(Clone, Debug, PartialEq)]
pub enum Klid {
    /// The KLID is known outright, such as "00000409".
    Direct(String),
    /// The KLID is the key whose "Layout Id" value is this number.
    LayoutId(u16),
}

/// The language part of an HKL. Only the low 32 bits carry meaning; on 64-bit
/// Windows the handle arrives sign-extended.
pub fn langid(hkl: usize) -> u16 {
    (hkl & 0xFFFF) as u16
}

pub fn klid(hkl: usize) -> Klid {
    let low = (hkl & 0xFFFF) as u16;
    let high = ((hkl >> 16) & 0xFFFF) as u16;
    match high & 0xF000 {
        // An input method editor: its KLID is the whole handle.
        0xE000 => Klid::Direct(format!("{:08X}", hkl & 0xFFFF_FFFF)),
        0xF000 => Klid::LayoutId(high & 0x0FFF),
        _ if high == 0 => Klid::Direct(format!("0000{low:04X}")),
        _ => Klid::Direct(format!("0000{high:04X}")),
    }
}

/// The code and names for an HKL. Anything Windows cannot answer is left
/// out rather than guessed: an unknown language shows its LANGID.
pub fn describe(hkl: usize) -> Language {
    let id = langid(hkl);
    let locale = locale_name(id);
    let code = locale
        .as_deref()
        .and_then(|name| locale_info(name, LOCALE_SISO639LANGNAME))
        .map(|code| code.to_uppercase())
        .unwrap_or_else(|| format!("{id:04X}"));
    let name = locale
        .as_deref()
        .and_then(|name| {
            locale_info(name, LOCALE_SNATIVEDISPLAYNAME)
                .or_else(|| locale_info(name, LOCALE_SENGLISHDISPLAYNAME))
        })
        .unwrap_or_else(|| code.clone());
    let layout = match klid(hkl) {
        Klid::Direct(klid) => Some(klid),
        Klid::LayoutId(id) => klid_for_layout_id(id),
    }
    .and_then(|klid| layout_text(&klid));
    Language { code, name, layout }
}

fn locale_name(langid: u16) -> Option<String> {
    let mut buffer = [0u16; 86];
    let length = unsafe {
        LCIDToLocaleName(
            u32::from(langid),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            LOCALE_ALLOW_NEUTRAL_NAMES,
        )
    };
    from_wide(&buffer, length)
}

fn locale_info(locale: &str, kind: u32) -> Option<String> {
    let name = wide(locale);
    let mut buffer = [0u16; 128];
    let length = unsafe {
        GetLocaleInfoEx(
            name.as_ptr(),
            kind,
            buffer.as_mut_ptr(),
            buffer.len() as i32,
        )
    };
    from_wide(&buffer, length)
}

/// A length from a Win32 call that counts the terminating NUL.
fn from_wide(buffer: &[u16], length: i32) -> Option<String> {
    let length = usize::try_from(length).ok()?.checked_sub(1)?;
    let text = String::from_utf16_lossy(buffer.get(..length)?);
    (!text.is_empty()).then_some(text)
}

fn layout_text(klid: &str) -> Option<String> {
    registry_string(&format!(r"{LAYOUTS_KEY}\{klid}"), "Layout Text")
}

/// Every KLID that has a "Layout Id", read once: about two hundred small
/// registry reads the first time a variant layout is shown, none after.
fn klid_for_layout_id(id: u16) -> Option<String> {
    static IDS: OnceLock<BTreeMap<u16, String>> = OnceLock::new();
    IDS.get_or_init(layout_ids).get(&id).cloned()
}

fn layout_ids() -> BTreeMap<u16, String> {
    let mut ids = BTreeMap::new();
    let mut key: HKEY = std::ptr::null_mut();
    let path = wide(LAYOUTS_KEY);
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) }
        != ERROR_SUCCESS
    {
        return ids;
    }
    for index in 0.. {
        let mut name = [0u16; 16];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                name.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        let klid = String::from_utf16_lossy(&name[..length as usize]);
        let value = registry_string(&format!(r"{LAYOUTS_KEY}\{klid}"), "Layout Id");
        if let Some(id) = value.and_then(|text| u16::from_str_radix(text.trim(), 16).ok()) {
            ids.insert(id, klid);
        }
    }
    unsafe { RegCloseKey(key) };
    ids
}

fn registry_string(path: &str, value: &str) -> Option<String> {
    let path = wide(path);
    let value = wide(value);
    let mut buffer = [0u16; 256];
    let mut bytes = std::mem::size_of_val(&buffer) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    from_wide(&buffer, (bytes / 2) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_is_the_low_word_even_when_sign_extended() {
        assert_eq!(langid(0x0409_0409), 0x0409);
        assert_eq!(langid(0xFFFF_FFFF_F0A8_0422), 0x0422);
    }

    #[test]
    fn plain_and_variant_layouts_lead_to_their_registry_key() {
        // English (US): the layout word equals the language word.
        assert_eq!(klid(0x0409_0409), Klid::Direct("00000409".into()));
        // English with the United Kingdom layout: the layout word is a LANGID.
        assert_eq!(klid(0x0809_0409), Klid::Direct("00000809".into()));
        // Ukrainian (Enhanced), as this PC loads it, sign-extended.
        assert_eq!(klid(0xFFFF_FFFF_F0A8_0422), Klid::LayoutId(0x00A8));
        // A Japanese input method.
        assert_eq!(klid(0xE001_0411), Klid::Direct("E0010411".into()));
    }

    #[test]
    fn win32_lengths_drop_the_terminating_nul() {
        let text: Vec<u16> = "en-US\0".encode_utf16().collect();
        assert_eq!(from_wide(&text, 6), Some("en-US".to_string()));
        assert_eq!(from_wide(&text, 0), None);
        assert_eq!(from_wide(&text, 1), None);
    }

    #[test]
    fn this_pc_names_its_languages() {
        // Every Windows installation knows these two locales, whichever
        // keyboards are installed.
        let english = describe(0x0409_0409);
        assert_eq!(english.code, "EN");
        assert_eq!(english.name, "English (United States)");
        assert_eq!(english.layout.as_deref(), Some("US"));
        let ukrainian = describe(0x0422_0422);
        assert_eq!(ukrainian.code, "UK");
        assert_eq!(ukrainian.name, "українська (Україна)");
    }
}
