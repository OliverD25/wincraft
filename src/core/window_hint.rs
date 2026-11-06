//! Points to a window that came up on another monitor: a short glow in the
//! accent colour around the window, and a small pill on the monitor the
//! user was looking at, with an arrow to the other one ("Telegram · Right
//! monitor →"). Both are click-through, never take the keyboard or the
//! mouse, and never move or change any window (Principle 1). They fade in,
//! hold and fade out in about a second, or show and vanish at once when
//! the Windows setting "Animation effects" is off. A new hint replaces one
//! still on screen.
//!
//! QuickJump shows it after a jump; WindowDeck or the palette's window
//! switch can call `show` the same way (Principle 3: a host helper).
//!
//! This file holds what is decided without Windows (which monitor, the
//! arrow, the spots, the fades, the text), so it is tested; `screen` puts
//! it on screen and `draw` paints it.

mod draw;
mod screen;

pub use screen::{hide, show, show_demo};

use crate::core::monitors::Rect;

/// The glow's line and its soft outer part, and the window corners it
/// follows, in logical pixels.
pub const LINE: f32 = 3.0;
pub const GLOW: f32 = 12.0;
pub const CORNER: f32 = 8.0;
pub const FADE_IN_MS: u64 = 120;
pub const FADE_OUT_MS: u64 = 300;
pub const GLOW_HOLD_MS: u64 = 400;
pub const PILL_HOLD_MS: u64 = 1200;
/// A longer window name is cut with "…".
pub const NAME_CHARS: usize = 40;
/// Between the pill and the edge of the work area, logical pixels.
pub const EDGE_GAP: f32 = 16.0;

/// Where the user was looking before the window came up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Before {
    /// The window that was in front; 0 when there was none.
    Window(isize),
    /// A WinCraft window that closed for the jump, such as Quick access:
    /// where it was on the screen.
    Area(Rect),
}

/// A window to point to, and what to call it in the pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub window: isize,
    pub name: String,
    pub from: Before,
}

/// Where the other monitor is, seen from the one the user looks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    pub fn arrow(self) -> &'static str {
        match self {
            Dir::Left => "\u{2190}",
            Dir::Right => "\u{2192}",
            Dir::Up => "\u{2191}",
            Dir::Down => "\u{2193}",
        }
    }
}

/// The monitor the user looks at, by its place in the monitor list: the
/// one of a WinCraft window that closed for the jump, else the one of the
/// window in front, else the one with the mouse.
pub fn looking_at(
    closed: Option<usize>,
    front: Option<usize>,
    pointer: Option<usize>,
) -> Option<usize> {
    closed.or(front).or(pointer)
}

/// The desktop is in front: there is no window the user looks at.
pub fn is_desktop(class: &str) -> bool {
    matches!(class, "Progman" | "WorkerW")
}

/// Only a window on another monitor gets a hint.
pub fn another_monitor(looking: Option<usize>, target: Option<usize>) -> bool {
    matches!((looking, target), (Some(a), Some(b)) if a != b)
}

pub fn centre(rect: Rect) -> (i32, i32) {
    ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
}

/// Where monitor `to` is from monitor `from`, by their centres. A tie
/// between across and up or down counts as across.
pub fn direction(from: Rect, to: Rect) -> Dir {
    let (from_x, from_y) = centre(from);
    let (to_x, to_y) = centre(to);
    let (dx, dy) = (to_x - from_x, to_y - from_y);
    if dx.abs() >= dy.abs() {
        if dx >= 0 {
            Dir::Right
        } else {
            Dir::Left
        }
    } else if dy > 0 {
        Dir::Down
    } else {
        Dir::Up
    }
}

