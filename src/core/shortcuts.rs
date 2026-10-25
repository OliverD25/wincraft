//! Every command's shortcut: one key press, or two in a row (a chord, such
//! as Win+Alt+K, then S). Windows only knows one press, so the host
//! registers each first press once, and after it waits up to three seconds
//! for the second with `core::chord`. This module holds the rules that need
//! no Windows: which keys can be used, what clashes, the search on the
//! Keyboard shortcuts page, and which first presses to register.

use std::collections::BTreeSet;
use std::time::Duration;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_WIN};

use crate::core::chord::{Stroke, Verdict};
use crate::core::hotkeys::{self, Shortcut};
use crate::core::traits::Hotkey;
use crate::core::ui_bridge::{CommandId, ShortcutChange, ShortcutInfo};

/// How long WinCraft waits for the second key press of a chord.
pub const CHORD_WAIT: Duration = Duration::from_secs(3);

/// The group of WinCraft's own commands, listed first.
pub const HOST_GROUP: &str = "WinCraft";

const ESCAPE: u32 = 0x1B;

/// Keys Windows keeps for itself, or that WinCraft must never take from
/// it: Win+number opens taskbar buttons and Alt+Tab switches windows.
const RESERVED: &[&str] = &[
    "Win+L",
    "Ctrl+Alt+Delete",
    "Ctrl+Shift+Esc",
    "Win+Ctrl+Shift+B",
    "Alt+Tab",
    "Alt+Shift+Tab",
    "Ctrl+Alt+Tab",
    "Win+Tab",
    "Ctrl+Esc",
    "Win+1",
    "Win+2",
    "Win+3",
    "Win+4",
    "Win+5",
    "Win+6",
    "Win+7",
    "Win+8",
    "Win+9",
    "Win+0",
];

pub fn is_reserved(hotkey: Hotkey) -> bool {
    RESERVED.iter().any(|text| {
        hotkeys::parse(text)
            .map(|reserved| hotkeys::same(reserved, hotkey))
            .unwrap_or(false)
    })
}

/// The empty text means no shortcut; text that does not parse counts as
/// none too.
pub fn parse(text: &str) -> Option<Shortcut> {
    if text.trim().is_empty() {
        return None;
    }
    hotkeys::parse_shortcut(text).ok()
}

/// Whether two bindings as text name the same keys.
pub fn same_text(a: &str, b: &str) -> bool {
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a.same_as(&b),
        (None, None) => true,
        _ => false,
    }
}

pub fn is_changed(info: &ShortcutInfo) -> bool {
    !same_text(&info.binding, &info.default_binding)
}

/// What the page says about keys pressed for `target`, in the order the
/// design checks them; the first problem found wins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    Fine,
    /// Windows keeps these keys.
    Reserved {
        keys: String,
    },
    /// A first press needs Win, Ctrl or Alt.
    NeedsModifier {
        keys: String,
    },
    /// The palette's hotkey takes one press.
    OnePressOnly,
    /// The keys are the first press of another command's chord.
    ChordStart {
        keys: String,
        other: String,
    },
    /// The chord's first press is another command's shortcut on its own.
    StartTaken {
        keys: String,
        other: String,
        group: String,
    },
    /// Another command has exactly these keys.
    UsedBy {
        keys: String,
        id: String,
        label: String,
        group: String,
    },
    /// Another app holds the first press.
    HeldByApp {
        keys: String,
    },
}

impl Check {
    /// Danger: the keys cannot be saved at all.
    pub fn blocks(&self) -> bool {
        matches!(
            self,
            Check::Reserved { .. }
                | Check::NeedsModifier { .. }
                | Check::OnePressOnly
                | Check::ChordStart { .. }
                | Check::StartTaken { .. }
        )
    }

