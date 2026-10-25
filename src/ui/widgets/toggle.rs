use egui::emath::easing;
use egui::{pos2, vec2, CursorIcon, Response, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::{focus, pending};

const WIDTH: f32 = 40.0;
const HEIGHT: f32 = 20.0;
const KNOB: f32 = 12.0;
const INSET: f32 = 3.0;

const SLIDE: f32 = 0.15;

/// 40×20 pill. The knob slides over 150 ms, easing out, and the colour moves
/// with it. A click shows its new value at once, even if the caller still
/// passes the old one for a few frames while the host catches up.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, mut response) = ui.allocate_exact_size(vec2(WIDTH, HEIGHT), Sense::click());
    *on = pending::shown(ui, response.id, *on);
    if response.clicked() {
        *on = !*on;
        pending::choose(ui, response.id, *on);
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        // A switch that was not drawn on the last frame (the window just
        // opened, or the page changed) starts at its value; only a change
        // while it is on screen slides.
        let pass = ui.ctx().cumulative_pass_nr();
        let drawn_id = response.id.with("drawn");
        let previous = ui.ctx().data_mut(|d| {
            let previous = d.get_temp::<u64>(drawn_id);
            d.insert_temp(drawn_id, pass);
            previous
        });
        let fresh = just_appeared(previous, pass);
        let position = ui.ctx().animate_bool_with_time_and_easing(
            response.id.with("knob"),
            *on,
            if fresh { 0.0 } else { SLIDE },
            easing::cubic_out,
        );
        let colour = tokens.text_disabled.lerp_to_gamma(tokens.accent, position);
        let painter = ui.painter();
        painter.rect(
            rect,
            10,
            egui::Color32::TRANSPARENT,
            theme::line(ui.ctx(), colour),
            StrokeKind::Inside,
        );
        let left = rect.left() + INSET + KNOB / 2.0;
        let right = rect.right() - INSET - KNOB / 2.0;
        let centre = pos2(left + (right - left) * position, rect.center().y);
        painter.circle_filled(centre, KNOB / 2.0, colour);
        focus::ring(ui, &response, 10.0);
    }

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if ui.is_enabled() {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Whether a switch last drawn on pass `previous` is new on pass `now`.
fn just_appeared(previous: Option<u64>, now: u64) -> bool {
    previous.is_none_or(|previous| now.saturating_sub(previous) > 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_slides_only_while_it_stays_on_screen() {
        assert!(just_appeared(None, 7));
        assert!(!just_appeared(Some(6), 7));
        assert!(just_appeared(Some(3), 7));
    }

    #[test]
    fn the_slide_starts_and_ends_exactly_and_moves_fast_first() {
        assert_eq!(easing::cubic_out(0.0), 0.0);
        assert_eq!(easing::cubic_out(1.0), 1.0);
        assert!(easing::cubic_out(0.5) > 0.5);
    }
}