/// The top left of a pill `size` big on `work`, at the edge that faces
/// `dir`, in line with the middle of `target` and kept `gap` inside the
/// work area.
pub fn pill_spot(work: Rect, dir: Dir, target: Rect, size: (i32, i32), gap: i32) -> (i32, i32) {
    let (mid_x, mid_y) = centre(target);
    let keep = |value: i32, low: i32, high: i32| value.min(high).max(low);
    let y = keep(mid_y - size.1 / 2, work[1] + gap, work[3] - gap - size.1);
    let x = keep(mid_x - size.0 / 2, work[0] + gap, work[2] - gap - size.0);
    match dir {
        Dir::Left => (work[0] + gap, y),
        Dir::Right => (work[2] - gap - size.0, y),
        Dir::Up => (x, work[1] + gap),
        Dir::Down => (x, work[3] - gap - size.1),
    }
}

/// The box the glow goes round, and whether it is drawn inside the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ring {
    pub frame: Rect,
    pub inside: bool,
}

/// The glow for a window's visible frame on a monitor's work area. A
/// maximised window, or one that covers the work area, has nothing around
/// it on its monitor, so the glow runs along the inside of the work area.
pub fn ring(frame: Rect, work: Rect, maximised: bool) -> Ring {
    let covers =
        frame[0] <= work[0] && frame[1] <= work[1] && frame[2] >= work[2] && frame[3] >= work[3];
    if maximised || covers {
        Ring {
            frame: work,
            inside: true,
        }
    } else {
        Ring {
            frame,
            inside: false,
        }
    }
}

/// The glow window's box: the frame and `reach` around it, or the frame
/// itself when the glow is inside.
pub fn ring_box(ring: Ring, reach: i32) -> Rect {
    if ring.inside {
        return ring.frame;
    }
    let [left, top, right, bottom] = ring.frame;
    [left - reach, top - reach, right + reach, bottom + reach]
}

/// How strong a part is `ms` after it began, holding `hold` ms; None when
/// it is over. Without animations it shows fully for the hold, then goes.
pub fn strength(ms: u64, hold: u64, animate: bool) -> Option<f32> {
    if !animate {
        return (ms < hold).then_some(1.0);
    }
    if ms < FADE_IN_MS {
        Some(ms as f32 / FADE_IN_MS as f32)
    } else if ms < FADE_IN_MS + hold {
        Some(1.0)
    } else if ms < FADE_IN_MS + hold + FADE_OUT_MS {
        Some(1.0 - (ms - FADE_IN_MS - hold) as f32 / FADE_OUT_MS as f32)
    } else {
        None
    }
}

/// `text` cut to `max` characters, the last one "…".
pub fn cut(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}\u{2026}", kept.trim_end())
}

/// The pill's words without the arrow, in two parts: the name, and the
/// place after it (" · Right monitor"). The pill draws the place in a
/// quieter colour.
pub fn pill_parts(name: &str, monitor: Option<&str>) -> (String, String) {
    let place = match monitor {
        Some(word) if word.starts_with("Monitor") => word.to_string(),
        Some(word) => format!("{word} monitor"),
        None => "Other monitor".to_string(),
    };
    let name = cut(name, NAME_CHARS);
    if name.is_empty() {
        (String::new(), place)
    } else {
        (name, format!(" \u{b7} {place}"))
    }
}

/// The pill's words without the arrow: "Telegram · Right monitor".
pub fn pill_words(name: &str, monitor: Option<&str>) -> String {
    let (name, place) = pill_parts(name, monitor);
    format!("{name}{place}")
}

/// The arrow goes on the side of the other monitor: first for the left,
/// last otherwise.
pub fn arrow_first(dir: Dir) -> bool {
    dir == Dir::Left
}

/// The whole line, as the pill shows it and the log records it.
pub fn pill_text(name: &str, monitor: Option<&str>, dir: Dir) -> String {
    let words = pill_words(name, monitor);
    if arrow_first(dir) {
        format!("{} {words}", dir.arrow())
    } else {
        format!("{words} {}", dir.arrow())
    }
}

#[cfg(test)]
mod tests;
