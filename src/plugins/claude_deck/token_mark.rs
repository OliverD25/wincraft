//! The token mark on each chat row (design section 16, decided
//! 2026-11-18): the settings **Token mark** (Line, Fill, Signal bars or
//! Off) and **Token mark counts** (Today or The last 7 days), the two
//! hotkey commands that change them, and the words of the row's tip. It
//! replaced the on/off **Token bar**; a saved Token bar on reads as Line,
//! off as Off. The drawing is in `stats::bar`.

use serde_json::Value;

use crate::core::host;
use crate::core::traits::PaletteCommand;

use super::model::ChatLine;
use super::rows::Look;
use super::stats::view::{percent, Share};
use super::{Handle, Settings, ID};

pub const KEY: &str = "token_mark";
pub const COUNTS_KEY: &str = "token_mark_counts";
/// Not on the settings page: the style the on/off command turns the mark back on to.
pub const LAST_KEY: &str = "token_mark_last";
/// The on/off setting it replaced.
pub const OLD_KEY: &str = "show_token_bar";

pub const LABEL: &str = "Token mark";
pub const COUNTS_LABEL: &str = "Token mark counts";
pub const LINE: &str = "Line";
pub const FILL: &str = "Fill";
pub const SIGNAL: &str = "Signal bars";
pub const OFF: &str = "Off";
pub const TODAY: &str = "Today";
pub const WEEK: &str = "The last 7 days";
/// The quick settings' words for the two counts.
pub const COUNT_TODAY: &str = "Count today";
pub const COUNT_WEEK: &str = "Count the last 7 days";
pub const ON_OFF_COMMAND: &str = "ClaudeDeck: Token mark on or off";
pub const COUNTS_COMMAND: &str = "ClaudeDeck: Token mark, today or 7 days";

/// How a chat row shows its share of the tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// A thin line at the bottom of the row.
    #[default]
    Line,
    /// The row's background, filled from the left.
    Fill,
    /// Four small bars before the status mark.
    Signal,
    Off,
}

/// The three styles that draw something, in the menu's order.
pub const STYLES: [(&str, Style); 3] = [
    (LINE, Style::Line),
    (FILL, Style::Fill),
    (SIGNAL, Style::Signal),
];

/// Which tokens the share is of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Counts {
    Today,
    #[default]
    Week,
}

pub fn style_word(style: Style) -> &'static str {
    match style {
        Style::Line => LINE,
        Style::Fill => FILL,
        Style::Signal => SIGNAL,
        Style::Off => OFF,
    }
}

fn style_of(word: &str) -> Option<Style> {
    match word {
        LINE => Some(Style::Line),
        FILL => Some(Style::Fill),
        SIGNAL => Some(Style::Signal),
        OFF => Some(Style::Off),
        _ => None,
    }
}

pub fn counts_word(counts: Counts) -> &'static str {
    match counts {
        Counts::Today => TODAY,
        Counts::Week => WEEK,
    }
}

/// The three settings from a settings file: the style, the style to come
/// back to, and the counts. A file from before has only the Token bar.
pub fn read(settings: &Value) -> (Style, Style, Counts) {
    let word = |key: &str| settings.get(key).and_then(Value::as_str);
    let style = word(KEY).and_then(style_of).unwrap_or_else(|| {
        match settings.get(OLD_KEY).and_then(Value::as_bool) {
            Some(false) => Style::Off,
            _ => Style::Line,
        }
    });
    let last = match style {
        Style::Off => word(LAST_KEY)
            .and_then(style_of)
            .filter(|s| *s != Style::Off)
            .unwrap_or(Style::Line),
        on => on,
    };
    let counts = match word(COUNTS_KEY) {
        Some(TODAY) => Counts::Today,
        _ => Counts::Week,
    };
    (style, last, counts)
}

/// Rewrites a file from before: the host has filled `token_mark` in from
/// the defaults by now, so the old Token bar decides it, and goes.
pub fn migrate(settings: &mut Value) {
    let Some(map) = settings.as_object_mut() else {
        return;
    };
    if let Some(old) = map.remove(OLD_KEY) {
        let style = if old.as_bool() == Some(false) {
            OFF
        } else {
            LINE
        };
        map.insert(KEY.to_string(), Value::String(style.to_string()));
    }
}

