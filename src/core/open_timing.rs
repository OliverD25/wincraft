//! How long each opening of the WindowDeck strip takes, step by step.
//!
//! One opening writes one info line to the log. The steps happen on three
//! threads (the keyboard hook, the host and the palette's UI thread), so the
//! opening in progress lives in one shared slot. `Timing` is the plain part:
//! it keeps the marks and writes the line.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A key stamp older than this belongs to no opening.
const KEY_STAMP_FRESH: Duration = Duration::from_secs(2);

/// One step of an opening, in the order they normally happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenStep {
    /// The host took the request from the keyboard hook.
    Host,
    /// The taskbar buttons were read.
    TaskbarRead,
    /// The host sent the strip data to the UI thread.
    Request,
    /// The UI thread received that data.
    UiGot,
    /// The strip hidden OS window was found.
    WindowMade,
    /// The strip drew its first frame.
    FirstFrame,
    /// The last window picture (DWM thumbnail) was registered.
    Pictures,
    /// The strip was uncloaked, so the user sees it.
    OnScreen,
}

impl OpenStep {
    fn words(self) -> &'static str {
        match self {
            OpenStep::Host => "host",
            OpenStep::TaskbarRead => "taskbar read",
            OpenStep::Request => "request",
            OpenStep::UiGot => "UI got it",
            OpenStep::WindowMade => "window made",
            OpenStep::FirstFrame => "first frame",
            OpenStep::Pictures => "pictures",
            OpenStep::OnScreen => "on screen",
        }
    }
}

/// The marks of one opening. Every mark is milliseconds since the opening
/// first moment.
#[derive(Debug)]
pub struct Timing {
    label: String,
    new_window: Option<bool>,
    marks: Vec<(OpenStep, u64)>,
}

impl Timing {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            new_window: None,
            marks: Vec::new(),
        }
    }

    /// Keeps the first time a step happened; a second mark is ignored.
    pub fn mark(&mut self, step: OpenStep, ms: u64) {
        if !self.marks.iter().any(|(s, _)| *s == step) {
            self.marks.push((step, ms));
        }
    }

    /// Keeps the last time a step happened, for a step that repeats.
    pub fn mark_latest(&mut self, step: OpenStep, ms: u64) {
        match self.marks.iter_mut().find(|(s, _)| *s == step) {
            Some(mark) => mark.1 = ms,
            None => self.marks.push((step, ms)),
        }
    }

    /// Whether the strip needed a new OS window or found its old one.
    pub fn set_new_window(&mut self, new: bool) {
        self.new_window = Some(new);
    }

    /// The log line. Steps that did not happen are left out.
    pub fn line(&self) -> String {
        let window = match self.new_window {
            Some(true) => ", new window",
            Some(false) => ", reused window",
            None => "",
        };
        let mut marks = self.marks.clone();
        marks.sort_by_key(|(_, ms)| *ms);
        let steps: Vec<String> = marks
            .iter()
            .map(|(step, ms)| format!("{} {ms} ms", step.words()))
            .collect();
        format!(
            "WindowDeck open timing ({}{window}): {}",
            self.label,
            steps.join(" \u{b7} ")
        )
    }
}

