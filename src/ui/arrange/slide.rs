//! What happens to a card when its window is closed from the strip: the
//! card shrinks and fades, the cards after it slide into the gap, and a card
//! whose window refuses to close comes back with a note. The curves and
//! rules are plain functions; `Slides` remembers where cards were drawn.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Rect, Vec2};

/// How long a closed card takes to shrink and fade, or to grow back.
pub const CLOSE_TIME: Duration = Duration::from_millis(180);
/// A closed card ends this part of its size.
pub const CLOSE_SCALE: f32 = 0.85;
/// How long the cards after a closed one take to slide into the gap.
pub const SLIDE_TIME: Duration = Duration::from_millis(200);
/// A window still open this long after it was asked to close is waiting for
/// its app (a "Save changes?" question, or a refusal).
pub const CLOSE_WAIT: Duration = Duration::from_millis(1500);

/// A card in the strip: its window, and whether it is that window's ghost
/// in the By monitor view.
pub type SlideKey = (isize, bool);

/// How far a motion that lasts `total` is after `elapsed`, from 0 to 1,
/// slowing down at the end (ease-out cubic).
pub fn ease_out(elapsed: Duration, total: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// A card whose window was asked to close, `elapsed` after the ask: its size
/// as a part of the full size, and its opacity.
pub fn close_curve(elapsed: Duration) -> (f32, f32) {
    let progress = ease_out(elapsed, CLOSE_TIME);
    (1.0 - (1.0 - CLOSE_SCALE) * progress, 1.0 - progress)
}

/// The same motion backwards, for a card whose window did not close.
pub fn regrow_curve(elapsed: Duration) -> (f32, f32) {
    let progress = ease_out(elapsed, CLOSE_TIME);
    (CLOSE_SCALE + (1.0 - CLOSE_SCALE) * progress, progress)
}

/// How far a card sliding from `from` to `to` is from `to`, `elapsed` after
/// it started. Exactly zero once the slide is over.
pub fn slide_offset(from: Rect, to: Rect, elapsed: Duration) -> Vec2 {
    if elapsed >= SLIDE_TIME {
        return Vec2::ZERO;
    }
    (from.min - to.min) * (1.0 - ease_out(elapsed, SLIDE_TIME))
}

/// Whether a card that was asked to close comes back: its window is still in
/// the host's data `asked` after the ask.
pub fn come_back(asked: Duration, still_listed: bool) -> bool {
    asked >= CLOSE_WAIT && still_listed
}

/// The note on a card that waits for its app.
pub fn waiting_text(app: &str) -> String {
    format!("Waiting for {app}")
}

/// Whether the user's Windows setting "Animation effects" is on. When it is
/// off, cards change at once.
pub fn animations_on() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut on: i32 = 1;
    // SAFETY: the call writes one BOOL into `on`.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut on as *mut i32).cast(),
            0,
        )
    };
    read == 0 || on != 0
}

/// Where `key` was drawn last frame, when it must slide to `target`: it
/// comes after the first card of a window in `closed` in `last` (the cards
/// of last frame, in drawing order) and its place changed. Cards before the
/// closed one, and cards that did not move, do not slide.
pub fn slide_start(
    last: &[(SlideKey, Rect)],
    closed: &[isize],
    key: SlideKey,
    target: Rect,
) -> Option<Rect> {
    let gap = last.iter().position(|(card, _)| closed.contains(&card.0))?;
    let at = last.iter().position(|(card, _)| *card == key)?;
    let before = last[at].1;
    (at > gap && before.min != target.min).then_some(before)
}

#[derive(Clone, Copy, Debug)]
struct Slide {
    from: Rect,
    to: Rect,
    since: Instant,
}

/// Where each card was drawn, and the slides that run.
#[derive(Default)]
pub struct Slides {
    /// This frame's cards, as drawn, in drawing order.
    drawn: Vec<(SlideKey, Rect)>,
    last: Vec<(SlideKey, Rect)>,
    /// A card was just taken out: cards after it start to slide this frame.
    gap: Vec<isize>,
    running: HashMap<SlideKey, Slide>,
}

impl Slides {
    /// Called once at the start of a frame, before any card is drawn.
    pub fn next_frame(&mut self) {
        self.last = std::mem::take(&mut self.drawn);
        self.gap.clear();
        let last = &self.last;
        self.running
            .retain(|key, _| last.iter().any(|(card, _)| card == key));
    }

    /// The cards of the windows `closed` are gone: the cards after them
    /// slide.
    pub fn open_gap(&mut self, closed: &[isize]) {
        self.gap = closed.to_vec();
    }

    /// Where the card `key` is drawn, given the place the layout gave it.
    pub fn place(&mut self, key: SlideKey, target: Rect, now: Instant, animate: bool) -> Rect {
        let drawn = self.slid(key, target, now, animate);
        self.drawn.push((key, drawn));
        drawn
    }

    fn slid(&mut self, key: SlideKey, target: Rect, now: Instant, animate: bool) -> Rect {
        if !animate {
            self.running.remove(&key);
            return target;
        }
        let seen = self.last.iter().find(|(card, _)| *card == key).map(|c| c.1);
        if !self.gap.is_empty() {
            if let Some(from) = slide_start(&self.last, &self.gap, key, target) {
                let start = Slide {
                    from,
                    to: target,
                    since: now,
                };
                self.running.insert(key, start);
            }
        }
        let Some(slide) = self.running.get(&key).copied() else {
            return target;
        };
        let slide = if slide.to == target {
            slide
        } else {
            // The layout moved again: go on from where the card is drawn.
            let again = Slide {
                from: seen.unwrap_or(slide.from),
                to: target,
                since: now,
            };
            self.running.insert(key, again);
            again
        };
        let elapsed = now.saturating_duration_since(slide.since);
        if elapsed >= SLIDE_TIME {
            self.running.remove(&key);
            return target;
        }
        target.translate(slide_offset(slide.from, slide.to, elapsed))
    }

