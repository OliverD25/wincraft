//! The 26 px header, as on the design page: the counts of the statuses
//! that have chats, the "Only open windows" switch, the pin for stay on top,
//! the mini mode button and the close button. The rest of it drags the
//! window.

use egui::{pos2, vec2, Color32, CursorIcon, Painter, Rect, Sense, Shape, Stroke, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::marks;
use super::model::Mark;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    OnlyOpen,
    Pin,
    Mini,
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
) -> Option<Click> {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, tokens.panel_bg);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        theme::line(ui.ctx(), tokens.border),
    );
    let drag = ui.interact(rect, ui.id().with("claude-deck-header"), Sense::drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let middle = rect.center().y;
    let button =
        |right: f32| Rect::from_min_size(pos2(right - BUTTON.x, middle - BUTTON.y / 2.0), BUTTON);
    let close = button(rect.right() - PAD_RIGHT);
    let mini = button(close.left() - COUNT_GAP);
    let pin = button(mini.left() - COUNT_GAP);
    let label = painter.layout_job(text::job(
        ONLY_OPEN,
        theme::regular(SMALL),
        tokens.text_secondary,
        None,
    ));
    let label_rect = Rect::from_min_size(
        pos2(
            pin.left() - GAP - label.size().x,
            middle - label.size().y / 2.0,
        ),
        label.size(),
    );
    let switch = Rect::from_min_size(
        pos2(label_rect.left() - GAP - SWITCH.x, middle - SWITCH.y / 2.0),
        SWITCH,
    );

    let mut x = rect.left() + PAD_LEFT;
    for (mark, count) in counts {
        let width = marks::width(settings.mark).max(marks::SIZE);
        let number = painter.layout_job(text::job(
            &count.to_string(),
            theme::regular(SMALL),
            tokens.text_primary,
            None,
        ));
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
    let toggle = ui
        .interact(
            switch.union(label_rect),
            ui.id().with("claude-deck-only-open"),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    if toggle.clicked() {
        clicked = Some(Click::OnlyOpen);
    }
    paint_switch(&painter, switch, settings.only_open, tokens);
    painter.galley(label_rect.min, label, Color32::PLACEHOLDER);

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
fn paint_switch(painter: &Painter, rect: Rect, on: bool, tokens: &Tokens) {
    let (fill, border, knob, knob_left) = if on {
        (tokens.accent, tokens.accent, tokens.window_bg, 13.0)
    } else {
        (
            Color32::TRANSPARENT,
            tokens.text_secondary,
            tokens.text_secondary,
            2.0,
        )
    };
    painter.rect(
        rect,
        rect.height() / 2.0,
        fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    painter.circle_filled(
        pos2(rect.left() + knob_left + 3.5, rect.top() + 2.0 + 3.5),
        3.5,
        knob,
    );
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
