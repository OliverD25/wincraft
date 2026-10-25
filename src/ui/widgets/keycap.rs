use egui::{pos2, vec2, Color32, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};

/// "Win+Alt+F1" → ["Win", "Alt", "F1"].
pub fn split(binding: &str) -> Vec<&str> {
    binding
        .split('+')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .collect()
}

/// "Win+Alt+F1" → "Win + Alt + F1", the way a chip spells a binding.
pub fn spaced(binding: &str) -> String {
    split(binding).join(" + ")
}

/// Modifiers held so far during a capture, open-ended: "Win + Alt +".
pub fn held(modifiers: &str) -> String {
    let keys = spaced(modifiers);
    if keys.is_empty() {
        keys
    } else {
        format!("{keys} +")
    }
}

/// One chip for a whole binding: 13 regular, padding 3 8, panel fill, 1 px
/// border, radius 4, secondary text. While capturing it is outlined and
/// written in the accent colour.
pub fn binding(ui: &mut Ui, text: &str, accent: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (text_colour, stroke) = if accent {
        (tokens.accent, tokens.accent)
    } else {
        (tokens.text_secondary, tokens.border)
    };
    chip(ui, text, theme::regular(13.0), text_colour, stroke)
}

/// The same chip at the footer's 12 px, in any font: the return arrow only
/// exists in egui's monospace fallback.
pub fn hint(ui: &mut Ui, text: &str, font: FontId) -> Response {
    let tokens = Tokens::get(ui.ctx());
    chip(ui, text, font, tokens.text_secondary, tokens.border)
}

fn chip(ui: &mut Ui, text: &str, font: FontId, text_colour: Color32, stroke: Color32) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, text_colour);
    let size = vec2(galley.size().x + 16.0, galley.size().y + 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect(
            rect,
            tokens.radius_row,
            tokens.panel_bg,
            theme::line(ui.ctx(), stroke),
            StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, text_colour);
    }
    response
}

/// How a shortcut is drawn as keycaps: one cap per key, the presses of a
/// chord joined by "then".
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    /// 24 on the Keyboard shortcuts page, 22 in the palette.
    pub height: f32,
    /// This many presses from the start get an accent border, for Search
    /// by keys.
    pub matched: usize,
    /// A dashed empty cap after "then": the chord's second press is awaited.
    pub waiting: bool,
}

impl Caps {
    pub fn page() -> Self {
        Self {
            height: 24.0,
            matched: 0,
            waiting: false,
        }
    }

    pub fn palette() -> Self {
        Self {
            height: 22.0,
            ..Self::page()
        }
    }
}

const CAP_PAD: f32 = 7.0;
const KEY_GAP: f32 = 4.0;
const STEP_GAP: f32 = 8.0;

/// One piece of a drawn shortcut.
enum Piece {
    Key(std::sync::Arc<egui::Galley>, usize),
    Then(std::sync::Arc<egui::Galley>),
    Waiting,
}

/// [Win][Alt][K] then [S]. `steps` holds each press's keys in order.
pub fn caps(ui: &mut Ui, steps: &[Vec<String>], look: Caps) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let font = theme::regular(12.0);
    let painter = ui.painter().clone();
    let then = || painter.layout_no_wrap("then".to_string(), font.clone(), tokens.text_disabled);
    let mut pieces: Vec<(Piece, f32, f32)> = Vec::new();
    for (index, keys) in steps.iter().enumerate() {
        if index > 0 {
            let galley = then();
            let width = galley.size().x;
            pieces.push((Piece::Then(galley), width, STEP_GAP));
        }
        for (at, key) in keys.iter().enumerate() {
            let galley = painter.layout_no_wrap(key.clone(), font.clone(), tokens.text_primary);
            let width = (galley.size().x + 2.0 * CAP_PAD).max(look.height);
            let gap = if at == 0 { STEP_GAP } else { KEY_GAP };
            pieces.push((Piece::Key(galley, index), width, gap));
        }
    }
    if look.waiting {
        let galley = then();
        let width = galley.size().x;
        pieces.push((Piece::Then(galley), width, STEP_GAP));
        pieces.push((Piece::Waiting, look.height, STEP_GAP));
    }
    let width: f32 = pieces
        .iter()
        .enumerate()
        .map(|(at, (_, width, gap))| width + if at == 0 { 0.0 } else { *gap })
        .sum();
    let (rect, response) = ui.allocate_exact_size(vec2(width, look.height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let mut x = rect.left();
    for (at, (piece, width, gap)) in pieces.into_iter().enumerate() {
        if at > 0 {
            x += gap;
        }
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(width, look.height));
        match piece {
            Piece::Key(galley, step) => {
                let edge = if step < look.matched {
                    tokens.accent
                } else {
                    tokens.border
                };
                paint_cap(ui, &painter, cell, edge, tokens.elevated_bg);
                painter.galley(
                    cell.center() - galley.size() / 2.0 - vec2(0.0, 1.0),
                    galley,
                    tokens.text_primary,
                );
            }
            Piece::Then(galley) => painter.galley(
                pos2(cell.left(), cell.center().y - galley.size().y / 2.0),
                galley,
                tokens.text_disabled,
            ),
            Piece::Waiting => dashed(ui, &painter, cell, tokens.text_disabled),
        }
        x += width;
    }
    response
}

/// A cap: 1 px border with a 2 px bottom, so it reads as a key.
fn paint_cap(ui: &Ui, painter: &Painter, cell: Rect, edge: Color32, fill: Color32) {
    let tokens = Tokens::get(ui.ctx());
    let line = theme::line(ui.ctx(), edge);
    painter.rect(cell, tokens.radius_row, fill, line, StrokeKind::Inside);
    let bottom = theme::snap(1.0, ui.ctx().pixels_per_point());
    let inner = f32::from(tokens.radius_row).min(cell.width() / 2.0);
    painter.hline(
        cell.left() + inner..=cell.right() - inner,
        cell.bottom() - line.width - bottom / 2.0,
        Stroke::new(bottom, edge),
    );
}

/// The empty cap of a second press still to come, outlined in dashes.
fn dashed(ui: &Ui, painter: &Painter, cell: Rect, colour: Color32) {
    let stroke = theme::line(ui.ctx(), colour);
    let r = cell.shrink(stroke.width / 2.0);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    for pair in corners.windows(2) {
        painter.add(egui::Shape::dashed_line(pair, stroke, 3.0, 2.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_is_written_as_one_spaced_chip() {
        assert_eq!(split("Win+Alt+F1"), vec!["Win", "Alt", "F1"]);
        assert_eq!(spaced("Win+Alt+F1"), "Win + Alt + F1");
        assert_eq!(spaced("Ctrl + Shift + ,"), "Ctrl + Shift + ,");
        assert_eq!(spaced(""), "");
    }

    #[test]
    fn held_modifiers_stay_open_ended() {
        assert_eq!(held("Win+Alt"), "Win + Alt +");
        assert_eq!(held(""), "");
    }
}
