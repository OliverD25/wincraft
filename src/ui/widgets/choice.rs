use egui::{pos2, vec2, Color32, CornerRadius, CursorIcon, Rect, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::focus;
use crate::ui::widgets::icons::{self, Icon};

const HEIGHT: f32 = 32.0;
const OPTION_PADDING: f32 = 14.0;
const DROPDOWN_WIDTH: f32 = 200.0;

/// Up to four options read at a glance side by side; beyond that a row of
/// segments gets too wide, so the handout switches to a dropdown.
pub fn choice(
    ui: &mut Ui,
    id_salt: &str,
    options: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    if options.len() <= 4 {
        segmented(ui, options, selected)
    } else {
        dropdown(ui, id_salt, options, selected)
    }
}

/// Selected option: accent text and a 1 px accent stroke inset into its cell.
pub fn segmented(ui: &mut Ui, options: &[&str], selected: Option<usize>) -> Option<usize> {
    let tokens = Tokens::get(ui.ctx());
    let galleys: Vec<_> = options
        .iter()
        .map(|option| {
            ui.painter().layout_no_wrap(
                (*option).to_string(),
                theme::regular(13.0),
                Color32::PLACEHOLDER,
            )
        })
        .collect();
    let widths: Vec<f32> = galleys
        .iter()
        .map(|galley| galley.size().x + OPTION_PADDING * 2.0)
        .collect();
    let (outer, _) = ui.allocate_exact_size(vec2(widths.iter().sum(), HEIGHT), Sense::hover());
    let line = theme::stroke(ui.ctx(), 1.0, tokens.border);

    let mut chosen = None;
    let mut x = outer.left();
    let last = options.len().saturating_sub(1);
    for (index, galley) in galleys.into_iter().enumerate() {
        let cell = Rect::from_min_size(pos2(x, outer.top()), vec2(widths[index], HEIGHT));
        x += widths[index];
        let response = ui
            .interact(cell, ui.id().with(("segment", index)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        if response.clicked() && selected != Some(index) {
            chosen = Some(index);
        }
        let is_selected = selected == Some(index);
        let radius = CornerRadius {
            nw: if index == 0 { 4 } else { 0 },
            sw: if index == 0 { 4 } else { 0 },
            ne: if index == last { 4 } else { 0 },
            se: if index == last { 4 } else { 0 },
        };
        let painter = ui.painter();
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
        if !is_selected && hover > 0.0 {
            painter.rect_filled(cell, radius, tokens.hover_bg.gamma_multiply(hover));
        }
        if index > 0 {
            painter.vline(cell.left() + line.width / 2.0, cell.y_range(), line);
        }
        if is_selected {
            painter.rect_stroke(
                cell,
                radius,
                theme::stroke(ui.ctx(), 1.0, tokens.accent),
                StrokeKind::Inside,
            );
        }
        let colour = if is_selected {
            tokens.accent
        } else {
            tokens.text_secondary
        };
        let pos = cell.center() - galley.size() / 2.0;
        painter.galley(pos, galley, colour);
        focus::ring(ui, &response, 4.0);
    }
    ui.painter().rect_stroke(outer, 4, line, StrokeKind::Inside);
    chosen
}

/// 32 high, 200 wide, the current value in 14 regular and a chevron at the right.
pub fn dropdown(
    ui: &mut Ui,
    id_salt: &str,
    options: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(DROPDOWN_WIDTH, HEIGHT), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    let current = selected
        .and_then(|index| options.get(index))
        .copied()
        .unwrap_or("\u{2014}");

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
        painter.rect(
            rect,
            4,
            tokens.hover_bg.gamma_multiply(hover),
            theme::stroke(ui.ctx(), 1.0, tokens.border),
            StrokeKind::Inside,
        );
        painter.text(
            pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            current,
            theme::regular(14.0),
            tokens.text_primary,
        );
        let chevron = Rect::from_center_size(
            pos2(rect.right() - 10.0 - 7.0, rect.center().y),
            vec2(14.0, 14.0),
        );
        icons::paint(painter, chevron, Icon::ChevronDown, tokens.text_primary);
        focus::ring(ui, &response, 4.0);
    }

    let mut chosen = None;
    egui::Popup::menu(&response)
        .id(ui.id().with(("dropdown", id_salt)))
        .width(DROPDOWN_WIDTH)
        .show(|ui| {
            for (index, option) in options.iter().enumerate() {
                let (row, row_response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::click());
                let hover = ui.ctx().animate_bool_with_time(
                    row_response.id.with("hover"),
                    row_response.hovered(),
                    0.08,
                );
                let painter = ui.painter();
                painter.rect_filled(row, 4, tokens.hover_bg.gamma_multiply(hover));
                let colour = if selected == Some(index) {
                    tokens.accent
                } else {
                    tokens.text_primary
                };
                painter.text(
                    pos2(row.left() + 10.0, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    *option,
                    theme::regular(14.0),
                    colour,
                );
                if row_response.clicked() {
                    chosen = Some(index);
                }
            }
        });
    chosen
}