    /// The two lines under the field: what is wrong, and what to do.
    pub fn message(&self, target: &str) -> Option<(String, String)> {
        let lines = match self {
            Check::Fine => return None,
            Check::Reserved { keys } => (
                format!("Windows keeps {keys} for itself."),
                "Choose other keys.".to_string(),
            ),
            Check::NeedsModifier { keys } => (
                format!("{keys} needs Win, Ctrl or Alt with it."),
                "Hold Win, Ctrl or Alt and press a key.".to_string(),
            ),
            Check::OnePressOnly => (
                "The palette's shortcut is one key press, not a chord.".to_string(),
                "Choose keys without a second press.".to_string(),
            ),
            Check::ChordStart { keys, .. } => (
                format!(
                    "{keys} is the start of a chord, so it cannot also be a shortcut on its own."
                ),
                "Press a second key to make a chord, or choose other keys.".to_string(),
            ),
            Check::StartTaken { keys, other, group } => (
                format!("{keys} is already the shortcut of {other} in {group}."),
                "A chord cannot start with another command's shortcut. Choose other keys."
                    .to_string(),
            ),
            Check::UsedBy {
                keys, label, group, ..
            } => (
                format!("Already used by {label} in {group}."),
                format!("Replace gives {keys} to {target}. {label} is left with no shortcut."),
            ),
            Check::HeldByApp { keys } => (
                format!("Another app holds {keys}, so it may not reach WinCraft."),
                "You can save it anyway and try it.".to_string(),
            ),
        };
        Some(lines)
    }
}

/// Checks `candidate` for the command `target`. `held` asks Windows
/// whether another app holds a first press; it is not asked about a press
/// WinCraft itself uses.
pub fn check(
    candidate: &Shortcut,
    target: &str,
    all: &[ShortcutInfo],
    held: impl Fn(Hotkey) -> bool,
) -> Check {
    let first_keys = hotkeys::format(candidate.first);
    if is_reserved(candidate.first) {
        return Check::Reserved { keys: first_keys };
    }
    if let Some(second) = candidate.second.filter(|second| is_reserved(*second)) {
        return Check::Reserved {
            keys: hotkeys::format(second),
        };
    }
    if candidate.first.modifiers & (MOD_WIN | MOD_CONTROL | MOD_ALT) == 0 {
        return Check::NeedsModifier { keys: first_keys };
    }
    let me = all.iter().find(|info| info.id == target);
    if candidate.is_chord() && me.is_some_and(|info| info.one_press) {
        return Check::OnePressOnly;
    }
    let others: Vec<(&ShortcutInfo, Shortcut)> = all
        .iter()
        .filter(|info| info.id != target)
        .filter_map(|info| parse(&info.binding).map(|keys| (info, keys)))
        .collect();
    for (info, keys) in &others {
        if !candidate.is_chord() && keys.is_chord() && hotkeys::same(keys.first, candidate.first) {
            return Check::ChordStart {
                keys: first_keys,
                other: info.label.clone(),
            };
        }
        if candidate.is_chord() && !keys.is_chord() && hotkeys::same(keys.first, candidate.first) {
            return Check::StartTaken {
                keys: first_keys,
                other: info.label.clone(),
                group: info.group.clone(),
            };
        }
    }
    if let Some((info, _)) = others.iter().find(|(_, keys)| keys.same_as(candidate)) {
        return Check::UsedBy {
            keys: hotkeys::format_shortcut(candidate),
            id: info.id.clone(),
            label: info.label.clone(),
            group: info.group.clone(),
        };
    }
    let mine = all
        .iter()
        .filter_map(|info| parse(&info.binding))
        .any(|keys| hotkeys::same(keys.first, candidate.first));
    if !mine && held(candidate.first) {
        return Check::HeldByApp { keys: first_keys };
    }
    Check::Fine
}

/// Replace: `target` gets the keys and `other` is left with none.
pub fn replace(target: &str, keys: &Shortcut, other: &str) -> Vec<ShortcutChange> {
    vec![
        ShortcutChange {
            id: other.to_string(),
            binding: Some(String::new()),
        },
        ShortcutChange {
            id: target.to_string(),
            binding: Some(hotkeys::format_shortcut(keys)),
        },
    ]
}

/// Every command whose keys clash with another's or that another app
/// holds: the Conflicts chip and the group badges count these.
pub fn conflicts(all: &[ShortcutInfo]) -> BTreeSet<String> {
    let bound: Vec<(&ShortcutInfo, Shortcut)> = all
        .iter()
        .filter_map(|info| parse(&info.binding).map(|keys| (info, keys)))
        .collect();
    let mut found = BTreeSet::new();
    for (index, (info, keys)) in bound.iter().enumerate() {
        if !info.registered {
            found.insert(info.id.clone());
        }
        for (other, theirs) in &bound[index + 1..] {
            let clash = keys.same_as(theirs)
                || (keys.is_chord() != theirs.is_chord()
                    && hotkeys::same(keys.first, theirs.first));
            if clash {
                found.insert(info.id.clone());
                found.insert(other.id.clone());
            }
        }
    }
    found
}

/// The chips above the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Changed,
    Unbound,
    Conflicts,
}

