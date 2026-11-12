//! Fading ClaudeDeck's window when the mouse is away (design section 8):
//! after `AWAY` without the mouse it goes to half see-through in a short
//! step, and it is solid again when the mouse comes back, when it has the
//! keyboard, or when a chat starts to wait. Only ClaudeDeck's own window
//! changes. When it is faded and still, nothing repaints.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA,
    WS_EX_LAYERED,
};

/// How see-through the faded window is: the design's opacity.
pub const FADED: f32 = 0.5;
/// How long the mouse is away before the window fades.
pub const AWAY: Duration = Duration::from_secs(3);
/// How long the step from solid to faded (or back) takes.
pub const STEP: Duration = Duration::from_millis(200);
/// A frame while the step runs.
const FRAME: Duration = Duration::from_millis(16);

/// What the window saw this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Seen {
    /// The setting "Fade when the mouse is away".
    pub on: bool,
    pub mouse_in: bool,
    pub focused: bool,
    /// A chat started to wait since the last frame.
    pub new_wait: bool,
}

#[derive(Debug)]
pub struct Fade {
    /// 1 is solid.
    pub alpha: f32,
    away_since: Option<Instant>,
    /// A chat started to wait while the mouse was away: solid until the
    /// mouse comes back or the window gets the keyboard.
    alert: bool,
    last: Option<Instant>,
    /// The alpha last given to the window, 255 when it is not layered.
    applied: u8,
}

impl Default for Fade {
    fn default() -> Self {
        Fade {
            alpha: 1.0,
            away_since: None,
            alert: false,
            last: None,
            applied: u8::MAX,
        }
    }
}

impl Fade {
    /// Where the window should be: solid or faded.
    fn target(&self, seen: Seen, now: Instant) -> f32 {
        let away_long = self
            .away_since
            .is_some_and(|since| now.duration_since(since) >= AWAY);
        if seen.on && away_long && !self.alert {
            FADED
        } else {
            1.0
        }
    }

    /// One frame: moves the alpha towards where it should be, and says
    /// when the fade needs the next frame (None: it needs none).
    pub fn step(&mut self, seen: Seen, now: Instant) -> Option<Duration> {
        // A frame after a long quiet time counts as one frame, so the step
        // always shows.
        let elapsed = self
            .last
            .map_or(Duration::ZERO, |last| now.duration_since(last))
            .min(FRAME * 2);
        self.last = Some(now);
        if seen.new_wait {
            self.alert = true;
        }
        if seen.mouse_in || seen.focused {
            self.alert = false;
            self.away_since = None;
        } else if self.away_since.is_none() {
            self.away_since = Some(now);
        }
        let target = self.target(seen, now);
        let room = 1.0 - FADED;
        let by = room * (elapsed.as_secs_f32() / STEP.as_secs_f32()).min(1.0);
        self.alpha = if self.alpha < target {
            (self.alpha + by).min(target)
        } else {
            (self.alpha - by).max(target)
        };
        if (self.alpha - target).abs() > f32::EPSILON {
            return Some(FRAME);
        }
        match self.away_since {
            Some(since) if seen.on && !self.alert && target >= 1.0 => {
                Some(AWAY.saturating_sub(now.duration_since(since)))
            }
            _ => None,
        }
    }

    /// Gives ClaudeDeck's own window the alpha, when it changed. Solid,
    /// the window is not layered at all.
    pub fn apply(&mut self, hwnd: isize) {
        let byte = (self.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        if hwnd == 0 || byte == self.applied {
            return;
        }
        self.applied = byte;
        let handle = hwnd as HWND;
        unsafe {
            let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
            let layered = WS_EX_LAYERED as isize;
            if byte == u8::MAX {
                SetWindowLongPtrW(handle, GWL_EXSTYLE, style & !layered);
            } else {
                if style & layered == 0 {
                    SetWindowLongPtrW(handle, GWL_EXSTYLE, style | layered);
                }
                SetLayeredWindowAttributes(handle, 0, byte, LWA_ALPHA);
            }
        }
    }
}
