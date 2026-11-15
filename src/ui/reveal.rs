//! Shows one of WinCraft's own egui windows only once it has drawn itself.
//! Windows paints a new window, or one shown again after `SW_HIDE`, white
//! until the app's next frame is drawn. So the window goes on screen
//! covered (cloaked), draws one frame there, and is uncovered on the frame
//! after that. A limit uncovers it anyway, so a missed frame never leaves
//! it invisible.
//!
//! A hidden viewport does not draw, so the step that shows the window runs
//! on a frame that does: the main (root) frame for a deferred viewport, the
//! parent's frame for an immediate one.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, ShowWindow, SHOW_WINDOW_CMD};

use crate::ui::dwm_thumbs;
use crate::ui::settings;

/// How far a window is on its way to the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reveal {
    /// Not on screen yet: it is shown once it is found and in its place.
    #[default]
    Hidden,
    /// On screen but covered, waiting for its first frame.
    Covered { since: Instant },
    /// One frame was drawn into the covered window.
    Drawn,
    /// On screen and uncovered.
    Shown,
}

/// Uncovers the window anyway after this long.
pub const COVERED_LONGEST: Duration = Duration::from_millis(400);

/// What the caller does now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Not done yet: draw again soon.
    Again,
    /// Put the window in its place and show it covered (`show_covered`).
    Show,
    /// It has drawn: uncover it.
    Uncover,
    /// It did not draw in time: uncover it anyway and say so in the log.
    UncoverLate,
}

impl Reveal {
    /// The step of a frame that runs while the window may be hidden.
    /// `found` says whether the window's handle is known.
    pub fn on_main_frame(&mut self, found: bool, now: Instant) -> Step {
        match *self {
            Reveal::Hidden if !found => Step::Again,
            Reveal::Hidden => {
                *self = Reveal::Covered { since: now };
                Step::Show
            }
            Reveal::Covered { since }
                if now.saturating_duration_since(since) >= COVERED_LONGEST =>
            {
                *self = Reveal::Shown;
                Step::UncoverLate
            }
            Reveal::Covered { .. } | Reveal::Drawn => Step::Again,
            Reveal::Shown => Step::Nothing,
        }
    }

    /// The step of a frame of the window itself.
    pub fn on_window_frame(&mut self) -> Step {
        match *self {
            Reveal::Covered { .. } => {
                *self = Reveal::Drawn;
                Step::Again
            }
            Reveal::Drawn => {
                *self = Reveal::Shown;
                Step::Uncover
            }
            Reveal::Hidden | Reveal::Shown => Step::Nothing,
        }
    }

    /// The window was covered again while on screen (`cover`): it is
    /// uncovered after its next frame, within the same limit.
    pub fn covered_again(&mut self, now: Instant) {
        if !matches!(self, Reveal::Covered { .. }) {
            *self = Reveal::Covered { since: now };
        }
    }
}

/// Covers the window, then shows it with `how` (for example `SW_SHOW`).
pub fn show_covered(window: isize, how: SHOW_WINDOW_CMD) {
    cover(window);
    unsafe { ShowWindow(window as HWND, how) };
}

pub fn cover(window: isize) {
    dwm_thumbs::cloak(window as HWND, true);
}

pub fn uncover(window: isize) {
    dwm_thumbs::cloak(window as HWND, false);
}

/// The same for an immediate viewport, which egui makes and draws in one
/// call. Its builder says `.with_visible(false)`; `after_call` runs right
/// after each `show_viewport_immediate`, on the parent's frame.
#[derive(Default)]
pub struct Immediate {
    reveal: Reveal,
    window: isize,
    /// The parent's pass of the last call.
    pass: Option<u64>,
}

impl Immediate {
    /// `ctx` is the parent's; `how` shows the window (`SW_SHOWNA` for one
    /// that must not take the keyboard).
    pub fn after_call(
        &mut self,
        ctx: &egui::Context,
        id: egui::ViewportId,
        title: &str,
        how: SHOW_WINDOW_CMD,
    ) {
        let pass = ctx.cumulative_pass_nr();
        let gone = self.window != 0 && unsafe { IsWindow(self.window as HWND) } == 0;
        if fresh_window(self.pass, pass) || gone {
            *self = Immediate::default();
        }
        self.pass = Some(pass);
        if self.window == 0 {
            self.window = settings::find_window(title, 0);
        }
        match self.reveal.on_main_frame(self.window != 0, Instant::now()) {
            Step::Show => {
                show_covered(self.window, how);
                // winit hides a window it thinks is hidden whenever one of
                // its flags changes, so it must know this one is shown.
                ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(true));
                ctx.request_repaint();
                // This call drew into the hidden window; the next one counts.
                return;
            }
            Step::UncoverLate => {
                uncover(self.window);
                log::warn!("\"{title}\" was shown before it had drawn itself");
                return;
            }
            Step::Again => ctx.request_repaint(),
            Step::Nothing | Step::Uncover => {}
        }
        match self.reveal.on_window_frame() {
            Step::Uncover => uncover(self.window),
            Step::Again => ctx.request_repaint(),
            _ => {}
        }
    }
}

/// egui closes an immediate viewport in any pass of its parent that did
/// not show it, so a gap in the parent's passes means a new window.
fn fresh_window(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|last| last.wrapping_add(1) != now)
}

#[cfg(test)]
mod tests;