struct Active {
    timing: Timing,
    start: Instant,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// Microseconds after `EPOCH`, plus one, when a key was last seen; 0 for none.
static KEY_STAMP: AtomicU64 = AtomicU64::new(0);

/// The keyboard hook saw the key of an opening. Allocates nothing.
pub fn key_seen() {
    let epoch = *EPOCH.get_or_init(Instant::now);
    let micros = epoch.elapsed().as_micros() as u64;
    KEY_STAMP.store(micros.saturating_add(1), Ordering::Relaxed);
}

/// The host starts an opening. With `from_key` the first moment is the
/// hook stamp (when it is fresh) and the host arrival is marked.
pub fn begin(label: &str, from_key: bool) {
    let now = Instant::now();
    let mut start = now;
    if from_key {
        let stamp = KEY_STAMP.swap(0, Ordering::Relaxed);
        if let (Some(epoch), true) = (EPOCH.get(), stamp > 0) {
            let seen = *epoch + Duration::from_micros(stamp - 1);
            if now.saturating_duration_since(seen) < KEY_STAMP_FRESH {
                start = seen;
            }
        }
    }
    let mut timing = Timing::new(label);
    if from_key {
        timing.mark(OpenStep::Host, ms_since(start, now));
    }
    if let Ok(mut active) = ACTIVE.lock() {
        *active = Some(Active { timing, start });
    }
}

fn ms_since(start: Instant, now: Instant) -> u64 {
    now.saturating_duration_since(start).as_millis() as u64
}

/// Marks a step of the opening in progress, if there is one.
pub fn mark(step: OpenStep) {
    with_active(|active, ms| active.timing.mark(step, ms));
}

/// Like `mark`, for a step that repeats: the last time is kept.
pub fn mark_latest(step: OpenStep) {
    with_active(|active, ms| active.timing.mark_latest(step, ms));
}

/// Says whether the strip made a new OS window for this opening.
pub fn set_new_window(new: bool) {
    with_active(|active, _| active.timing.set_new_window(new));
}

/// Whether an opening is being timed.
pub fn is_active() -> bool {
    ACTIVE.lock().is_ok_and(|active| active.is_some())
}

/// The opening ended without a strip (Windows keeps the key).
pub fn cancel() {
    if let Ok(mut active) = ACTIVE.lock() {
        *active = None;
    }
}

/// The strip is on screen: writes the line and ends the opening. False
/// when no opening was being timed.
pub fn finish() -> bool {
    let Ok(mut slot) = ACTIVE.lock() else {
        return false;
    };
    let Some(mut active) = slot.take() else {
        return false;
    };
    let ms = ms_since(active.start, Instant::now());
    active.timing.mark(OpenStep::OnScreen, ms);
    log::info!("{}", active.timing.line());
    true
}

fn with_active(change: impl FnOnce(&mut Active, u64)) {
    if let Ok(mut slot) = ACTIVE.lock() {
        if let Some(active) = slot.as_mut() {
            let ms = ms_since(active.start, Instant::now());
            change(active, ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_every_step_in_order() {
        let mut t = Timing::new("Win+2");
        t.set_new_window(true);
        t.mark(OpenStep::Host, 3);
        t.mark(OpenStep::TaskbarRead, 41);
        t.mark(OpenStep::Request, 46);
        t.mark(OpenStep::UiGot, 48);
        t.mark(OpenStep::WindowMade, 205);
        t.mark(OpenStep::FirstFrame, 231);
        t.mark(OpenStep::Pictures, 236);
        t.mark(OpenStep::OnScreen, 268);
        assert_eq!(
            t.line(),
            "WindowDeck open timing (Win+2, new window): host 3 ms \u{b7} taskbar read 41 ms \u{b7} request 46 ms \u{b7} UI got it 48 ms \u{b7} window made 205 ms \u{b7} first frame 231 ms \u{b7} pictures 236 ms \u{b7} on screen 268 ms"
        );
    }

    #[test]
    fn steps_that_did_not_happen_are_left_out() {
        let mut t = Timing::new("hover");
        t.mark(OpenStep::Request, 5);
        t.mark(OpenStep::OnScreen, 60);
        assert_eq!(
            t.line(),
            "WindowDeck open timing (hover): request 5 ms \u{b7} on screen 60 ms"
        );
    }

    #[test]
    fn marks_are_written_by_time_whatever_order_they_came_in() {
        let mut t = Timing::new("x");
        t.mark(OpenStep::UiGot, 20);
        t.mark(OpenStep::Request, 10);
        assert_eq!(
            t.line(),
            "WindowDeck open timing (x): request 10 ms \u{b7} UI got it 20 ms"
        );
    }

    #[test]
    fn a_step_keeps_its_first_time_unless_it_is_marked_latest() {
        let mut t = Timing::new("x");
        t.mark(OpenStep::Pictures, 10);
        t.mark(OpenStep::Pictures, 30);
        assert!(t.line().ends_with("pictures 10 ms"));
        t.mark_latest(OpenStep::Pictures, 30);
        assert!(t.line().ends_with("pictures 30 ms"));
    }

    #[test]
    fn the_window_flag_says_new_or_reused() {
        let mut t = Timing::new("Win+1");
        assert!(t.line().contains("(Win+1)"));
        t.set_new_window(false);
        assert!(t.line().contains("(Win+1, reused window)"));
        t.set_new_window(true);
        assert!(t.line().contains("(Win+1, new window)"));
    }
}
