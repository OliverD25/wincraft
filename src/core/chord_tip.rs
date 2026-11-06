//! The chord tip: a small panel at the bottom of the screen while a chord
//! waits for its next key (Principle 1: the user always sees that WinCraft
//! holds the keyboard). It shows the keys pressed so far, an empty keycap
//! where the next key goes, the keys that can follow with what each does,
//! and a bar that counts down the wait. When the chord ends it says how:
//! the command it ran, "Nothing assigned to X", or "Cancelled", then fades.
//!
//! The host shows it for the chords bound in Settings, Hotkeys, and a
//! plugin that runs its own wait through `core::chord` shows it with its
//! own list of keys (Principle 3: the host gives it for free). It is
//! click-through and never takes the focus.
//!
//! This file holds what is decided without Windows (the times, the
//! states, the sizes), so it is tested; `screen` puts it on screen and
//! `draw` paints it.

mod draw;
mod screen;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

use crate::core::chord::Stroke;
use crate::core::hotkeys;
use crate::core::traits::Hotkey;

pub use screen::{end, hide, show};

/// The tip appears this long after the first key, so a fast second key
/// never flashes it.
pub const SHOW_AFTER_MS: u64 = 150;
/// How long the closing states stay before they fade.
pub const MATCHED_MS: u64 = 600;
pub const UNASSIGNED_MS: u64 = 1200;
pub const CANCELLED_MS: u64 = 600;
pub const FADE_MS: u64 = 150;
/// A waiting tip whose owner never ended it goes this long after its time.
const LATE_MS: u64 = 1000;

/// One key that can follow, and what it does: "A", "Add this window".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub text: String,
}

impl Row {
    pub fn new(key: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            text: text.into(),
        }
    }
}

/// What the tip shows while it waits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Content {
    /// The keys pressed so far, one keycap each: "Win", "`".
    pub keys: Vec<String>,
    /// The keys that can follow.
    pub rows: Vec<Row>,
    /// How many of the first `rows` are a list of their own (QuickJump's
    /// pinned windows): they are drawn as one two-column grid, then a
    /// hairline, then the other rows as a second grid. 0 for one grid.
    pub pins: usize,
}

impl Content {
    /// The keycaps of a shortcut's text: "Win+Alt+K" gives Win, Alt, K.
    pub fn of_keys(keys: &str, rows: Vec<Row>) -> Self {
        Self {
            keys: crate::ui::widgets::keycap::split(keys)
                .into_iter()
                .map(str::to_string)
                .collect(),
            rows,
            pins: 0,
        }
    }
}

/// How a chord ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// A key ran a command; `text` says what it did.
    Matched { key: String, text: String },
    /// Nothing is assigned to the key; it was kept from the app all the
    /// same.
    Unassigned { key: String },
    /// The key is assigned, but its command could not do it: "Quick
    /// access is full".
    Refused { key: String, text: String },
    /// The time ran out, or Esc.
    Cancelled,
}

/// The colour of a closing state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Danger,
    Quiet,
}

impl End {
    /// How long it stays before it fades.
    pub fn stays_ms(&self) -> u64 {
        match self {
            End::Matched { .. } => MATCHED_MS,
            End::Unassigned { .. } | End::Refused { .. } => UNASSIGNED_MS,
            End::Cancelled => CANCELLED_MS,
        }
    }

    /// The key it adds to the keys pressed so far.
    pub fn key(&self) -> Option<&str> {
        match self {
            End::Matched { key, .. } | End::Unassigned { key } | End::Refused { key, .. } => {
                Some(key)
            }
            End::Cancelled => None,
        }
    }

    pub fn text(&self) -> String {
        match self {
            End::Matched { text, .. } | End::Refused { text, .. } => text.clone(),
            End::Unassigned { key } => format!("Nothing assigned to {key}"),
            End::Cancelled => "Cancelled".to_string(),
        }
    }

    pub fn tone(&self) -> Tone {
        match self {
            End::Matched { .. } => Tone::Accent,
            End::Unassigned { .. } | End::Refused { .. } => Tone::Danger,
            End::Cancelled => Tone::Quiet,
        }
    }

    /// A chord that ran or was cancelled before the tip showed leaves no
    /// trace; one that went wrong shows why at once.
    fn shows_when_fast(&self) -> bool {
        matches!(self, End::Unassigned { .. } | End::Refused { .. })
    }
}

