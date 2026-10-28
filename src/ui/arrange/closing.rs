//! The strip's side of closing a window: the card shrinks while the app
//! answers, goes (and the cards after it slide) when the window is gone, or
//! comes back with a note when the window is still open (see `slide`).

use super::*;

impl Shared {
    /// A card's size and opacity now; see `card_motion`.
    pub(super) fn motion(&self, hwnd: isize) -> (f32, f32) {
        let now = Instant::now();
        let since = |at: Instant| now.saturating_duration_since(at);
        let left = shrink_age(
            self.leaving
                .iter()
                .find(|gone| gone.window.hwnd == hwnd)
                .map(|gone| since(gone.since)),
            self.closing.get(&hwnd).map(|at| since(*at)),
            self.animate,
        );
        card_motion(
            self.appeared.get(&hwnd).map(|at| since(*at)),
            left,
            self.waiting.get(&hwnd).map(|wait| since(wait.since)),
        )
    }

    /// The window was asked to close or is gone: its card takes no clicks.
    pub(super) fn is_leaving(&self, hwnd: isize) -> bool {
        self.is_gone(hwnd) || self.closing.contains_key(&hwnd)
    }

    /// The window is gone, and its card is on its way out.
    pub(super) fn is_gone(&self, hwnd: isize) -> bool {
        self.leaving.iter().any(|gone| gone.window.hwnd == hwnd)
    }

    /// A click or Ctrl+W asked the window to close.
    pub(super) fn ask_close(&mut self, hwnd: isize) {
        self.waiting.remove(&hwnd);
        self.closing.insert(hwnd, Instant::now());
        crate::detailed!("deck: asked window {hwnd:#x} to close");
    }

    /// The note on a card whose app has not closed the window.
    pub(super) fn waiting_note(&self, hwnd: isize) -> Option<String> {
        let wait = self.waiting.get(&hwnd)?;
        Some(slide::waiting_text(&wait.app))
    }

    /// How long until a window that was asked to close needs a look.
    pub(super) fn next_check(&self) -> Option<Duration> {
        let now = Instant::now();
        self.closing
            .values()
            .map(|at| CLOSE_WAIT.saturating_sub(now.saturating_duration_since(*at)))
            .min()
    }

    /// Where a card is drawn: the place the layout gave it, or a place on
    /// its way there while it slides into a gap.
    pub(super) fn slid(&mut self, key: slide::SlideKey, target: Rect) -> Rect {
        self.slides.place(key, target, Instant::now(), self.animate)
    }

    /// Ends the motions that are over, and the windows that have faded out.
    /// True while something still moves.
    pub(super) fn settle_motion(&mut self) -> bool {
        let now = Instant::now();
        self.animate = slide::animations_on();
        self.slides.next_frame();
        let over = |since: Instant| now.saturating_duration_since(since) >= CARD_MOTION;
        let shrink = if self.animate {
            CLOSE_TIME
        } else {
            Duration::ZERO
        };
        let done: Vec<isize> = self
            .leaving
            .iter()
            .filter(|gone| now.saturating_duration_since(gone.since) >= shrink)
            .map(|gone| gone.window.hwnd)
            .collect();
        if !done.is_empty() {
            self.leaving
                .retain(|gone| !done.contains(&gone.window.hwnd));
            if let Some(snapshot) = self.snapshot.as_mut() {
                for group in &mut snapshot.groups {
                    group.windows.retain(|w| !done.contains(&w.hwnd));
                }
            }
            self.slides.open_gap(&done);
        }
        self.appeared.retain(|_, at| !over(*at));
        self.check_closing(now);
        let shrinking = |at: &Instant| now.saturating_duration_since(*at) < CLOSE_TIME;
        !done.is_empty()
            || !self.leaving.is_empty()
            || !self.appeared.is_empty()
            || self.slides.running()
            || self.closing.values().any(shrinking)
            || self.waiting.values().any(|wait| shrinking(&wait.since))
    }

    /// The group's name if the window is in the data and not on its way out.
    fn listed_in(&self, hwnd: isize) -> Option<String> {
        if self.is_gone(hwnd) {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        let group = snapshot
            .groups
            .iter()
            .find(|group| group.windows.iter().any(|w| w.hwnd == hwnd))?;
        Some(group.label.clone())
    }

    /// Decides what became of the windows that were asked to close. Still
    /// open after `CLOSE_WAIT` means the app is waiting, so the card comes
    /// back with a note. A window that is gone has left already. A card that
    /// waits goes when its window does.
    fn check_closing(&mut self, now: Instant) {
        let asked: Vec<(isize, Instant)> = self.closing.iter().map(|(h, at)| (*h, *at)).collect();
        for (hwnd, at) in asked {
            if self.is_gone(hwnd) {
                self.closing.remove(&hwnd);
                continue;
            }
            let app = self.listed_in(hwnd);
            let elapsed = now.saturating_duration_since(at);
            if slide::come_back(elapsed, app.is_some()) {
                let app = app.unwrap_or_default();
                log::info!("WindowDeck: window {hwnd:#x} ({app}) is still open after a close");
                crate::detailed!("deck: window {hwnd:#x} still open {elapsed:?} after the close; its card is back, waiting for {app}");
                self.closing.remove(&hwnd);
                self.waiting.insert(hwnd, Waiting { since: now, app });
            } else if elapsed >= CLOSE_WAIT {
                self.closing.remove(&hwnd);
            }
        }
        let open: Vec<isize> = self
            .waiting
            .keys()
            .copied()
            .filter(|hwnd| self.listed_in(*hwnd).is_some())
            .collect();
        self.waiting.retain(|hwnd, _| open.contains(hwnd));
    }

    /// The deck closed or opened afresh: no card is closing or waiting.
    pub(super) fn forget_closing(&mut self) {
        self.closing.clear();
        self.waiting.clear();
        self.slides.clear();
    }
}
