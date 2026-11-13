//! The status marks, 14 px, as on the design page (option A, "Shapes"):
//! a triangle for approval, a speech bubble for a question, a tick, a
//! turning ring, a dashed ring, a hollow circle, a square, a cross and a
//! dashed "?". Each status has its own colour and its own shape, so it
//! reads without colour too. "Shapes and words" adds a short word.

use std::f32::consts::{FRAC_PI_2, TAU};

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::core::claude_sessions::Age;
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::model::Mark;
use super::MarkStyle;

pub const SIZE: f32 = 14.0;
/// The shape and its word take at least this much, so names line up.
pub const WORD_WIDTH: f32 = 66.0;
const WORD_GAP: f32 = 4.0;
const WORD_SIZE: f32 = 12.0;

/// The colour of a mark and of its word.
pub fn colour(mark: Mark, tokens: &Tokens) -> Color32 {
    match mark {
        Mark::NeedsApproval | Mark::AsksYou => tokens.warning,
        Mark::DoneNotSeen => tokens.success,
        Mark::Working | Mark::WaitsOnServer | Mark::ServerJob(_) | Mark::Background => tokens.info,
        Mark::Idle(Age::Fresh) => tokens.text_secondary,
        Mark::Idle(Age::Cooling) => tokens.text_disabled,
        Mark::Idle(Age::Cold | Age::Frozen) => tokens.info,
        Mark::Stopped => tokens.text_disabled,
        Mark::Failed => tokens.danger,
        Mark::Unknown => tokens.text_secondary,
    }
}

/// The word's colour: Idle keeps the secondary text colour unless it is
/// cold or frozen.
fn word_colour(mark: Mark, tokens: &Tokens) -> Color32 {
    match mark {
        Mark::Idle(Age::Fresh | Age::Cooling) => tokens.text_secondary,
        _ => colour(mark, tokens),
    }
}

/// Paints a mark, with its word when the style asks, its top-left corner
/// at `at` and its middle at `at.y + SIZE / 2`. `turn` (0 to 1) is where
/// the Working ring has got to. Returns the width it took.
pub fn paint(
    painter: &Painter,
    at: Pos2,
    mark: Mark,
    style: MarkStyle,
    tokens: &Tokens,
    turn: f32,
) -> f32 {
    if style == MarkStyle::Off {
        return 0.0;
    }
    shape(
        painter,
        Rect::from_min_size(at, vec2(SIZE, SIZE)),
        mark,
        tokens,
        turn,
    );
    if style == MarkStyle::Shapes {
        return SIZE;
    }
    let font = if mark.bold() {
        theme::semibold(WORD_SIZE)
    } else {
        theme::regular(WORD_SIZE)
    };
    let galley = painter.layout_job(text::job(
        mark.word(),
        font,
        word_colour(mark, tokens),
        None,
    ));
    let left = at.x + SIZE + WORD_GAP;
    let width = galley.size().x;
    let top = at.y + SIZE / 2.0 - galley.size().y / 2.0;
    painter.galley(pos2(left, top), galley, word_colour(mark, tokens));
    (SIZE + WORD_GAP + width).max(WORD_WIDTH)
}

/// The room a mark takes in this style.
pub fn width(style: MarkStyle) -> f32 {
    match style {
        MarkStyle::Off => 0.0,
        MarkStyle::Shapes => SIZE,
        MarkStyle::ShapesAndWords => WORD_WIDTH,
    }
}

/// A point of the 14 x 14 drawing inside `rect`.
fn at(rect: Rect, x: f32, y: f32) -> Pos2 {
    let scale = rect.width() / SIZE;
    pos2(rect.left() + x * scale, rect.top() + y * scale)
}

fn ring(rect: Rect, radius: f32, from: f32, to: f32, steps: usize) -> Vec<Pos2> {
    (0..=steps)
        .map(|step| {
            let angle = from + (to - from) * step as f32 / steps as f32;
            at(rect, 7.0 + radius * angle.cos(), 7.0 + radius * angle.sin())
        })
        .collect()
}

fn dashed_ring(painter: &Painter, rect: Rect, radius: f32, stroke: Stroke, dash: f32, gap: f32) {
    let points = ring(rect, radius, 0.0, TAU, 48);
    painter.extend(Shape::dashed_line(&points, stroke, dash, gap));
}

/// The "?" of a speech bubble or of Unknown, drawn as a line so it stays
/// sharp at 14 px.
fn question(painter: &Painter, rect: Rect, top: f32, colour: Color32) {
    let curve = [
        (5.3, 1.4),
        (5.7, 0.5),
        (6.5, 0.0),
        (7.6, 0.0),
        (8.5, 0.6),
        (8.6, 1.6),
        (7.9, 2.4),
        (7.0, 3.0),
        (7.0, 3.9),
    ]
    .iter()
    .map(|(x, y)| at(rect, *x, top + y))
    .collect();
    painter.add(Shape::line(curve, Stroke::new(1.4, colour)));
    painter.circle_filled(at(rect, 7.0, top + 5.5), 0.8, colour);
}