/// What is on screen at a moment.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    /// Not yet, or not at all.
    Hidden,
    /// Waiting, with `left` of the time still to go (1 at the start).
    Waiting { left: f32 },
    /// Showing how it ended, at this strength.
    Closing { alpha: f32 },
    /// Over: the tip goes.
    Done,
}

/// The tip's life in milliseconds from the first key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timeline {
    wait_ms: u64,
    ended: Option<(u64, End)>,
}

impl Timeline {
    pub fn new(wait_ms: u64) -> Self {
        Self {
            wait_ms: wait_ms.max(1),
            ended: None,
        }
    }

    /// The chord ended at `at` ms. Only the first end counts.
    pub fn end(&mut self, at: u64, end: End) {
        if self.ended.is_none() {
            self.ended = Some((at, end));
        }
    }

    pub fn ending(&self) -> Option<&End> {
        self.ended.as_ref().map(|(_, end)| end)
    }

    pub fn frame(&self, now: u64) -> Frame {
        match &self.ended {
            None if now < SHOW_AFTER_MS => Frame::Hidden,
            // The owner always ends the wait; should it miss that, the tip
            // still goes a little after the wait's time.
            None if now > self.wait_ms + LATE_MS => Frame::Done,
            None => Frame::Waiting {
                left: 1.0 - (now as f32 / self.wait_ms as f32).min(1.0),
            },
            Some((at, end)) => {
                if *at < SHOW_AFTER_MS && !end.shows_when_fast() {
                    return Frame::Done;
                }
                let since = now.saturating_sub(*at);
                let stays = end.stays_ms();
                if since < stays {
                    Frame::Closing { alpha: 1.0 }
                } else if since < stays + FADE_MS {
                    Frame::Closing {
                        alpha: 1.0 - (since - stays) as f32 / FADE_MS as f32,
                    }
                } else {
                    Frame::Done
                }
            }
        }
    }
}

/// How one of the three waiting dots looks `ms` after the wait began:
/// its strength and how far it is lifted (0 to 1). Each dot rises and
/// falls once every 1.2 s, the next one 150 ms later.
pub fn dot(ms: u64, index: usize) -> (f32, f32) {
    let cycle = 1200.0;
    let t = ((ms as f32 - 150.0 * index as f32).rem_euclid(cycle)) / cycle;
    let lift = if t < 0.4 {
        t / 0.4
    } else if t < 0.8 {
        1.0 - (t - 0.4) / 0.4
    } else {
        0.0
    };
    (0.25 + 0.75 * lift, lift)
}

/// The name of a key pressed while a chord waits, leaving out the
/// modifiers still held from its first press: "Alt+X" after Win+`.
pub fn key_label(stroke: Stroke, held: u32) -> String {
    let all = MOD_WIN | MOD_ALT | MOD_CONTROL | MOD_SHIFT;
    hotkeys::format(Hotkey {
        modifiers: stroke.modifiers & all & !held,
        vk: stroke.vk,
    })
}

// Sizes in logical pixels, from the design (option 5b).
pub(crate) const WIDTH: f32 = 620.0;
pub(crate) const HEADER: f32 = 58.0;
pub(crate) const LIST_TOP: f32 = 10.0;
pub(crate) const LIST_BOTTOM: f32 = 14.0;
pub(crate) const ROW: f32 = 34.0;
pub(crate) const BAR: f32 = 3.0;
/// Between the pin grid and the command grid: 6 px, the hairline, 6 px.
pub(crate) const GROUP_GAP: f32 = 13.0;

/// The lines of rows a group of `rows` takes in two columns.
pub(crate) fn lines(rows: usize) -> usize {
    rows.div_ceil(2)
}

/// The panel's size without its shadow: the list only while it waits.
/// `pins` rows are a grid of their own, then `commands` rows another.
pub fn panel_size(pins: usize, commands: usize, list: bool) -> (f32, f32) {
    let all = lines(pins) + lines(commands);
    let list = if list && all > 0 {
        let between = if pins > 0 && commands > 0 {
            GROUP_GAP
        } else {
            0.0
        };
        1.0 + LIST_TOP + all as f32 * ROW + between + LIST_BOTTOM
    } else {
        0.0
    };
    (WIDTH, HEADER + list + BAR)
}

#[cfg(test)]
mod tests;
