//! The list under the Check a shortcut box while it holds a combination:
//! that combination's row, then free combinations close to it. "Close" is
//! the same key with one modifier added, removed or swapped, or the same
//! modifiers with a key next to it on the keyboard. Win+Alt ones come first,
//! because WinCraft's own defaults start with Win+Alt (Principle 4).

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

use crate::core::hotkeys;
use crate::core::traits::Hotkey;

use super::probe::{Entry, Status};

/// At most this many free combinations follow the typed one.
pub const MAX: usize = 8;

const MODIFIER_BITS: [u32; 4] = [MOD_WIN, MOD_ALT, MOD_CONTROL, MOD_SHIFT];

/// Keys in the order they sit on a keyboard row, so a key's neighbours are
/// the ones beside it.
const KEY_ROWS: [&str; 4] = [
    "`1234567890-=",
    "QWERTYUIOP[]\\",
    "ASDFGHJKL;'",
    "ZXCVBNM,./",
];

/// The keys beside `vk`: the next one first, then the one before. F-keys
/// step through F1-F24; a key on no row steps through the detector's key
/// list.
fn neighbours(vk: u32) -> Vec<u32> {
    let mut rows: Vec<Vec<u32>> = KEY_ROWS
        .iter()
        .map(|row| {
            row.chars()
                .filter_map(|c| hotkeys::parse(&c.to_string()).ok().map(|key| key.vk))
                .collect()
        })
        .collect();
    rows.push((0x70..0x70 + 24).collect());
    let row = rows
        .into_iter()
        .find(|row| row.contains(&vk))
        .unwrap_or_else(hotkeys::all_keys);
    let Some(at) = row.iter().position(|key| *key == vk) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    if let Some(next) = row.get(at + 1) {
        keys.push(*next);
    }
    if let Some(before) = at.checked_sub(1).and_then(|at| row.get(at)) {
        keys.push(*before);
    }
    keys
}

/// The combinations close to `hotkey`, closest first, each once: one
/// modifier added, then one removed, then one swapped for another, then the
/// same modifiers with a neighbouring key.
pub fn candidates(hotkey: Hotkey) -> Vec<Hotkey> {
    let mods = hotkey.modifiers & !MOD_NOREPEAT;
    let mut sets = Vec::new();
    for bit in MODIFIER_BITS {
        if mods & bit == 0 {
            sets.push(mods | bit);
        }
    }
    for bit in MODIFIER_BITS {
        if mods & bit != 0 && mods & !bit != 0 {
            sets.push(mods & !bit);
        }
    }
    for gone in MODIFIER_BITS.into_iter().filter(|bit| mods & bit != 0) {
        for came in MODIFIER_BITS.into_iter().filter(|bit| mods & bit == 0) {
            sets.push(mods & !gone | came);
        }
    }
    let mut found: Vec<Hotkey> = sets
        .into_iter()
        .map(|set| Hotkey {
            modifiers: set | MOD_NOREPEAT,
            vk: hotkey.vk,
        })
        .collect();
    if mods != 0 {
        found.extend(neighbours(hotkey.vk).into_iter().map(|vk| Hotkey {
            modifiers: mods | MOD_NOREPEAT,
            vk,
        }));
    }
    let mut seen = Vec::new();
    found.retain(|key| {
        let new = *key != with_norepeat(hotkey) && !seen.contains(key);
        seen.push(*key);
        new
    });
    found
}

fn with_norepeat(hotkey: Hotkey) -> Hotkey {
    Hotkey {
        modifiers: hotkey.modifiers | MOD_NOREPEAT,
        vk: hotkey.vk,
    }
}

fn win_alt(hotkey: &Hotkey) -> bool {
    hotkey.modifiers & (MOD_WIN | MOD_ALT) == MOD_WIN | MOD_ALT
}

/// The rows to show for `hotkey`, as indexes into `entries`: its own row
/// when the scan has one, then up to `MAX` free ones close to it, Win+Alt
/// ones first.
pub fn results(entries: &[Entry], hotkey: Hotkey) -> Vec<usize> {
    let find = |key: Hotkey| entries.iter().position(|entry| entry.hotkey == key);
    let mut close: Vec<Hotkey> = candidates(hotkey);
    // A stable sort keeps "closest first" inside each group.
    close.sort_by_key(|key| !win_alt(key));
    let mut rows: Vec<usize> = find(with_norepeat(hotkey)).into_iter().collect();
    rows.extend(
        close
            .into_iter()
            .filter_map(find)
            .filter(|at| entries[*at].status == Status::Free)
            .take(MAX),
    );
    rows
}

