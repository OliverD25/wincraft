use egui::{vec2, Color32, CursorIcon, Response, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::focus;
use crate::ui::widgets::icons::{self, Icon};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Danger,
}

const HEIGHT: f32 = 32.0;
const SMALL_HEIGHT: f32 = 26.0;
const PADDING: f32 = 12.0;
const ICON: f32 = 13.0;
const ICON_GAP: f32 = 6.0;

pub fn button(ui: &mut Ui, label: &str, kind: Kind) -> Response {
    with_icon(ui, label, kind, None)
}

/// The button at the right end of a tile: 26 high, 12.5 px text.
pub fn small(ui: &mut Ui, label: &str, kind: Kind) -> Response {
    draw(ui, label, kind, None, SMALL_HEIGHT, 12.5)
}

/// Outlined, never filled: colour lives in the stroke and the label, and the
/// only fills are the translucent hover and pressed tints.
pub fn with_icon(ui: &mut Ui, label: &str, kind: Kind, icon: Option<Icon>) -> Response {
    draw(ui, label, kind, icon, HEIGHT, 14.0)
}

fn draw(
    ui: &mut Ui,
    label: &str,
    kind: Kind,
    icon: Option<Icon>,
    height: f32,
    font_size: f32,
) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        theme::regular(font_size),
        Color32::PLACEHOLDER,
    );
    let icon_width = if icon.is_some() { ICON + ICON_GAP } else { 0.0 };
    let size = vec2(PADDING * 2.0 + icon_width + galley.size().x, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hover = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            enabled && response.hovered(),
            0.08,
        );
        let pressed = enabled && response.is_pointer_button_down_on();

        let (mut stroke, mut text, mut fill) = match kind {
            Kind::Primary => (
                tokens.accent,
                tokens.accent,
                tokens.accent_tint.gamma_multiply(hover),
            ),
            Kind::Secondary => (
                tokens.border,
                tokens.text_primary,
                tokens.hover_bg.gamma_multiply(hover),
            ),
            Kind::Danger => (
                tokens.danger,
                tokens.danger,
                tokens.hover_bg.gamma_multiply(hover),
            ),
        };
        if pressed {
            fill = tokens.selection_bg;
            if kind == Kind::Primary {
                stroke = tokens.accent_pressed;
                text = tokens.accent_pressed;
            }
        }

        let painter = ui.painter();
        painter.rect(
            rect,
            tokens.radius_row,
            fill,
            theme::line(ui.ctx(), stroke),
            StrokeKind::Inside,
        );
        let mut x = rect.left() + PADDING;
        if let Some(icon) = icon {
            let icon_rect = egui::Rect::from_min_size(
                egui::pos2(x, rect.center().y - ICON / 2.0),
                vec2(ICON, ICON),
            );
            icons::paint(painter, icon_rect, icon, text);
            x += ICON + ICON_GAP;
        }
        let text_pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, text);
        focus::ring(ui, &response, f32::from(tokens.radius_row));
    }

    if ui.is_enabled() {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}
