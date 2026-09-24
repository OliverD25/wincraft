use egui::{pos2, vec2, Align2, CursorIcon, Rect, Response, Sense, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::focus;

const WIDTH: f32 = 220.0;
const HEIGHT: f32 = 32.0;
const GAP: f32 = 12.0;
const READOUT: f32 = 36.0;
const KNOB: f32 = 16.0;

/// Digits after the point in `step`, so a 0.05 step reads "0.85", not "0.8500".
pub fn decimals(step: f64) -> usize {
    let text = format!("{step}");
    text.split_once('.')
        .map(|(_, fraction)| fraction.trim_end_matches('0').len())
        .unwrap_or(0)
}

pub fn snap_to_step(value: f64, min: f64, max: f64, step: f64) -> f64 {
    let stepped = if step > 0.0 {
        min + ((value - min) / step).round() * step
    } else {
        value
    };
    let factor = 10f64.powi(decimals(step) as i32);
    ((stepped.clamp(min, max)) * factor).round() / factor
}

/// 2 px track, accent fill up to the value, a 16 px knob outlined in accent and
/// filled with the window colour, and a mono readout on the right.
pub fn slider(ui: &mut Ui, value: &mut f64, min: f64, max: f64, step: f64) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, mut response) = ui.allocate_exact_size(vec2(WIDTH, HEIGHT), Sense::click_and_drag());
    let track = Rect::from_min_max(
        pos2(rect.left() + KNOB / 2.0, rect.center().y - 1.0),
        pos2(
            rect.right() - READOUT - GAP - KNOB / 2.0,
            rect.center().y + 1.0,
        ),
    );
    let span = (max - min).max(f64::EPSILON);

    let before = *value;
    if response.dragged() || response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let t = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
            *value = snap_to_step(min + t * span, min, max, step);
        }
    }
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        let delta = ui.input(|input| {
            let up =
                input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::ArrowUp);
            let down =
                input.key_pressed(egui::Key::ArrowLeft) || input.key_pressed(egui::Key::ArrowDown);
            up as i32 - down as i32
        });
        if delta != 0 {
            *value = snap_to_step(*value + delta as f64 * step, min, max, step);
        }
    }
    if (*value - before).abs() > f64::EPSILON {
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let t = ((*value - min) / span).clamp(0.0, 1.0) as f32;
        let knob_x = track.left() + track.width() * t;
        painter.rect_filled(track, 0, tokens.border);
        painter.rect_filled(track.with_max_x(knob_x), 0, tokens.accent);
        painter.circle(
            pos2(knob_x, track.center().y),
            KNOB / 2.0 - 0.5,
            tokens.window_bg,
            theme::stroke(ui.ctx(), 1.0, tokens.accent),
        );
        painter.text(
            pos2(rect.right(), rect.center().y),
            Align2::RIGHT_CENTER,
            format!("{:.*}", decimals(step), *value),
            theme::mono(12.0),
            tokens.text_secondary,
        );
        focus::ring(ui, &response, 4.0);
    }

    if ui.is_enabled() {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readout_uses_the_precision_of_the_step() {
        assert_eq!(decimals(0.05), 2);
        assert_eq!(decimals(0.5), 1);
        assert_eq!(decimals(1.0), 0);
        assert_eq!(decimals(10.0), 0);
    }

    #[test]
    fn values_snap_to_the_step_and_stay_in_range() {
        assert_eq!(snap_to_step(0.8537, 0.0, 1.0, 0.05), 0.85);
        assert_eq!(snap_to_step(0.30000001, 0.0, 1.0, 0.05), 0.3);
        assert_eq!(snap_to_step(1.2, 0.0, 1.0, 0.05), 1.0);
        assert_eq!(snap_to_step(-0.4, 0.0, 1.0, 0.05), 0.0);
    }
}
