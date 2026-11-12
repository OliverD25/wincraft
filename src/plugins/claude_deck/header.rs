//! The 26 px header, as on the design page: the counts of the statuses
//! that have chats, the "Only open windows" switch, the quick settings
//! gear, the pin for stay on top, the mini mode button and the close
//! button. The rest of it drags the window.

use egui::{pos2, vec2, Color32, CursorIcon, Painter, Rect, Sense, Shape, Stroke, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::marks;
use super::model::Mark;
use super::quick::{self, Quick};
use super::{MarkStyle, Settings};

pub const HEIGHT: f32 = 26.0;
const PAD_LEFT: f32 = 10.0;
const PAD_RIGHT: f32 = 4.0;
const GAP: f32 = 8.0;
const COUNT_GAP: f32 = 3.0;
const BUTTON: egui::Vec2 = egui::Vec2::new(22.0, 20.0);
const SWITCH: egui::Vec2 = egui::Vec2::new(24.0, 13.0);
const SMALL: f32 = 12.0;

pub const ONLY_OPEN: &str = "Only open windows";
const PIN_TIP: &str = "Stay on top";
pub const MINI_TIP: &str = "Mini mode";
const CLOSE_TIP: &str = "Close";
pub const MINIMIZE_TIP: &str = "Minimize";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    OnlyOpen,
    Pin,
    Mini,
    Minimize,
    Close,
}

