//! Quick access with no windows yet (design 4): two dashed card outlines
//! with dashed keycaps 1 and 2 where windows will go, what to press to add
//! one, and a button that opens the picker.

use egui::epaint::Shape;
use egui::{pos2, vec2, Align2, Painter, Pos2, Rect, Stroke, Ui, UiBuilder};

use crate::core::shortcuts;
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::button::Kind;

use super::buttons::{self, Extra};
use super::grid::{card_rect, CAP, CAP_GAP, CARD_H};
use super::Outcome;
use crate::plugins::quick_jump::shared::Request;

const CORNER: f32 = 9.0;
/// Between the cards' row and the text, and between the text and the button.
const BLOCK_GAP: f32 = 18.0;
const TITLE_H: f32 = 24.0;
const LINE_GAP: f32 = 8.0;
const KEYS_H: f32 = 26.0;
const NOTE_H: f32 = 18.0;
const TEXT_TOP: f32 = 4.0;
const BUTTON_H: f32 = 32.0;
/// More room under the button than the list has under its cards.
pub const BOTTOM: f32 = 28.0;

pub const TITLE: &str = "No windows yet";
pub const ADD_LABEL: &str = "Add a window to Quick access\u{2026}";
const NOTE: &str = "Up to 10 windows, numbered 1\u{2013}9 and 0 in the order you add them.";

/// Everything under the header, from the top of the first card.
pub fn body_height() -> f32 {
    (CARD_H + CAP_GAP + CAP)
        + BLOCK_GAP
        + TEXT_TOP
        + TITLE_H
        + LINE_GAP
        + KEYS_H
        + LINE_GAP
        + NOTE_H
        + BLOCK_GAP
        + BUTTON_H
}

/// A piece of the "how to add one" line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Words(String),
    Cap(String),
}

/// "In any window, press [Win] [`] then [A] to add it here as number 1."
/// from the jump shortcut as written ("Win+`", or a chord such as
/// "Win+Alt+K, J"). None while that shortcut has no keys.
pub fn how_to_add(jump_keys: &str) -> Option<Vec<Piece>> {
    let steps = shortcuts::parse(jump_keys)?.steps();
    let mut pieces = vec![Piece::Words("In any window, press".to_string())];
    for (index, step) in steps.into_iter().enumerate() {
        if index > 0 {
            pieces.push(Piece::Words("then".to_string()));
        }
        pieces.extend(step.into_iter().map(Piece::Cap));
    }
    pieces.push(Piece::Words("then".to_string()));
    pieces.push(Piece::Cap("A".to_string()));
    pieces.push(Piece::Words("to add it here as number 1.".to_string()));
    Some(pieces)
}

/// The points of a rounded rectangle's outline, to draw it dashed.
fn outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    const STEPS: usize = 6;
    let quarter = std::f32::consts::FRAC_PI_2;
    // Each corner's arc: its centre and the angle it starts at.
    let arcs = [
        (pos2(rect.right() - radius, rect.top() + radius), -quarter),
        (pos2(rect.right() - radius, rect.bottom() - radius), 0.0),
        (pos2(rect.left() + radius, rect.bottom() - radius), quarter),
        (
            pos2(rect.left() + radius, rect.top() + radius),
            2.0 * quarter,
        ),
    ];
    let mut points = vec![pos2(rect.left() + radius, rect.top())];
    for (centre, start) in arcs {
        for step in 0..=STEPS {
            let angle = start + quarter * step as f32 / STEPS as f32;
            points.push(centre + radius * vec2(angle.cos(), angle.sin()));
        }
    }
    points.push(points[0]);
    points
}

fn dashed(painter: &Painter, rect: Rect, radius: f32, stroke: Stroke, dash: f32, gap: f32) {
    painter.extend(Shape::dashed_line(
        &outline(rect, radius),
        stroke,
        dash,
        gap,
    ));
}

fn plus(painter: &Painter, centre: Pos2, colour: egui::Color32) {
    let line = Stroke::new(1.75, colour);
    painter.hline(centre.x - 7.0..=centre.x + 7.0, centre.y, line);
    painter.vline(centre.x, centre.y - 7.0..=centre.y + 7.0, line);
}

fn accent_cap(painter: &Painter, x: f32, middle: f32, key: &str, tokens: &Tokens) -> f32 {
    let galley = painter.layout_no_wrap(key.to_string(), theme::semibold(13.0), tokens.accent);
    let width = (galley.size().x + 14.0).max(KEYS_H);
    let rect = Rect::from_min_size(pos2(x, middle - KEYS_H / 2.0), vec2(width, KEYS_H));
    painter.rect_stroke(
        rect,
        5,
        Stroke::new(1.0, tokens.accent),
        egui::StrokeKind::Inside,
    );
    painter.hline(
        rect.x_range().shrink(2.0),
        rect.bottom() - 1.0,
        Stroke::new(2.0, tokens.accent),
    );
    painter.text(
        rect.center() - vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        key,
        theme::semibold(13.0),
        tokens.accent,
    );
    rect.right()
}

