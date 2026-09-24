use std::time::Duration;

use egui::{pos2, vec2, Color32, Rect, Sense, StrokeKind, Ui};

use crate::core::hotkeys;
use crate::core::theme::{self, Tokens};
use crate::core::traits::Hotkey;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::{focus, hotkey_capture, keycap, text};

#[derive(Clone, Debug)]
pub struct Verdict {
    pub text: String,
    pub colour: Color32,
}

/// One capture at a time: which hotkey is being rebound, and the combination
/// pressed so far with its verdict.
#[derive(Clone, Debug)]
pub struct Session {
    pub owner: String,
    pub pending: Option<(Hotkey, Verdict)>,
}

pub enum Outcome {
    Apply(Hotkey),
    Cancel,
}

const BOX_WIDTH: f32 = 240.0;
const BOX_HEIGHT: f32 = 32.0;

/// The box, the verdict line under it and Apply / Cancel. The low-level hook
/// swallows every key while the session is open, so Esc reaches us through the
/// hook rather than through egui.
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    verdict_for: impl Fn(Hotkey) -> Verdict,
) -> Option<Outcome> {
    let tokens = Tokens::get(ui.ctx());
    if let Some(hotkey) = hotkey_capture::take_result() {
        session.pending = Some((hotkey, verdict_for(hotkey)));
    }
    if hotkey_capture::take_cancelled() {
        return Some(Outcome::Cancel);
    }
    // The hook swallows keys system-wide, so it must not outlive the window's
    // focus: a user who clicks Change and then switches to another app would
    // otherwise find their typing disappearing.
    if ui.ctx().input(|input| input.viewport().focused) == Some(false) {
        return Some(Outcome::Cancel);
    }
    // The hook runs between frames; without a timed repaint the held
    // modifiers would only appear when the mouse next moves.
    ui.ctx().request_repaint_after(Duration::from_millis(30));

    let mut outcome = None;
    ui.add_space(10.0);
    ui.spacing_mut().item_spacing.y = 6.0;

    let (rect, _) = ui.allocate_exact_size(vec2(BOX_WIDTH, BOX_HEIGHT), Sense::hover());
    ui.painter().rect_stroke(
        rect,
        4,
        theme::stroke(ui.ctx(), 1.0, tokens.accent),
        StrokeKind::Inside,
    );
    focus::paint(ui, rect, 4.0);
    let inner = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), rect.max);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            match &session.pending {
                Some((hotkey, _)) => {
                    keycap::chips(ui, &hotkeys::format(*hotkey), 12.0, false);
                }
                None => {
                    let held = hotkeys::format_modifiers(hotkey_capture::live_modifiers());
                    if !held.is_empty() {
                        keycap::chips(ui, &held, 12.0, true);
                        ui.add_space(4.0);
                    }
                    let mut hint = text::job(
                        "Press keys\u{2026}",
                        theme::lora(12.0),
                        tokens.text_disabled,
                        None,
                    );
                    if let Some(section) = hint.sections.first_mut() {
                        section.format.italics = true;
                    }
                    text::single(ui, hint);
                }
            }
        },
    );

    if let Some((_, verdict)) = &session.pending {
        text::secondary(ui, &verdict.text, verdict.colour);
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let ready = session.pending.is_some();
        let apply = ui.add_enabled_ui(ready, |ui| button::button(ui, "Apply", Kind::Primary));
        if apply.inner.clicked() {
            if let Some((hotkey, _)) = &session.pending {
                outcome = Some(Outcome::Apply(*hotkey));
            }
        }
        if button::button(ui, "Cancel", Kind::Secondary).clicked() {
            outcome = Some(Outcome::Cancel);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            text::secondary(ui, "Esc cancels", tokens.text_disabled);
        });
    });
    ui.add_space(10.0);
    outcome
}