/// Draws the header in `rect` and says what was clicked. A drag that
/// starts on it moves the window.
pub fn show(
    ui: &mut Ui,
    rect: Rect,
    counts: &[(Mark, usize)],
    settings: &Settings,
    tokens: &Tokens,
    turn: f32,
    quick: &mut Quick,
) -> Option<Click> {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, tokens.panel_bg);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        theme::line(ui.ctx(), tokens.border),
    );
    let drag = ui.interact(rect, ui.id().with("claude-deck-header"), Sense::drag());
    if drag.drag_started() && !super::edges::pressed_on_edge(ui.ctx()) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let middle = rect.center().y;
    let button =
        |right: f32| Rect::from_min_size(pos2(right - BUTTON.x, middle - BUTTON.y / 2.0), BUTTON);
    let close = button(rect.right() - PAD_RIGHT);
    let minimize = button(close.left() - COUNT_GAP);
    let mini = button(minimize.left() - COUNT_GAP);
    let pin = button(mini.left() - COUNT_GAP);
    let gear = button(pin.left() - COUNT_GAP);
    let label = painter.layout_job(text::job(
        ONLY_OPEN,
        theme::regular(SMALL),
        tokens.text_secondary,
        None,
    ));
    let numbers: Vec<_> = counts
        .iter()
        .map(|(_, count)| {
            painter.layout_job(text::job(
                &count.to_string(),
                theme::regular(SMALL),
                tokens.text_primary,
                None,
            ))
        })
        .collect();
    let width = marks::width(settings.mark).max(marks::SIZE);
    let needed: f32 = numbers
        .iter()
        .map(|number| width + COUNT_GAP + number.size().x + GAP)
        .sum();
    // The counts come first: when they do not all fit, the switch's label
    // goes and its tooltip names it.
    let with_label = gear.left() - GAP - label.size().x - GAP - SWITCH.x;
    let label_rect = (rect.left() + PAD_LEFT + needed <= with_label).then(|| {
        Rect::from_min_size(
            pos2(
                gear.left() - GAP - label.size().x,
                middle - label.size().y / 2.0,
            ),
            label.size(),
        )
    });
    let switch_right = label_rect.map_or(gear.left(), |label| label.left());
    let switch = Rect::from_min_size(
        pos2(
            (switch_right - GAP - SWITCH.x).round(),
            (middle - SWITCH.y / 2.0).round(),
        ),
        SWITCH,
    );

    let mut x = rect.left() + PAD_LEFT;
    for ((mark, _), number) in counts.iter().zip(numbers) {
        if x + width + COUNT_GAP + number.size().x > switch.left() - GAP {
            break;
        }
        let style = match settings.mark {
            MarkStyle::Off => MarkStyle::Shapes,
            style => style,
        };
        let used = marks::paint(
            &painter,
            pos2(x, middle - marks::SIZE / 2.0),
            *mark,
            style,
            tokens,
            turn,
        );
        x += used + COUNT_GAP;
        let size = number.size();
        painter.galley(pos2(x, middle - size.y / 2.0), number, Color32::PLACEHOLDER);
        x += size.x + GAP;
    }

    let mut clicked = None;
    let mut toggle = ui
        .interact(
            label_rect.map_or(switch, |label| switch.union(label)),
            ui.id().with("claude-deck-only-open"),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    if label_rect.is_none() {
        toggle = toggle.on_hover_text(ONLY_OPEN);
    }
    if toggle.clicked() {
        clicked = Some(Click::OnlyOpen);
    }
    paint_switch(&painter, switch, settings.only_open, tokens);
    if let Some(at) = label_rect {
        painter.galley(at.min, label, Color32::PLACEHOLDER);
    }

    quick.gear = Some(gear);
    let gear_response = ui
        .interact(gear, ui.id().with("claude-deck-quick"), Sense::click())
        .on_hover_text(format!("{} ({})", quick::TIP, quick::KEY));
    if quick.is_open(ui.ctx()) {
        painter.rect_filled(gear, 4.0, tokens.selection_bg);
    } else if gear_response.hovered() {
        painter.rect_filled(gear, 4.0, tokens.hover_bg);
    }
    paint_gear(&painter, gear, tokens);
    if gear_response.clicked() {
        quick.toggle = true;
    }

    let pinned = settings.stay_on_top;
    let pin_response = ui
        .interact(pin, ui.id().with("claude-deck-pin"), Sense::click())
        .on_hover_text(PIN_TIP);
    if pinned {
        painter.rect_filled(pin, 4.0, tokens.selection_bg);
    } else if pin_response.hovered() {
        painter.rect_filled(pin, 4.0, tokens.hover_bg);
    }
    paint_pin(&painter, pin, pinned, tokens);
    if pin_response.clicked() {
        clicked = Some(Click::Pin);
    }

    let mini_response = ui
        .interact(mini, ui.id().with("claude-deck-mini"), Sense::click())
        .on_hover_text(MINI_TIP);
    if mini_response.hovered() {
        painter.rect_filled(mini, 4.0, tokens.hover_bg);
    }
    let m = mini.center() - vec2(6.0, 6.0);
    let at = |x: f32, y: f32| m + vec2(x, y);
    let stroke = Stroke::new(1.2, tokens.text_secondary);
    painter.line_segment([at(7.5, 2.0), at(7.5, 10.0)], stroke);
    painter.line_segment([at(4.5, 4.0), at(2.5, 6.0)], stroke);
    painter.line_segment([at(2.5, 6.0), at(4.5, 8.0)], stroke);
    if mini_response.clicked() {
        clicked = Some(Click::Mini);
    }

    let minimize_response = ui
        .interact(
            minimize,
            ui.id().with("claude-deck-minimize"),
            Sense::click(),
        )
        .on_hover_text(MINIMIZE_TIP);
    if minimize_response.hovered() {
        painter.rect_filled(minimize, 4.0, tokens.hover_bg);
    }
    let c = minimize.center();
    painter.line_segment(
        [c + vec2(-3.5, 0.5), c + vec2(3.5, 0.5)],
        Stroke::new(1.2, tokens.text_secondary),
    );
    if minimize_response.clicked() {
        clicked = Some(Click::Minimize);
    }

    let close_response = ui
        .interact(close, ui.id().with("claude-deck-close"), Sense::click())
        .on_hover_text(CLOSE_TIP);
    if close_response.hovered() {
        painter.rect_filled(close, 4.0, tokens.hover_bg);
    }
    let c = close.center();
    let stroke = Stroke::new(1.2, tokens.text_secondary);
    painter.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], stroke);
    painter.line_segment([c + vec2(3.0, -3.0), c + vec2(-3.0, 3.0)], stroke);
    if close_response.clicked() {
        clicked = Some(Click::Close);
    }
    clicked
}