/// Draws the empty state from `origin`, the top left of the first card.
/// Enter or the button asks for the picker.
pub fn draw(ui: &mut Ui, painter: &Painter, origin: Pos2, jump_keys: &str, out: &mut Vec<Outcome>) {
    let tokens = Tokens::get(ui.ctx());
    let dash = Stroke::new(2.0, tokens.border);
    for index in 0..2 {
        let card = card_rect(origin, index);
        dashed(painter, card.shrink(1.0), CORNER, dash, 6.0, 4.0);
        plus(painter, card.center(), tokens.text_disabled);
        let cap = Rect::from_center_size(
            pos2(card.center().x, card.bottom() + CAP_GAP + CAP / 2.0),
            vec2(CAP, CAP),
        );
        dashed(
            painter,
            cap.shrink(0.5),
            6.0,
            Stroke::new(1.0, tokens.border),
            3.0,
            3.0,
        );
        painter.hline(
            cap.x_range().shrink(3.0),
            cap.bottom() - 1.5,
            Stroke::new(3.0, tokens.border),
        );
        painter.text(
            cap.center() - vec2(0.0, 1.0),
            Align2::CENTER_CENTER,
            (index + 1).to_string(),
            theme::semibold(15.0),
            tokens.text_disabled,
        );
    }

    let left = origin.x + 2.0;
    let mut y = origin.y + CARD_H + CAP_GAP + CAP + BLOCK_GAP + TEXT_TOP;
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        TITLE,
        theme::regular(18.0),
        tokens.text_primary,
    );
    y += TITLE_H + LINE_GAP;
    let middle = y + KEYS_H / 2.0;
    match how_to_add(jump_keys) {
        Some(pieces) => {
            let mut x = left;
            for piece in pieces {
                x = match piece {
                    Piece::Words(words) => painter
                        .text(
                            pos2(x, middle),
                            Align2::LEFT_CENTER,
                            words,
                            theme::regular(14.0),
                            tokens.text_secondary,
                        )
                        .right(),
                    Piece::Cap(key) => accent_cap(painter, x, middle, &key, &tokens),
                } + 6.0;
            }
        }
        None => {
            painter.text(
                pos2(left, middle),
                Align2::LEFT_CENTER,
                "Use the button below to add a window as number 1.",
                theme::regular(14.0),
                tokens.text_secondary,
            );
        }
    }
    y += KEYS_H + LINE_GAP;
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        NOTE,
        theme::regular(13.0),
        tokens.text_disabled,
    );
    y += NOTE_H + BLOCK_GAP;

    let area = Rect::from_min_size(pos2(origin.x, y), vec2(300.0, BUTTON_H));
    let clicked = ui
        .scope_builder(UiBuilder::new().max_rect(area), |ui| {
            buttons::draw(ui, ADD_LABEL, Kind::Secondary, Extra::Plus).clicked()
        })
        .inner;
    let enter = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));
    if clicked || enter {
        out.push(Outcome::Send(Request::OpenPicker));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(pieces: &[Piece]) -> Vec<String> {
        pieces
            .iter()
            .map(|piece| match piece {
                Piece::Words(text) => text.clone(),
                Piece::Cap(key) => format!("[{key}]"),
            })
            .collect()
    }

    #[test]
    fn the_line_names_the_jump_keys_then_a() {
        let pieces = how_to_add("Win+`").unwrap();
        assert_eq!(
            words(&pieces),
            [
                "In any window, press",
                "[Win]",
                "[`]",
                "then",
                "[A]",
                "to add it here as number 1."
            ]
        );
    }

    #[test]
    fn a_chord_as_the_jump_shortcut_keeps_its_two_steps() {
        let pieces = how_to_add("Win+Alt+K, J").unwrap();
        assert_eq!(
            words(&pieces),
            [
                "In any window, press",
                "[Win]",
                "[Alt]",
                "[K]",
                "then",
                "[J]",
                "then",
                "[A]",
                "to add it here as number 1."
            ]
        );
    }

    #[test]
    fn no_jump_keys_means_no_line_about_them() {
        assert_eq!(how_to_add(""), None);
    }

    #[test]
    fn the_empty_window_is_as_tall_as_the_design_shows() {
        // 18 top, 28 header, 18 gap, then the body, then 28 under the button.
        assert_eq!(18.0 + 28.0 + 18.0 + body_height() + BOTTOM, 538.0);
    }

    #[test]
    fn the_outline_is_one_closed_path_inside_the_rectangle() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(300.0, 250.0));
        let points = outline(rect, 9.0);
        assert_eq!(points.first(), points.last());
        assert!(points
            .iter()
            .all(|point| rect.expand(0.01).contains(*point)));
    }
}
