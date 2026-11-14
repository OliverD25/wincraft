//! The "job on a server" mark, shared by ClaudeDeck's list and WindowDeck's
//! Claude cards: two stacked server boxes drawn as a thin outline with
//! hollow dots, pulsing slowly in opacity while the job uses the CPU.

use egui::{pos2, Color32, Painter, Pos2, Rect, Stroke};

use crate::core::theme;

/// The opacity of the mark: 55 to 100 % while busy, as `turn` (0 to 1)
/// goes round; solid while the job rests. With animations off `turn`
/// stays 0, so a busy job shows still, at 55 %.
pub fn pulse(busy: bool, turn: f32) -> f32 {
    if busy {
        0.55 + 0.45 * (turn * std::f32::consts::PI).sin().powi(2)
    } else {
        1.0
    }
}

/// Paints the mark into the square `rect`, drawn on a 14-unit grid.
pub fn paint(painter: &Painter, rect: Rect, colour: Color32, busy: bool, turn: f32) {
    let unit = rect.width() / 14.0;
    let at = |x: f32, y: f32| -> Pos2 { pos2(rect.left() + x * unit, rect.top() + y * unit) };
    let colour = theme::with_alpha(colour, pulse(busy, turn));
    for top in [1.5, 7.5] {
        let rect_box = Rect::from_min_max(at(1.5, top), at(12.5, top + 5.0));
        painter.rect_stroke(
            rect_box,
            1.2 * unit,
            Stroke::new(1.1 * unit, colour),
            egui::StrokeKind::Inside,
        );
        painter.circle_stroke(at(4.3, top + 2.5), unit, Stroke::new(0.8 * unit, colour));
    }
}

#[cfg(test)]
mod tests;
