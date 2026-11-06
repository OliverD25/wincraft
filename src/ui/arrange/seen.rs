//! Ctrl+M in the strip: "Mark all as seen". The host clears the marks and
//! answers with the windows it cleared; the strip says so for a few seconds
//! and keeps that list so Ctrl+Z can put the marks back.

use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Rect, StrokeKind};

use crate::core::attention;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{ArrangeSnapshot, HostRequest};

use super::Shared;

/// How long the line (and with it the undo) stays.
pub(super) const LINE_TIME: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(super) struct Seen {
    line: Option<(String, Instant)>,
    undo: Vec<isize>,
}

impl Seen {
    /// The host's answer to Ctrl+M (`windows`), or to Ctrl+Z (`undone`).
    pub fn answer(&mut self, windows: Vec<isize>, undone: bool, now: Instant) {
        if undone {
            self.line = Some((attention::UNDONE_LINE.to_string(), now));
            self.undo.clear();
            return;
        }
        self.line = Some((attention::marked_line(windows.len()), now));
        self.undo = windows;
    }

    /// Whether the line shows the Ctrl + Z chip and "Undo" after the text.
    pub fn offers_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// The line to show now, and how long it has left.
    pub fn line(&self, now: Instant) -> Option<(&str, Duration)> {
        let (text, since) = self.line.as_ref()?;
        let left = LINE_TIME.checked_sub(now.saturating_duration_since(*since))?;
        Some((text, left))
    }

    /// Ctrl+Z: the windows to mark again, only while the line shows.
    pub fn take_undo(&mut self, now: Instant) -> Option<Vec<isize>> {
        if self.undo.is_empty() || self.line(now).is_none() {
            return None;
        }
        Some(std::mem::take(&mut self.undo))
    }
}

/// Whether any window in the snapshot has the orange mark: then the hints
/// offer Ctrl+M.
pub(super) fn any_marked(snapshot: &ArrangeSnapshot) -> bool {
    snapshot
        .groups
        .iter()
        .flat_map(|group| group.windows.iter())
        .any(|window| window.attention)
}

/// Handles Ctrl+M and Ctrl+Z, and draws the line. The caller has checked
/// that no menu is open, nothing is typed and no card is dragged.
pub(super) fn keys(ctx: &egui::Context, shared: &mut Shared) {
    let (mark, undo) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::CTRL, egui::Key::M),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::Z),
        )
    });
    let now = Instant::now();
    if mark {
        shared.to_host.send(HostRequest::MarkAllSeen);
    } else if undo {
        if let Some(windows) = shared.seen.take_undo(now) {
            shared.to_host.send(HostRequest::RestoreMarks(windows));
        }
    }
}

/// The key chip's text before "Undo".
pub(super) const UNDO_KEY: &str = "Ctrl + Z";

/// The line at the foot of the strip, above everything. With `undo` it
/// ends with a dot, the Ctrl + Z key chip (drawn like the hint chips of
/// the footer) and "Undo".
pub(super) fn line(ctx: &egui::Context, panel: Rect, text: &str, undo: bool) {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("arrange-seen-line"),
    ));
    let font = theme::regular(12.0);
    let galley = layer.layout_no_wrap(text.to_string(), font.clone(), tokens.text_primary);
    let tail = undo.then(|| {
        (
            layer.layout_no_wrap("\u{b7}".to_string(), font.clone(), tokens.text_disabled),
            layer.layout_no_wrap(UNDO_KEY.to_string(), font.clone(), tokens.text_primary),
            layer.layout_no_wrap("Undo".to_string(), font.clone(), tokens.text_primary),
        )
    });
    let tail_width = tail.as_ref().map_or(0.0, |(dot, key, word)| {
        8.0 + dot.size().x + 8.0 + key.size().x + 12.0 + 6.0 + word.size().x
    });
    let size = vec2(galley.size().x + tail_width + 24.0, galley.size().y + 12.0);
    let rect = Rect::from_center_size(
        pos2(panel.center().x, panel.bottom() - 8.0 - size.y / 2.0),
        size,
    );
    layer.rect(
        rect,
        tokens.radius_row,
        tokens.elevated_bg,
        theme::line(ctx, tokens.border),
        StrokeKind::Inside,
    );
    let mut x = rect.left() + 12.0;
    let middle = rect.center().y;
    let text_width = galley.size().x;
    let height = galley.size().y;
    layer.galley(pos2(x, middle - height / 2.0), galley, tokens.text_primary);
    x += text_width;
    if let Some((dot, key, word)) = tail {
        x += 8.0;
        let dot_width = dot.size().x;
        layer.galley(
            pos2(x, middle - dot.size().y / 2.0),
            dot,
            tokens.text_disabled,
        );
        x += dot_width + 8.0;
        let chip = Rect::from_min_size(
            pos2(x, middle - key.size().y / 2.0 - 1.0),
            vec2(key.size().x + 12.0, key.size().y + 2.0),
        );
        layer.rect(
            chip,
            tokens.radius_row,
            tokens.panel_bg,
            theme::line(ctx, tokens.border),
            StrokeKind::Inside,
        );
        layer.galley(chip.center() - key.size() / 2.0, key, tokens.text_primary);
        x = chip.right() + 6.0;
        layer.galley(
            pos2(x, middle - word.size().y / 2.0),
            word,
            tokens.text_primary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_count_and_offers_the_undo() {
        let now = Instant::now();
        let mut seen = Seen::default();
        seen.answer(vec![1], false, now);
        assert_eq!(seen.line(now).unwrap().0, "Marked 1 window as seen");
        assert!(seen.offers_undo(), "the chip and Undo are drawn after it");
        seen.answer(vec![1, 2, 3], false, now);
        assert_eq!(seen.line(now).unwrap().0, "Marked 3 windows as seen");
    }

    #[test]
    fn with_no_marked_window_the_line_says_so_and_there_is_nothing_to_undo() {
        let now = Instant::now();
        let mut seen = Seen::default();
        seen.answer(Vec::new(), false, now);
        assert_eq!(seen.line(now).unwrap().0, "No windows to mark as seen");
        assert!(!seen.offers_undo());
        assert_eq!(seen.take_undo(now), None);
    }

    #[test]
    fn the_undo_works_once_and_only_while_the_line_shows() {
        let now = Instant::now();
        let mut seen = Seen::default();
        seen.answer(vec![5, 6], false, now);
        assert_eq!(
            seen.take_undo(now + Duration::from_secs(1)),
            Some(vec![5, 6])
        );
        assert_eq!(seen.take_undo(now + Duration::from_secs(1)), None);

        seen.answer(vec![5, 6], false, now);
        assert!(seen.line(now + Duration::from_secs(5)).is_none());
        assert_eq!(seen.take_undo(now + Duration::from_secs(5)), None);
    }

    #[test]
    fn after_an_undo_the_line_says_the_taskbar_stays() {
        let now = Instant::now();
        let mut seen = Seen::default();
        seen.answer(vec![5], false, now);
        seen.answer(Vec::new(), true, now);
        assert_eq!(
            seen.line(now).unwrap().0,
            "Orange frames are back. The taskbar stays as it is."
        );
        assert_eq!(seen.take_undo(now), None);
    }
}
