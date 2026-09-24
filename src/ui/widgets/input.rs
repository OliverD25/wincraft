use egui::{vec2, Align, Margin, Response, Sense, StrokeKind, TextEdit, Ui};

use crate::core::theme::{self, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// Body 14, for free text.
    Text,
    /// JetBrains Mono 13, for numbers and paths.
    Mono,
}

const HEIGHT: f32 = 32.0;

/// 32 high, 1 px border, radius 4. Focus is shown by the border turning
/// accent; inputs get no extra ring, because the caret already says where
/// typing will go.
pub fn text(ui: &mut Ui, id_salt: &str, value: &mut String, face: Face, width: f32) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
    let (font, colour) = match face {
        Face::Text => (theme::regular(14.0), tokens.text_primary),
        Face::Mono => (theme::mono(13.0), tokens.text_primary),
    };
    let edit = TextEdit::singleline(value)
        .id_salt(id_salt)
        .font(font)
        .text_color(colour)
        .frame(egui::Frame::NONE)
        .margin(Margin::symmetric(10, 0))
        .vertical_align(Align::Center)
        .desired_width(width - 20.0);
    let response = ui.put(rect, edit);

    let border = if response.has_focus() {
        tokens.accent
    } else {
        tokens.border
    };
    ui.painter().rect_stroke(
        rect,
        4,
        theme::stroke(ui.ctx(), 1.0, border),
        StrokeKind::Inside,
    );
    response
}

/// A number keeps its half-typed text while the field has focus, so typing
/// "0." on the way to "0.5" is not immediately rewritten to "0". Returns true
/// only when the text parses to a new value inside the range.
pub fn number(ui: &mut Ui, id_salt: &str, value: &mut f64, min: f64, max: f64) -> bool {
    let buffer_id = ui.id().with(("number-buffer", id_salt));
    let mut buffer = ui
        .data(|data| data.get_temp::<String>(buffer_id))
        .unwrap_or_else(|| format_number(*value));
    let response = text(ui, id_salt, &mut buffer, Face::Mono, 96.0);

    let mut changed = false;
    if response.changed() {
        if let Ok(parsed) = buffer.trim().parse::<f64>() {
            let clamped = parsed.clamp(min, max);
            if (clamped - *value).abs() > f64::EPSILON {
                *value = clamped;
                changed = true;
            }
        }
    }
    if response.has_focus() {
        ui.data_mut(|data| data.insert_temp(buffer_id, buffer));
    } else {
        ui.data_mut(|data| data.remove::<String>(buffer_id));
    }
    changed
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}
