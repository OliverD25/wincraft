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

/// One chip for a whole binding: 13 regular, padding 3 8, panel fill, 1 px
/// border, radius 4, secondary text; with `accent` it is outlined and
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
    /// 24 on the Hotkeys page, 22 in the palette.
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

/// Each piece of a drawn shortcut with its width and the gap before it, and
/// the width of them all.
fn layout_caps(ui: &Ui, steps: &[Vec<String>], look: Caps) -> (Vec<(Piece, f32, f32)>, f32) {
    let tokens = Tokens::get(ui.ctx());
    let font = theme::regular(12.0);
    let painter = ui.painter();
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
    (pieces, width)
}

/// How wide `caps` draws these keys.
pub fn caps_width(ui: &Ui, steps: &[Vec<String>], look: Caps) -> f32 {
    layout_caps(ui, steps, look).1
}

/// [Win][Alt][K] then [S]. `steps` holds each press's keys in order.
pub fn caps(ui: &mut Ui, steps: &[Vec<String>], look: Caps) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter().clone();
    let (pieces, width) = layout_caps(ui, steps, look);
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

/// One piece of the hotkey dialog's large keys row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Large {
    /// A modifier still held: accent border on the selection fill.
    Held(String),
    /// A pressed key: neutral, on the elevated fill.
    Key(String),
    /// "+" after the held modifiers.
    Plus,
    /// "then" between the two presses of a chord.
    Then,
    /// The dashed empty cap of the key still to come.
    Waiting,
}

const LARGE_HEIGHT: f32 = 44.0;
const LARGE_PAD: f32 = 16.0;
const LARGE_GAP: f32 = 10.0;
const LARGE_WAITING: f32 = 60.0;
const LARGE_FONT: f32 = 18.0;
const LARGE_RADIUS: u8 = 6;
/// The smallest the row may shrink to, so no key is written under 12 px.
const LARGE_MIN_SCALE: f32 = 12.0 / LARGE_FONT;

/// How much the row shrinks to fit `room`, from its width at full size.
fn large_scale(natural: f32, room: f32) -> f32 {
    if natural <= room || natural <= 0.0 {
        1.0
    } else {
        (room / natural).max(LARGE_MIN_SCALE)
    }
}

/// The dialog's keys: 44 px caps with 18 px text, a 1 px border with a
/// 3 px bottom edge, 10 apart, centred in `row`. A row too wide for it
/// shrinks, down to 12 px text.
pub fn large(ui: &Ui, row: Rect, pieces: &[Large]) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let measure = |scale: f32| -> Vec<(f32, Option<std::sync::Arc<egui::Galley>>)> {
        let font = theme::regular(LARGE_FONT * scale);
        pieces
            .iter()
            .map(|piece| match piece {
                Large::Held(key) | Large::Key(key) => {
                    let galley =
                        painter.layout_no_wrap(key.clone(), font.clone(), tokens.text_primary);
                    let width = (galley.size().x + 2.0 * LARGE_PAD * scale).max(LARGE_HEIGHT);
                    (width, Some(galley))
                }
                Large::Plus => {
                    let galley =
                        painter.layout_no_wrap("+".to_string(), font.clone(), tokens.text_disabled);
                    (galley.size().x, Some(galley))
                }
                Large::Then => {
                    let galley = painter.layout_no_wrap(
                        "then".to_string(),
                        theme::regular((13.0 * scale).max(12.0)),
                        tokens.text_disabled,
                    );
                    (galley.size().x, Some(galley))
                }
                Large::Waiting => (LARGE_WAITING * scale, None),
            })
            .collect()
    };
    let total = |cells: &[(f32, Option<std::sync::Arc<egui::Galley>>)], scale: f32| {
        let gaps = cells.len().saturating_sub(1) as f32 * LARGE_GAP * scale;
        cells.iter().map(|(width, _)| width).sum::<f32>() + gaps
    };
    let natural = measure(1.0);
    let scale = large_scale(total(&natural, 1.0), row.width());
    let cells = if scale < 1.0 { measure(scale) } else { natural };
    let mut x = row.center().x - total(&cells, scale) / 2.0;
    for (piece, (width, galley)) in pieces.iter().zip(cells) {
        let cell = Rect::from_min_size(
            pos2(x, row.center().y - LARGE_HEIGHT / 2.0),
            vec2(width, LARGE_HEIGHT),
        );
        match (piece, galley) {
            (Large::Held(_), Some(galley)) => {
                large_cap(ui, painter, cell, tokens.accent, tokens.selection_bg);
                painter.galley(
                    cell.center() - galley.size() / 2.0 - vec2(0.0, 1.0),
                    galley,
                    tokens.text_primary,
                );
            }
            (Large::Key(_), Some(galley)) => {
                large_cap(ui, painter, cell, tokens.border, tokens.elevated_bg);
                painter.galley(
                    cell.center() - galley.size() / 2.0 - vec2(0.0, 1.0),
                    galley,
                    tokens.text_primary,
                );
            }
            (Large::Plus | Large::Then, Some(galley)) => {
                painter.galley(
                    pos2(cell.left(), cell.center().y - galley.size().y / 2.0),
                    galley,
                    tokens.text_disabled,
                );
            }
            _ => dashed(ui, painter, cell, tokens.text_disabled),
        }
        x += width + LARGE_GAP * scale;
    }
}

/// A large cap: 1 px border and 3 px at the bottom, radius 6.
fn large_cap(ui: &Ui, painter: &Painter, cell: Rect, edge: Color32, fill: Color32) {
    let line = theme::line(ui.ctx(), edge);
    painter.rect(cell, LARGE_RADIUS, fill, line, StrokeKind::Inside);
    let extra = theme::snap(2.0, ui.ctx().pixels_per_point());
    let inner = f32::from(LARGE_RADIUS);
    painter.hline(
        cell.left() + inner..=cell.right() - inner,
        cell.bottom() - line.width - extra / 2.0,
        Stroke::new(extra, edge),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_large_row_shrinks_to_fit_but_never_under_12_px() {
        assert_eq!(large_scale(300.0, 344.0), 1.0);
        assert_eq!(large_scale(0.0, 344.0), 1.0);
        assert_eq!(large_scale(688.0, 344.0), LARGE_MIN_SCALE);
        assert!((large_scale(400.0, 344.0) - 0.86).abs() < 0.01);
        assert_eq!(large_scale(10_000.0, 344.0), LARGE_MIN_SCALE);
        assert!(LARGE_FONT * large_scale(10_000.0, 344.0) > 11.99);
    }

    #[test]
    fn a_binding_is_written_as_one_spaced_chip() {
        assert_eq!(split("Win+Alt+F1"), vec!["Win", "Alt", "F1"]);
        assert_eq!(spaced("Win+Alt+F1"), "Win + Alt + F1");
        assert_eq!(spaced("Ctrl + Shift + ,"), "Ctrl + Shift + ,");
        assert_eq!(spaced(""), "");
    }
}