#[cfg(test)]
mod tests {
    use super::super::probe::classify;
    use super::*;

    fn key(text: &str) -> Hotkey {
        hotkeys::parse(text).expect(text)
    }

    fn names(keys: &[Hotkey]) -> Vec<String> {
        keys.iter().map(|key| hotkeys::format(*key)).collect()
    }

    #[test]
    fn neighbours_follow_the_keyboard_rows() {
        let next = |text: &str| {
            names(
                &neighbours(key(text).vk)
                    .into_iter()
                    .map(|vk| Hotkey { modifiers: 0, vk })
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(next("`"), ["1"]);
        assert_eq!(next("5"), ["6", "4"]);
        assert_eq!(next("F1"), ["F2"]);
        assert_eq!(next("F12"), ["F13", "F11"]);
        assert_eq!(next("L"), [";", "K"]);
        assert_eq!(next("\\"), ["]"]);
        assert!(
            !next("PageUp").is_empty(),
            "a key on no row uses the key list"
        );
    }

    #[test]
    fn candidates_add_then_remove_then_swap_then_step_the_key() {
        let close = names(&candidates(key("Win+`")));
        assert_eq!(
            close,
            [
                "Win+Alt+`",
                "Ctrl+Win+`",
                "Win+Shift+`",
                "Alt+`",
                "Ctrl+`",
                "Shift+`",
                "Win+1"
            ]
            .map(|text| hotkeys::format(key(text)))
        );
        let close = candidates(key("Win+Alt+F5"));
        assert!(close.contains(&key("Win+F5")), "a modifier removed");
        assert!(close.contains(&key("Win+Alt+F6")) && close.contains(&key("Win+Alt+F4")));
        assert!(!close.contains(&key("Win+Alt+F5")), "never the typed one");
    }

    fn scan(free: &[&str], taken: &[&str], typed: &str) -> Vec<Entry> {
        let mut entries = vec![classify(key(typed), true, None)];
        entries.extend(free.iter().map(|text| classify(key(text), false, None)));
        entries.extend(taken.iter().map(|text| classify(key(text), true, None)));
        entries
    }

    #[test]
    fn results_put_the_typed_row_first_and_win_alt_before_the_rest() {
        let entries = scan(
            &["Ctrl+Alt+F8", "Win+Alt+F9", "Alt+F9", "Ctrl+Alt+F10"],
            &["Ctrl+Alt+Shift+F9"],
            "Ctrl+Alt+F9",
        );
        let shown: Vec<String> = results(&entries, key("Ctrl+Alt+F9"))
            .into_iter()
            .map(|at| hotkeys::format(entries[at].hotkey))
            .collect();
        assert_eq!(
            shown,
            [
                "Ctrl+Alt+F9",
                "Win+Alt+F9",
                "Alt+F9",
                "Ctrl+Alt+F10",
                "Ctrl+Alt+F8"
            ]
            .map(|text| hotkeys::format(key(text)))
        );
    }

    #[test]
    fn results_skip_taken_and_unknown_combinations_and_stop_at_the_most() {
        let all_free: Vec<String> = candidates(key("Ctrl+Alt+F7"))
            .into_iter()
            .map(hotkeys::format)
            .collect();
        let free: Vec<&str> = all_free.iter().map(String::as_str).collect();
        assert!(free.len() > MAX);
        let entries = scan(&free, &[], "Ctrl+Alt+F7");
        let shown = results(&entries, key("Ctrl+Alt+F7"));
        assert_eq!(shown.len(), 1 + MAX);
        assert_eq!(shown[0], 0);

        // Taken neighbours are left out; a typed combination the scan does
        // not list (no modifier) still gets its free neighbours.
        let entries = scan(&["Win+Alt+F5"], &["Ctrl+Alt+F5"], "Win+F5");
        let shown = results(&entries, key("F5"));
        assert!(shown.is_empty(), "F5 alone: nothing close is in the scan");
        let shown = results(&entries, key("Alt+F5"));
        assert_eq!(shown, [1], "only the free Win+Alt+F5");
    }
}