/// The small switch of the design: 24 x 13, the knob on the right and the
/// accent fill when on.
pub(super) fn paint_switch(painter: &Painter, rect: Rect, on: bool, tokens: &Tokens) {
    let (fill, border, knob) = if on {
        (tokens.accent, tokens.accent, tokens.window_bg)
    } else {
        (
            Color32::TRANSPARENT,
            tokens.text_secondary,
            tokens.text_secondary,
        )
    };
    painter.rect(
        rect,
        rect.height() / 2.0,
        fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    painter.circle_filled(knob_centre(rect, on), KNOB_RADIUS, knob);
}

const KNOB_RADIUS: f32 = 4.0;

/// The knob sits on the track's middle line, with the same gap above,
/// below and at its end, on either side.
fn knob_centre(rect: Rect, on: bool) -> egui::Pos2 {
    let inset = rect.height() / 2.0;
    let x = if on {
        rect.right() - inset
    } else {
        rect.left() + inset
    };
    pos2(x, rect.center().y)
}

/// The gear of quick settings: a ring with six teeth.
fn paint_gear(painter: &Painter, rect: Rect, tokens: &Tokens) {
    let c = rect.center();
    let stroke = Stroke::new(1.2, tokens.text_secondary);
    painter.circle_stroke(c, 3.6, stroke);
    painter.circle_filled(c, 1.2, tokens.text_secondary);
    for tooth in 0..6 {
        let angle = std::f32::consts::TAU * tooth as f32 / 6.0;
        let way = vec2(angle.cos(), angle.sin());
        painter.line_segment(
            [c + way * 3.6, c + way * 5.8],
            Stroke::new(1.8, tokens.text_secondary),
        );
    }
}

/// The pin, filled in the accent while the window stays on top.
fn paint_pin(painter: &Painter, rect: Rect, on: bool, tokens: &Tokens) {
    let origin = rect.center() - vec2(6.0, 6.0);
    let at = |x: f32, y: f32| origin + vec2(x, y);
    let colour = if on {
        tokens.accent
    } else {
        tokens.text_secondary
    };
    let head = vec![
        at(4.0, 1.0),
        at(8.0, 1.0),
        at(7.4, 4.5),
        at(9.5, 6.5),
        at(2.5, 6.5),
        at(4.6, 4.5),
    ];
    let stroke = Stroke::new(1.1, colour);
    if on {
        // Two convex halves: egui fills only convex shapes.
        let top = vec![head[0], head[1], head[2], head[5]];
        let bottom = vec![head[5], head[2], head[3], head[4]];
        painter.add(Shape::convex_polygon(top, colour, Stroke::NONE));
        painter.add(Shape::convex_polygon(bottom, colour, Stroke::NONE));
    }
    painter.add(Shape::closed_line(head, stroke));
    painter.line_segment([at(6.0, 6.5), at(6.0, 11.0)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_knob_sits_on_the_middle_line_with_even_gaps() {
        let track = Rect::from_min_size(pos2(10.0, 20.0), SWITCH);
        for on in [false, true] {
            let centre = knob_centre(track, on);
            assert_eq!(centre.y, track.center().y, "on: {on}");
            let side_gap = if on {
                track.right() - (centre.x + KNOB_RADIUS)
            } else {
                centre.x - KNOB_RADIUS - track.left()
            };
            let top_gap = centre.y - KNOB_RADIUS - track.top();
            assert_eq!(side_gap, top_gap, "on: {on}");
        }
    }
}
