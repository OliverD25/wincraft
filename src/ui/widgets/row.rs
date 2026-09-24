use egui::{
    pos2, vec2, Align, Color32, CornerRadius, CursorIcon, Layout, Rect, Response, Sense, Shape, Ui,
    UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::{focus, text};

pub struct RowText<'a> {
    pub label: &'a str,
    pub meta: Option<&'a str>,
    pub desc: Option<(&'a str, Color32)>,
}

impl<'a> RowText<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            meta: None,
            desc: None,
        }
    }

    pub fn meta(mut self, meta: &'a str) -> Self {
        self.meta = Some(meta);
        self
    }

    pub fn desc(mut self, desc: &'a str, colour: Color32) -> Self {
        if !desc.is_empty() {
            self.desc = Some((desc, colour));
        }
        self
    }
}

const PADDING: f32 = 10.0;
const GAP: f32 = 24.0;
const CONTROL_GAP: f32 = 8.0;
const CONTROL_HEIGHT: f32 = 32.0;
const NARROW: f32 = 420.0;

#[derive(Clone, Copy, Default, PartialEq)]
struct Measured {
    controls_width: f32,
    left_height: f32,
}

fn left_column(ui: &mut Ui, row: &RowText) {
    let tokens = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.y = 2.0;
    let mut job = text::job(
        row.label,
        theme::regular(14.0),
        tokens.text_primary,
        Some(20.0),
    );
    if let Some(meta) = row.meta {
        job.append(
            meta,
            8.0,
            text::format(theme::mono(12.0), tokens.text_disabled, Some(20.0)),
        );
    }
    text::wrapped(ui, job);
    if let Some((desc, colour)) = row.desc {
        text::secondary(ui, desc, colour);
    }
}

/// A settings row: label, optional mono meta and description on the left,
/// controls on the right in the order they are added, at least 52 high, with
/// a 1 px rule under it. egui lays out in one pass, so the controls' width
/// and the text height are measured and remembered, and a change in either
/// redoes the frame before anything is shown.
pub fn settings_row(
    ui: &mut Ui,
    id_salt: &str,
    row: RowText,
    clickable: bool,
    controls: impl FnOnce(&mut Ui),
) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let id = ui.id().with(("settings-row", id_salt));
    let width = ui.available_width();
    let remembered: Measured = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let sense = if clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let background = ui.painter().add(Shape::Noop);

    let scoped = ui.scope_builder(UiBuilder::new().id_salt(id).sense(sense), |ui| {
        ui.spacing_mut().item_spacing = vec2(CONTROL_GAP, 0.0);
        ui.set_width(width);
        ui.add_space(PADDING);
        let top = ui.cursor().min;
        let mut measured = remembered;

        if width < NARROW {
            ui.scope(|ui| left_column(ui, &row));
            ui.add_space(8.0);
            ui.horizontal(controls);
        } else {
            let left_width = (width - remembered.controls_width - GAP).max(80.0);
            let content_height = remembered.left_height.max(CONTROL_HEIGHT);
            let left_top = top.y + (content_height - remembered.left_height) / 2.0;
            let left = ui.scope_builder(
                UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        pos2(top.x, left_top),
                        vec2(left_width, f32::INFINITY),
                    ))
                    .layout(Layout::top_down(Align::Min)),
                |ui| left_column(ui, &row),
            );
            measured.left_height = left.response.rect.height();

            let controls_rect = Rect::from_min_size(
                pos2(top.x + width - remembered.controls_width, top.y),
                vec2(remembered.controls_width.max(1.0), content_height),
            );
            let placed = ui.scope_builder(
                UiBuilder::new()
                    .max_rect(controls_rect)
                    .layout(Layout::left_to_right(Align::Center)),
                controls,
            );
            measured.controls_width = placed.response.rect.width();
            ui.advance_cursor_after_rect(Rect::from_min_size(top, vec2(width, content_height)));
        }

        if (measured.controls_width - remembered.controls_width).abs() > 0.5
            || (measured.left_height - remembered.left_height).abs() > 0.5
        {
            ui.data_mut(|data| data.insert_temp(id, measured));
            ui.ctx()
                .request_discard("a settings row measured its contents");
        }
        ui.add_space(PADDING);
    });

    let response = scoped.response;
    let rect = response.rect;
    let line = theme::stroke(ui.ctx(), 1.0, tokens.border);
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - line.width / 2.0, line);
    if clickable {
        let hover = ui
            .ctx()
            .animate_bool_with_time(id.with("hover"), response.hovered(), 0.08);
        ui.painter().set(
            background,
            egui::epaint::RectShape::filled(rect, 0, tokens.hover_bg.gamma_multiply(hover)),
        );
        focus::ring(ui, &response, 0.0);
        return response.on_hover_cursor(CursorIcon::PointingHand);
    }
    response
}

/// Selection is drawn from state, never from hover: the selection tint plus a
/// 2 px accent rule inset on the left edge.
pub fn paint_selection(ui: &Ui, rect: Rect) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    painter.rect_filled(rect, 4, tokens.selection_bg);
    let rule = theme::snap(2.0, ui.ctx().pixels_per_point());
    painter.rect_filled(
        Rect::from_min_max(rect.left_top(), pos2(rect.left() + rule, rect.bottom())),
        CornerRadius {
            nw: 1,
            sw: 1,
            ne: 0,
            se: 0,
        },
        tokens.accent,
    );
}

/// A navigation item: 36 high, 10 px from the left, accent text when
/// selected, the panel colour on hover.
pub fn nav_item(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
        if selected {
            paint_selection(ui, rect);
        } else if hover > 0.0 {
            ui.painter()
                .rect_filled(rect, 4, tokens.panel_bg.gamma_multiply(hover));
        }
        let colour = if selected {
            tokens.accent
        } else {
            tokens.text_primary
        };
        ui.painter().text(
            pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            theme::regular(14.0),
            colour,
        );
        focus::ring(ui, &response, 4.0);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}