/// The settings to save for a change, as (key, value) pairs.
pub type Saves = Vec<(&'static str, Value)>;

fn word(text: &str) -> Value {
    Value::String(text.to_string())
}

/// Sets a style. Turning the mark off keeps the style it had, for the
/// on/off command to come back to.
pub fn set_style(settings: &mut Settings, style: Style) -> Saves {
    let mut saves = vec![(KEY, word(style_word(style)))];
    if style == Style::Off {
        if settings.token_mark != Style::Off {
            settings.token_last = settings.token_mark;
        }
        saves.push((LAST_KEY, word(style_word(settings.token_last))));
    } else {
        settings.token_last = style;
    }
    settings.token_mark = style;
    saves
}

/// The on/off command: off, or back on in the style it had last.
pub fn flip(settings: &mut Settings) -> Saves {
    let style = if settings.token_mark == Style::Off {
        settings.token_last
    } else {
        Style::Off
    };
    set_style(settings, style)
}

pub fn set_counts(settings: &mut Settings, counts: Counts) -> Saves {
    settings.token_counts = counts;
    vec![(COUNTS_KEY, word(counts_word(counts)))]
}

/// The counts command: Today and the last 7 days.
pub fn switch_counts(settings: &mut Settings) -> Saves {
    let counts = match settings.token_counts {
        Counts::Today => Counts::Week,
        Counts::Week => Counts::Today,
    };
    set_counts(settings, counts)
}

/// The settings page turned the mark off: keep the style it had.
pub fn keep_last(before: &Settings, after: &mut Settings) -> Option<Style> {
    let gone_off = after.token_mark == Style::Off && before.token_mark != Style::Off;
    (gone_off && after.token_last != before.token_mark).then(|| {
        after.token_last = before.token_mark;
        before.token_mark
    })
}

/// What a hotkey or palette command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Flip,
    Counts,
}

/// The keys the user set for the two hotkey commands ("" for none), kept
/// for the quick settings menu. Not saved: the host gives them at start.
static KEYS: std::sync::Mutex<(String, String)> =
    std::sync::Mutex::new((String::new(), String::new()));

pub fn set_keys(flip: &str, counts: &str) {
    if let Ok(mut keys) = KEYS.lock() {
        *keys = (flip.to_string(), counts.to_string());
    }
}

/// The keys of the on/off command, empty for none.
pub fn flip_keys() -> String {
    KEYS.lock().map(|keys| keys.0.clone()).unwrap_or_default()
}

/// The grey note under the two counts: only when the counts command has
/// keys.
pub fn switch_note() -> Option<String> {
    let keys = KEYS.lock().map(|keys| keys.1.clone()).unwrap_or_default();
    (!keys.is_empty()).then(|| format!("{keys} switches the two."))
}

/// Carries out a hotkey or a palette command on the settings.
pub fn apply(settings: &mut Settings, asked: Asked) -> Saves {
    match asked {
        Asked::Flip => flip(settings),
        Asked::Counts => switch_counts(settings),
    }
}

/// The palette commands' ids; ClaudeDeck's own run from 1 to 3.
const COMMAND_FLIP: u32 = 4;
const COMMAND_COUNTS: u32 = 5;

/// The palette command for a key.
pub fn command(asked: Asked) -> PaletteCommand {
    let (id, label, hint, words) = match asked {
        Asked::Flip => (
            COMMAND_FLIP,
            ON_OFF_COMMAND,
            "The share of tokens on each chat row",
            "token bar usage stats line fill signal",
        ),
        Asked::Counts => (
            COMMAND_COUNTS,
            COUNTS_COMMAND,
            "Count today's tokens or the last 7 days'",
            "token bar usage stats week",
        ),
    };
    PaletteCommand {
        id,
        label,
        hint,
        words,
        confirm: "",
        list: false,
        toggle: None,
    }
}

/// What a palette command asks for.
pub fn asked_by(id: u32) -> Option<Asked> {
    match id {
        COMMAND_FLIP => Some(Asked::Flip),
        COMMAND_COUNTS => Some(Asked::Counts),
        _ => None,
    }
}

/// A hotkey or palette command, on the host thread: change and save.
pub fn run(shared: &Handle, asked: Asked) {
    let saves = match shared.lock() {
        Ok(mut shared) => apply(&mut shared.settings, asked),
        Err(_) => Vec::new(),
    };
    for (key, value) in saves {
        host::save_setting(ID, key, value);
    }
}

/// A chat row's mark, when it shows one: never on a stand-in line or in
/// mini mode.
pub fn share_of(look: &Look, line: &ChatLine) -> Option<Share> {
    let shown = look.settings.token_mark != Style::Off && !look.settings.mini && !line.stand_in;
    shown
        .then(|| look.shares.get(&line.chat.id, look.settings.token_counts))
        .flatten()
}

/// The hover panel's line for a chat: "12% of the last 7 days".
pub fn tip_line(look: &Look, chat_id: &str) -> Option<String> {
    if look.settings.token_mark == Style::Off || look.settings.mini {
        return None;
    }
    let share = look.shares.get(chat_id, look.settings.token_counts)?;
    Some(tip(
        &percent(share.percent, 100.0),
        look.settings.token_counts,
    ))
}

/// The row tip's line: "12% of today" or "12% of the last 7 days".
pub fn tip(percent: &str, counts: Counts) -> String {
    match counts {
        Counts::Today => format!("{percent} of today"),
        Counts::Week => format!("{percent} of the last 7 days"),
    }
}

/// The signal bars lit for a share of `percent` (0 to 100).
pub fn level(percent: f64) -> u8 {
    match percent {
        p if p < 6.0 => 1,
        p if p < 12.0 => 2,
        p if p < 25.0 => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests;
