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
        let text = attention::marked_line(windows.len());
        self.line = Some(if windows.is_empty() {
            (text, now)
        } else {
            (format!("{text} \u{b7} Ctrl+Z Undo"), now)
        });
        self.undo = windows;
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

/// The line at the foot of the strip, above everything.
pub(super) fn line(ctx: &egui::Context, panel: Rect, text: &str) {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("arrange-seen-line"),
    ));
    let galley = layer.layout_no_wrap(text.to_string(), theme::regular(12.0), tokens.text_primary);
    let size = galley.size() + vec2(24.0, 12.0);
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
    layer.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        tokens.text_primary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_count_and_offers_the_undo() {
        let now = Instant::now();
        let mut seen = Seen::default();
        seen.answer(vec![1], false, now);
        assert_eq!(
            seen.line(now).unwrap().0,
            "Marked 1 window as seen \u{b7} Ctrl+Z Undo"
        );
        seen.answer(vec![1, 2, 3], false, now);
        assert_eq!(
            seen.line(now).unwrap().0,
            "Marked 3 windows as seen \u{b7} Ctrl+Z Undo"
        );
    }

    #[test]
    fn with_no_marked_window_the_line_says_so_and_there_is_nothing_to_undo() {
        let now = Instant::now();
        let mut seen = Seen::default();
        seen.answer(Vec::new(), false, now);
        assert_eq!(seen.line(now).unwrap().0, "No windows to mark as seen");
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
