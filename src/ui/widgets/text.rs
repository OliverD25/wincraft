use egui::text::LayoutJob;
use egui::{Color32, FontId, Label, Response, TextFormat, Ui};

use crate::core::theme::{self, Tokens};

pub fn format(font: FontId, colour: Color32, line_height: Option<f32>) -> TextFormat {
    TextFormat {
        font_id: font,
        color: colour,
        line_height,
        ..Default::default()
    }
}

pub fn job(text: &str, font: FontId, colour: Color32, line_height: Option<f32>) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, format(font, colour, line_height));
    job
}

pub fn wrapped(ui: &mut Ui, job: LayoutJob) -> Response {
    ui.add(Label::new(job).wrap())
}

pub fn single(ui: &mut Ui, job: LayoutJob) -> Response {
    ui.add(Label::new(job).extend())
}

/// Lora 12/17 for descriptions and verdicts.
pub fn secondary(ui: &mut Ui, text: &str, colour: Color32) -> Response {
    wrapped(ui, job(text, theme::lora(12.0), colour, Some(17.0)))
}

/// Lora 13 secondary, at most 560 wide: the line under every page title.
pub fn page_description(ui: &mut Ui, text: &str) {
    let tokens = Tokens::get(ui.ctx());
    let width = ui.available_width().min(560.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        wrapped(
            ui,
            job(text, theme::lora(13.0), tokens.text_secondary, Some(19.0)),
        );
    });
}

/// 11 px uppercase with 0.10 em tracking, in the disabled text colour.
pub fn section_job(text: &str, colour: Color32) -> LayoutJob {
    let mut format = format(theme::lora(11.0), colour, Some(15.0));
    format.extra_letter_spacing = 1.1;
    let mut job = LayoutJob::default();
    job.append(&text.to_uppercase(), 0.0, format);
    job
}

pub fn group_header(ui: &mut Ui, text: &str) -> Response {
    let tokens = Tokens::get(ui.ctx());
    single(ui, section_job(text, tokens.text_disabled))
}

/// Cormorant 30/33 page title, with the mono meta line set 10 px after it on
/// the same baseline when there is one.
pub fn title(ui: &mut Ui, text: &str, meta: Option<&str>) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let mut job = job(
        text,
        theme::cormorant(30.0),
        tokens.text_primary,
        Some(33.0),
    );
    if let Some(meta) = meta {
        // Its own line height and bottom alignment put the small mono text on
        // the title's baseline instead of at the top of the 33 px line.
        let mut format = format(theme::mono(12.0), tokens.text_disabled, None);
        format.valign = egui::Align::BOTTOM;
        job.append(meta, 10.0, format);
    }
    single(ui, job)
}

pub fn mono(ui: &mut Ui, text: &str, size: f32, colour: Color32) -> Response {
    single(ui, job(text, theme::mono(size), colour, None))
}

/// A 12 px accent link such as "‹ All plugins".
pub fn link(ui: &mut Ui, text: &str) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let colour = tokens.accent;
    let response = ui.add(
        Label::new(job(text, theme::lora(12.0), colour, Some(17.0)))
            .extend()
            .sense(egui::Sense::click()),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    super::focus::ring(ui, &response, 2.0);
    response
}

/// The status line's 6 px dot: filled for a known state, outlined while the
/// state is still unknown.
pub fn status_dot(ui: &mut Ui, colour: Color32, filled: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
    let painter = ui.painter();
    if filled {
        painter.circle_filled(rect.center(), 3.0, colour);
    } else {
        painter.circle_stroke(rect.center(), 2.5, theme::stroke(ui.ctx(), 1.0, colour));
    }
}