impl Filter {
    pub const ALL: [Filter; 4] = [
        Filter::All,
        Filter::Changed,
        Filter::Unbound,
        Filter::Conflicts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Changed => "Changed",
            Filter::Unbound => "No shortcut",
            Filter::Conflicts => "Conflicts",
        }
    }

    pub fn keeps(self, info: &ShortcutInfo, conflicts: &BTreeSet<String>) -> bool {
        match self {
            Filter::All => true,
            Filter::Changed => is_changed(info),
            Filter::Unbound => parse(&info.binding).is_none(),
            Filter::Conflicts => conflicts.contains(&info.id),
        }
    }
}

/// The words of a query: "win+alt+k" and "alt k" both give key words.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| c.is_whitespace() || c == '+' || c == ',')
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether every word of `query` is in the command's name, description or
/// plugin, or is one of its keys, in any order. A word of one or two
/// letters must start a word, so "k" finds the K key and not every "k".
pub fn matches(info: &ShortcutInfo, query: &str) -> bool {
    let wanted = words(query);
    if wanted.is_empty() {
        return true;
    }
    let text = words(&format!(
        "{} {} {}",
        info.label, info.description, info.group
    ));
    let keys: Vec<String> = parse(&info.binding)
        .map(|keys| {
            keys.steps()
                .concat()
                .iter()
                .map(|key| key.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    wanted.iter().all(|word| {
        keys.iter().any(|key| key == word)
            || text
                .iter()
                .any(|have| have.starts_with(word) || (word.len() >= 3 && have.contains(word)))
    })
}

/// Whether the command's keys begin with `pressed`, for Search by keys.
pub fn starts_with(info: &ShortcutInfo, pressed: &[Hotkey]) -> bool {
    let Some(keys) = parse(&info.binding) else {
        return false;
    };
    let mine: Vec<Hotkey> = std::iter::once(keys.first).chain(keys.second).collect();
    !pressed.is_empty()
        && pressed.len() <= mine.len()
        && pressed
            .iter()
            .zip(&mine)
            .all(|(a, b)| hotkeys::same(*a, *b))
}

/// The groups in the page's order: WinCraft, then the plugins A to Z. Each
/// holds positions in `all`, in the order the host listed them.
pub fn groups(
    all: &[ShortcutInfo],
    keep: impl Fn(&ShortcutInfo) -> bool,
) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (index, info) in all.iter().enumerate().filter(|(_, info)| keep(info)) {
        match groups.iter_mut().find(|(name, _)| *name == info.group) {
            Some((_, rows)) => rows.push(index),
            None => groups.push((info.group.clone(), vec![index])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| {
        (a != HOST_GROUP)
            .cmp(&(b != HOST_GROUP))
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });
    groups
}

/// One first press to register, and what it does: run a command at once,
/// or wait for one of the second presses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstPress {
    pub hotkey: Hotkey,
    pub plain: Option<CommandId>,
    pub seconds: Vec<(Hotkey, CommandId)>,
}

/// Each first press once, however many chords start with it.
pub fn plan(bound: &[(Shortcut, CommandId)]) -> Vec<FirstPress> {
    let mut presses: Vec<FirstPress> = Vec::new();
    for (keys, command) in bound {
        let at = match presses
            .iter()
            .position(|press| hotkeys::same(press.hotkey, keys.first))
        {
            Some(at) => at,
            None => {
                presses.push(FirstPress {
                    hotkey: keys.first,
                    plain: None,
                    seconds: Vec::new(),
                });
                presses.len() - 1
            }
        };
        let press = &mut presses[at];
        match keys.second {
            Some(second) => press.seconds.push((second, *command)),
            None if press.plain.is_none() => press.plain = Some(*command),
            None => log::warn!(
                "{} is the shortcut of two commands; the first one keeps it",
                hotkeys::format(keys.first)
            ),
        }
    }
    presses
}

/// A key pressed while a chord waits for its second press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordKey {
    Run(CommandId),
    Cancel,
    Other,
}

pub fn chord_key(press: &FirstPress, stroke: Stroke) -> ChordKey {
    if stroke.vk == ESCAPE && stroke.modifiers == 0 {
        return ChordKey::Cancel;
    }
    let pressed = Hotkey {
        modifiers: stroke.modifiers,
        vk: stroke.vk,
    };
    let exact = press
        .seconds
        .iter()
        .find(|(second, _)| hotkeys::same(*second, pressed));
    // Win+Alt+K, S works with Win+Alt still held from the first press, as
    // Win+Alt+A then a digit does.
    let held_over = hotkeys::same(
        Hotkey {
            modifiers: stroke.modifiers,
            vk: 0,
        },
        Hotkey {
            modifiers: press.hotkey.modifiers,
            vk: 0,
        },
    );
    let plain = Hotkey {
        modifiers: 0,
        vk: stroke.vk,
    };
    exact
        .or_else(|| {
            held_over
                .then(|| {
                    press
                        .seconds
                        .iter()
                        .find(|(second, _)| hotkeys::same(*second, plain))
                })
                .flatten()
        })
        .map_or(ChordKey::Other, |(_, command)| ChordKey::Run(*command))
}

/// A matching key and Esc are kept from the app in front; any other key
/// goes through and ends the wait.
pub fn chord_verdict(key: ChordKey) -> Verdict {
    match key {
        ChordKey::Run(_) | ChordKey::Cancel => Verdict::Take,
        ChordKey::Other => Verdict::End,
    }
}

/// The line on screen while a chord waits (Principle 1: it names WinCraft).
pub fn wait_line(keys: &str) -> String {
    format!("WinCraft: {keys}, waiting for the next key")
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::core::chord;
    use crate::core::ui_bridge::{ActionKind, HostCommand};

    fn info(id: &str, group: &str, label: &str, binding: &str, default: &str) -> ShortcutInfo {
        ShortcutInfo {
            id: id.to_string(),
            command: Some(CommandId::Host(HostCommand::OpenSettings)),
            group: group.to_string(),
            plugin: None,
            category: String::new(),
            label: label.to_string(),
            description: String::new(),
            binding: binding.to_string(),
            default_binding: default.to_string(),
            registered: true,
            one_press: false,
        }
    }

    fn keys(text: &str) -> Shortcut {
        hotkeys::parse_shortcut(text).unwrap()
    }

    fn sample() -> Vec<ShortcutInfo> {
        vec![
            info(
                "screen_dimmer.dim_all",
                "ScreenDimmer",
                "Dim all monitors",
                "Win+Alt+D",
                "Win+Alt+D",
            ),
            info(
                "layout_keeper.save",
                "LayoutKeeper",
                "Save current layout",
                "Win+Alt+K, S",
                "",
            ),
            info(
                "layout_keeper.arrange",
                "LayoutKeeper",
                "Arrange strip",
                "Win+Alt+A",
                "Win+Alt+A",
            ),
            info("wincraft.quit", HOST_GROUP, "Quit WinCraft", "", ""),
        ]
    }

    #[test]
    fn the_checks_come_in_the_designs_order() {
        let all = sample();
        let never = |_: Hotkey| false;
        let always = |_: Hotkey| true;
        assert_eq!(
            check(&keys("Win+L"), "wincraft.quit", &all, always),
            Check::Reserved {
                keys: "Win+L".to_string()
            }
        );
        assert!(matches!(
            check(&keys("Win+3"), "wincraft.quit", &all, never),
            Check::Reserved { .. }
        ));
        assert!(matches!(
            check(&keys("Win+Alt+K, Win+L"), "wincraft.quit", &all, never),
            Check::Reserved { .. }
        ));
        assert!(matches!(
            check(&keys("Shift+K"), "wincraft.quit", &all, never),
            Check::NeedsModifier { .. }
        ));
        // A plain shortcut that starts another command's chord.
        assert!(matches!(
            check(&keys("Win+Alt+K"), "wincraft.quit", &all, always),
            Check::ChordStart { .. }
        ));
        // A chord that starts with another command's plain shortcut.
        assert!(matches!(
            check(&keys("Win+Alt+D, 1"), "wincraft.quit", &all, always),
            Check::StartTaken { .. }
        ));
        let used = check(&keys("Win+Alt+D"), "wincraft.quit", &all, always);
        assert!(matches!(&used, Check::UsedBy { id, .. } if id == "screen_dimmer.dim_all"));
        assert!(!used.blocks());
        assert_eq!(
            check(&keys("Win+Alt+X"), "wincraft.quit", &all, always),
            Check::HeldByApp {
                keys: "Win+Alt+X".to_string()
            }
        );
        assert_eq!(
            check(&keys("Win+Alt+X"), "wincraft.quit", &all, never),
            Check::Fine
        );
        // Another chord with the same first press is fine, and WinCraft's
        // own first press is never "held by another app".
        assert_eq!(
            check(&keys("Win+Alt+K, L"), "wincraft.quit", &all, always),
            Check::Fine
        );
        // A command's own keys are no clash with itself.
        assert_eq!(
            check(&keys("Win+Alt+D"), "screen_dimmer.dim_all", &all, always),
            Check::Fine
        );
    }

    #[test]
    fn the_palette_hotkey_takes_one_press() {
        let mut all = sample();
        all.push(ShortcutInfo {
            one_press: true,
            ..info(
                "wincraft.palette",
                HOST_GROUP,
                "Open the palette",
                "Win+Alt+P",
                "Win+Alt+P",
            )
        });
        assert_eq!(
            check(&keys("Win+Alt+P, 1"), "wincraft.palette", &all, |_| false),
            Check::OnePressOnly
        );
    }

    #[test]
    fn the_messages_name_the_keys_and_the_other_command() {
        let used = Check::UsedBy {
            keys: "Win+Alt+D".to_string(),
            id: "screen_dimmer.dim_all".to_string(),
            label: "Dim all monitors".to_string(),
            group: "ScreenDimmer".to_string(),
        };
        let (first, second) = used.message("Forget saved layout").unwrap();
        assert_eq!(first, "Already used by Dim all monitors in ScreenDimmer.");
        assert_eq!(
            second,
            "Replace gives Win+Alt+D to Forget saved layout. Dim all monitors is left with no shortcut."
        );
        assert!(Check::Fine.message("x").is_none());
        let reserved = Check::Reserved {
            keys: "Win+L".to_string(),
        };
        assert_eq!(
            reserved.message("x").unwrap().0,
            "Windows keeps Win+L for itself."
        );
        assert!(reserved.blocks());
    }

    #[test]
    fn replace_moves_the_keys_and_empties_the_other_command() {
        let changes = replace(
            "layout_keeper.forget",
            &keys("Win+Alt+D"),
            "screen_dimmer.dim_all",
        );
        assert_eq!(
            changes,
            vec![
                ShortcutChange {
                    id: "screen_dimmer.dim_all".to_string(),
                    binding: Some(String::new()),
                },
                ShortcutChange {
                    id: "layout_keeper.forget".to_string(),
                    binding: Some("Win+Alt+D".to_string()),
                },
            ]
        );
    }

    #[test]
    fn changed_means_other_keys_than_the_default() {
        let all = sample();
        assert!(!is_changed(&all[0]));
        assert!(is_changed(&all[1]));
        assert!(!is_changed(&all[3]));
        assert!(is_changed(&info("a", "A", "A", "", "Win+Alt+A")));
        assert!(!is_changed(&info("a", "A", "A", "win+alt+a", "Win+Alt+A")));
    }

    #[test]
    fn search_finds_name_description_plugin_and_keys_in_any_order() {
        let all = sample();
        let found = |query: &str| -> Vec<&str> {
            all.iter()
                .filter(|info| matches(info, query))
                .map(|info| info.id.as_str())
                .collect()
        };
        assert_eq!(found("layout save"), ["layout_keeper.save"]);
        assert_eq!(found("save layout"), ["layout_keeper.save"]);
        assert_eq!(found("dimmer"), ["screen_dimmer.dim_all"]);
        assert_eq!(found("win+alt+k"), ["layout_keeper.save"]);
        assert_eq!(found("alt k"), ["layout_keeper.save"]);
        assert_eq!(found("").len(), 4);
        assert!(found("nothing here").is_empty());
    }

    #[test]
    fn search_by_keys_finds_what_starts_with_the_presses() {
        let all = sample();
        let first = keys("Win+Alt+K").first;
        let hits: Vec<&str> = all
            .iter()
            .filter(|info| starts_with(info, &[first]))
            .map(|info| info.id.as_str())
            .collect();
        assert_eq!(hits, ["layout_keeper.save"]);
        let s = keys("S").first;
        assert!(starts_with(&all[1], &[first, s]));
        assert!(!starts_with(&all[1], &[first, keys("L").first]));
        assert!(!starts_with(&all[3], &[first]));
        assert!(!starts_with(&all[0], &[]));
    }

    #[test]
    fn the_chips_count_changed_unbound_and_conflicting_commands() {
        let mut all = sample();
        all.push(info("x.clash", "X", "Clash", "Win+Alt+D", ""));
        all.push(ShortcutInfo {
            registered: false,
            ..info("x.held", "X", "Held", "Win+Alt+H", "Win+Alt+H")
        });
        let found = conflicts(&all);
        assert_eq!(
            found.iter().map(String::as_str).collect::<Vec<_>>(),
            ["screen_dimmer.dim_all", "x.clash", "x.held"]
        );
        let count = |filter: Filter| all.iter().filter(|info| filter.keeps(info, &found)).count();
        assert_eq!(count(Filter::All), 6);
        assert_eq!(count(Filter::Changed), 2);
        assert_eq!(count(Filter::Unbound), 1);
        assert_eq!(count(Filter::Conflicts), 3);
        // A plain shortcut and a chord on the same first press clash too.
        let pair = vec![
            info("a", "A", "A", "Win+Alt+K", ""),
            info("b", "B", "B", "Win+Alt+K, S", ""),
        ];
        assert_eq!(conflicts(&pair).len(), 2);
    }

    #[test]
    fn wincraft_comes_first_then_the_plugins_a_to_z() {
        let all = sample();
        let order: Vec<String> = groups(&all, |_| true)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(order, ["WinCraft", "LayoutKeeper", "ScreenDimmer"]);
        let layout = &groups(&all, |_| true)[1].1;
        assert_eq!(layout, &vec![1, 2]);
        assert!(groups(&all, |_| false).is_empty());
    }

    #[test]
    fn each_first_press_is_registered_once() {
        let save = CommandId::Plugin {
            index: 0,
            kind: ActionKind::Palette,
            action: 1,
        };
        let forget = CommandId::Plugin {
            index: 0,
            kind: ActionKind::Palette,
            action: 2,
        };
        let quit = CommandId::Host(HostCommand::Exit);
        let presses = plan(&[
            (keys("Win+Alt+K, S"), save),
            (keys("Win+Alt+K, F"), forget),
            (keys("Win+Alt+Q"), quit),
        ]);
        assert_eq!(presses.len(), 2);
        assert_eq!(presses[0].plain, None);
        assert_eq!(presses[0].seconds.len(), 2);
        assert_eq!(presses[1].plain, Some(quit));
        assert!(presses[1].seconds.is_empty());
    }

    #[test]
    fn a_chord_runs_on_its_second_key_and_ends_on_any_other_key_esc_or_time() {
        let save = CommandId::Host(HostCommand::OpenLayout);
        let press = &plan(&[(keys("Win+Alt+K, S"), save)])[0];
        let s = Stroke {
            vk: 0x53,
            modifiers: 0,
        };
        let l = Stroke {
            vk: 0x4C,
            modifiers: 0,
        };
        let escape = Stroke {
            vk: ESCAPE,
            modifiers: 0,
        };
        assert_eq!(chord_key(press, s), ChordKey::Run(save));
        assert_eq!(chord_key(press, l), ChordKey::Other);
        assert_eq!(chord_key(press, escape), ChordKey::Cancel);
        assert_eq!(chord_verdict(ChordKey::Run(save)), Verdict::Take);
        assert_eq!(chord_verdict(ChordKey::Cancel), Verdict::Take);
        assert_eq!(chord_verdict(ChordKey::Other), Verdict::End);
        // With Shift held, S is another key.
        let shifted = Stroke {
            vk: 0x53,
            modifiers: windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_SHIFT,
        };
        assert_eq!(chord_key(press, shifted), ChordKey::Other);
        // Win+Alt still held from the first press counts as a plain S.
        let held = Stroke {
            vk: 0x53,
            modifiers: MOD_WIN | MOD_ALT,
        };
        assert_eq!(chord_key(press, held), ChordKey::Run(save));

        fn take(_: Stroke) -> Verdict {
            Verdict::Take
        }
        let start = Instant::now();
        let late = start + CHORD_WAIT + Duration::from_millis(1);
        assert_eq!(
            chord::verdict(s, start, CHORD_WAIT, late, take),
            Verdict::End
        );
        assert_eq!(
            chord::verdict(s, start, CHORD_WAIT, start + CHORD_WAIT, take),
            Verdict::Take
        );
    }

    #[test]
    fn the_wait_line_names_wincraft_and_the_keys() {
        assert_eq!(
            wait_line("Win+Alt+K"),
            "WinCraft: Win+Alt+K, waiting for the next key"
        );
    }

    #[test]
    fn empty_or_broken_text_is_no_shortcut() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("  "), None);
        assert_eq!(parse("Win+Banana"), None);
        assert!(parse("Win+Alt+K, S").unwrap().is_chord());
        assert!(same_text("", ""));
        assert!(!same_text("Win+Alt+A", ""));
    }
}
