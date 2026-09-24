use egui::{CornerRadius, Response, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};

fn keyboard_mode_id() -> egui::Id {
    egui::Id::new("wincraft-keyboard-navigation")
}

/// egui has no :focus-visible, so the window remembers how the user last
/// moved: a Tab or arrow key turns the ring on, any mouse press turns it off.
/// Called once per frame at the top of each window.
pub fn track(ctx: &egui::Context) {
    let (keys, pointer) = ctx.input(|input| {
        let keys = input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Tab
                        | egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight,
                    pressed: true,
                    ..
                }
            )
        });
        (keys, input.pointer.any_pressed())
    });
    if pointer {
        ctx.data_mut(|data| data.insert_temp(keyboard_mode_id(), false));
    } else if keys {
        ctx.data_mut(|data| data.insert_temp(keyboard_mode_id(), true));
    }
}

pub fn keyboard_active(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp(keyboard_mode_id()).unwrap_or(false))
}

/// The handout's focus ring: 2 px accent, 2 px outside the widget.
pub fn ring(ui: &Ui, response: &Response, radius: f32) {
    if response.has_focus() && keyboard_active(ui.ctx()) {
        paint(ui, response.rect, radius);
    }
}

pub fn paint(ui: &Ui, rect: egui::Rect, radius: f32) {
    let tokens = Tokens::get(ui.ctx());
    ui.painter().rect_stroke(
        rect.expand(2.0),
        CornerRadius::same((radius + 2.0).round() as u8),
        theme::stroke(ui.ctx(), 2.0, tokens.focus_ring),
        StrokeKind::Outside,
    );
}