/// The shape alone, in a `SIZE` square.
pub fn shape(painter: &Painter, rect: Rect, mark: Mark, tokens: &Tokens, turn: f32) {
    let colour = colour(mark, tokens);
    let hole = tokens.window_bg;
    let centre = at(rect, 7.0, 7.0);
    match mark {
        Mark::NeedsApproval => {
            painter.add(Shape::convex_polygon(
                vec![
                    at(rect, 7.0, 1.2),
                    at(rect, 13.0, 12.3),
                    at(rect, 1.0, 12.3),
                ],
                colour,
                Stroke::NONE,
            ));
            painter.rect_filled(
                Rect::from_min_size(at(rect, 6.2, 4.6), vec2(1.6, 4.0)),
                0.6,
                hole,
            );
            painter.rect_filled(
                Rect::from_min_size(at(rect, 6.2, 9.5), vec2(1.6, 1.6)),
                0.6,
                hole,
            );
        }
        Mark::AsksYou => {
            painter.circle_filled(at(rect, 7.0, 6.5), 5.2, colour);
            painter.add(Shape::convex_polygon(
                vec![at(rect, 2.6, 8.8), at(rect, 4.8, 10.9), at(rect, 1.6, 12.4)],
                colour,
                Stroke::NONE,
            ));
            question(painter, rect, 3.6, hole);
        }
        Mark::DoneNotSeen => {
            painter.circle_filled(centre, 6.0, colour);
            painter.add(Shape::line(
                vec![at(rect, 4.0, 7.2), at(rect, 6.0, 9.2), at(rect, 10.0, 4.8)],
                Stroke::new(1.7, hole),
            ));
        }
        Mark::Working => {
            painter.circle_stroke(
                centre,
                5.0,
                Stroke::new(2.0, theme::with_alpha(colour, 0.3)),
            );
            let from = -FRAC_PI_2 + turn * TAU;
            painter.add(Shape::line(
                ring(rect, 5.0, from, from + FRAC_PI_2, 12),
                Stroke::new(2.0, colour),
            ));
        }
        Mark::WaitsOnServer => {
            // A server: two stacked boxes, each with a dot.
            for top in [1.5, 7.5] {
                let rect_box = Rect::from_min_max(at(rect, 1.5, top), at(rect, 12.5, top + 5.0));
                painter.rect_stroke(
                    rect_box,
                    1.2,
                    Stroke::new(1.4, colour),
                    egui::StrokeKind::Inside,
                );
                painter.circle_filled(at(rect, 4.3, top + 2.5), 0.9, colour);
            }
        }
        Mark::ServerJob(busy) => {
            // The server of Waits on a server, as a thin outline with hollow
            // dots; it pulses slowly while the job uses CPU.
            let pulse = if busy {
                0.55 + 0.45 * (turn * std::f32::consts::PI).sin().powi(2)
            } else {
                1.0
            };
            let colour = theme::with_alpha(colour, pulse);
            for top in [1.5, 7.5] {
                let rect_box = Rect::from_min_max(at(rect, 1.5, top), at(rect, 12.5, top + 5.0));
                painter.rect_stroke(
                    rect_box,
                    1.2,
                    Stroke::new(1.1, colour),
                    egui::StrokeKind::Inside,
                );
                painter.circle_stroke(at(rect, 4.3, top + 2.5), 1.0, Stroke::new(0.8, colour));
            }
        }
        Mark::Background => {
            dashed_ring(painter, rect, 5.0, Stroke::new(1.6, colour), 2.2, 1.6);
            painter.circle_filled(centre, 2.0, colour);
        }
        Mark::Idle(Age::Frozen) => {
            let stroke = Stroke::new(1.3, colour);
            for (a, b) in [
                ((7.0, 1.5), (7.0, 12.5)),
                ((2.2, 4.2), (11.8, 9.8)),
                ((2.2, 9.8), (11.8, 4.2)),
            ] {
                painter.line_segment([at(rect, a.0, a.1), at(rect, b.0, b.1)], stroke);
            }
        }
        Mark::Idle(_) => {
            painter.circle_stroke(centre, 4.6, Stroke::new(1.6, colour));
        }
        Mark::Stopped => {
            painter.rect_filled(
                Rect::from_min_size(at(rect, 2.5, 2.5), vec2(9.0, 9.0)),
                1.5,
                colour,
            );
        }
        Mark::Failed => {
            painter.circle_filled(centre, 6.0, colour);
            let stroke = Stroke::new(1.7, hole);
            painter.line_segment([at(rect, 4.6, 4.6), at(rect, 9.4, 9.4)], stroke);
            painter.line_segment([at(rect, 9.4, 4.6), at(rect, 4.6, 9.4)], stroke);
        }
        Mark::Unknown => {
            dashed_ring(painter, rect, 5.5, Stroke::new(1.2, colour), 2.0, 1.5);
            question(painter, rect, 4.0, colour);
        }
    }
}

/// The dashed square of a stand-in for an archived or missing chat.
pub fn stand_in(painter: &Painter, rect: Rect, tokens: &Tokens) {
    let (l, t, r, b) = (2.0, 2.0, 12.0, 12.0);
    let points = vec![
        at(rect, l, t),
        at(rect, r, t),
        at(rect, r, b),
        at(rect, l, b),
        at(rect, l, t),
    ];
    painter.extend(Shape::dashed_line(
        &points,
        Stroke::new(1.0, tokens.text_disabled),
        2.0,
        1.5,
    ));
}