    /// Whether a card still slides, so the strip keeps repainting.
    pub fn running(&self) -> bool {
        !self.running.is_empty()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn card(x: f32) -> Rect {
        Rect::from_min_size(pos2(x, 10.0), vec2(100.0, 80.0))
    }

    #[test]
    fn a_closed_card_shrinks_to_85_percent_and_fades_out_in_180_ms() {
        assert_eq!(close_curve(ms(0)), (1.0, 1.0));
        let (scale, fade) = close_curve(ms(90));
        assert!(scale < 0.88 && scale > CLOSE_SCALE, "{scale}");
        assert!(
            fade < 0.2 && fade > 0.0,
            "ease-out: mostly done by half time"
        );
        let (scale, fade) = close_curve(ms(180));
        assert!((scale - CLOSE_SCALE).abs() < 1e-6);
        assert_eq!(fade, 0.0);
        assert_eq!(close_curve(ms(900)), close_curve(ms(180)));
    }

    #[test]
    fn a_card_that_comes_back_runs_the_shrink_in_reverse() {
        assert_eq!(regrow_curve(ms(0)), (CLOSE_SCALE, 0.0));
        assert_eq!(regrow_curve(ms(180)), (1.0, 1.0));
        let (scale, fade) = regrow_curve(ms(90));
        assert!(scale > 0.97 && fade > 0.8, "{scale} {fade}");
    }

    #[test]
    fn a_slide_starts_at_the_old_place_and_ends_exactly_at_the_new_one() {
        let (from, to) = (card(300.0), card(100.0));
        assert_eq!(slide_offset(from, to, ms(0)), vec2(200.0, 0.0));
        let half = slide_offset(from, to, ms(100));
        assert!(half.x > 0.0 && half.x < 50.0, "ease-out: {}", half.x);
        assert_eq!(slide_offset(from, to, ms(200)), Vec2::ZERO);
        assert_eq!(slide_offset(from, to, ms(2000)), Vec2::ZERO);
    }

    #[test]
    fn only_cards_after_the_closed_one_slide() {
        // 1, 2 and 3 side by side; card 2 is closed; 3 moves into its place.
        let last = [
            ((1, false), card(0.0)),
            ((2, false), card(108.0)),
            ((3, false), card(216.0)),
        ];
        assert_eq!(slide_start(&last, &[2], (1, false), card(0.0)), None);
        assert_eq!(
            slide_start(&last, &[2], (3, false), card(108.0)),
            Some(card(216.0))
        );
        assert_eq!(
            slide_start(&last, &[2], (3, false), card(216.0)),
            None,
            "a card that did not move does not slide"
        );
        assert_eq!(
            slide_start(&last, &[2], (1, false), card(50.0)),
            None,
            "a card before the closed one never slides"
        );
        assert_eq!(slide_start(&last, &[9], (3, false), card(108.0)), None);
    }

    #[test]
    fn cards_that_wrap_into_the_row_above_slide_too() {
        let up = |x, y| Rect::from_min_size(pos2(x, y), vec2(100.0, 80.0));
        let last = [
            ((1, false), up(0.0, 0.0)),
            ((2, false), up(108.0, 0.0)),
            ((3, false), up(0.0, 88.0)),
        ];
        assert_eq!(
            slide_start(&last, &[1], (3, false), up(108.0, 0.0)),
            Some(up(0.0, 88.0))
        );
    }

    #[test]
    fn slides_run_from_the_closed_gap_and_stop_at_the_target() {
        let t0 = Instant::now();
        let mut slides = Slides::default();
        for (hwnd, x) in [(1, 0.0), (2, 108.0), (3, 216.0)] {
            slides.place((hwnd, false), card(x), t0, true);
        }
        slides.next_frame();
        slides.open_gap(&[2]);
        let first = slides.place((1, false), card(0.0), t0, true);
        assert_eq!(first, card(0.0));
        let start = slides.place((3, false), card(108.0), t0, true);
        assert_eq!(start, card(216.0), "starts at the old place");
        assert!(slides.running());
        slides.next_frame();
        let mid = slides.place((3, false), card(108.0), t0 + ms(100), true);
        assert!(mid.min.x > 108.0 && mid.min.x < 216.0);
        slides.next_frame();
        let end = slides.place((3, false), card(108.0), t0 + ms(200), true);
        assert_eq!(end, card(108.0));
        assert!(!slides.running());
    }

    #[test]
    fn with_animations_off_nothing_slides() {
        let t0 = Instant::now();
        let mut slides = Slides::default();
        for (hwnd, x) in [(1, 0.0), (2, 108.0), (3, 216.0)] {
            slides.place((hwnd, false), card(x), t0, false);
        }
        slides.next_frame();
        slides.open_gap(&[2]);
        assert_eq!(
            slides.place((3, false), card(108.0), t0, false),
            card(108.0)
        );
        assert!(!slides.running());
    }

    #[test]
    fn a_card_comes_back_only_when_its_window_is_still_listed_after_1_5_s() {
        assert!(!come_back(ms(1499), true));
        assert!(come_back(ms(1500), true));
        assert!(!come_back(ms(1500), false), "gone: the normal leave runs");
        assert!(!come_back(ms(0), true));
    }

    #[test]
    fn the_note_names_the_app() {
        assert_eq!(waiting_text("Notepad"), "Waiting for Notepad");
    }
}
