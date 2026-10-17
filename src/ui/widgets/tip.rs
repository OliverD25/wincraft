use egui::{
    pos2, vec2, Align, Color32, CursorIcon, Layout, Rect, Sense, StrokeKind, Ui, UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::core::tips::Part;
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::{keycap, text};

pub const STRIP_HEIGHT: f32 = 32.0;
const CROSS_HIT: f32 = 24.0;

/// A line of a tip, left to right on the current line: plain words in
/// secondary text, keys as chips, what the person typed in the heavier
/// weight, literals in monospace. Nothing wraps; a line that is too long is
/// cut by the clip rectangle of the caller.
pub fn parts(ui: &mut Ui, line: &[Part], size: f32) {
    let tokens = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.x = 4.0;
    for part in line {
        match part {
            Part::Text(words) => {
                text::single(
                    ui,
                    text::job(words, theme::regular(size), tokens.text_secondary, None),
                );
            }
            Part::Key(keys) => {
                keycap::hint(ui, keys, theme::regular(12.0));
            }
            Part::Strong(words) => {
                text::single(
                    ui,
                    text::job(words, theme::semibold(size), tokens.text_primary, None),
                );
            }
            Part::Code(literal) => {
                text::single(
                    ui,
                    text::job(literal, theme::mono(size), tokens.text_primary, None),
                );
            }
        }
    }
}

/// The 32 px strip of a tip: a 1 px outline, the icon, the line and, when the
/// tip can be dismissed, a cross with a 24 px hit area. Returns true on the
/// frame the cross was clicked.
pub fn strip(
    ui: &mut Ui,
    rect: Rect,
    icon: Icon,
    icon_colour: Color32,
    line: &[Part],
    dismissable: bool,
) -> bool {
    let tokens = Tokens::get(ui.ctx());
    ui.painter().rect_stroke(
        rect,
        4,
        theme::stroke(ui.ctx(), 1.0, tokens.border),
        StrokeKind::Inside,
    );
    let mut dismissed = false;
    let mut right = rect.right() - 8.0;
    if dismissable {
        let hit = Rect::from_center_size(
            pos2(right - CROSS_HIT / 2.0, rect.center().y),
            vec2(CROSS_HIT, CROSS_HIT),
        );
        let response = ui
            .interact(hit, ui.id().with("tip-dismiss"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        if response.hovered() {
            ui.painter().rect_filled(hit, 4, tokens.hover_bg);
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(hit.center(), vec2(12.0, 12.0)),
            Icon::Close,
            tokens.text_secondary,
        );
        dismissed = response.clicked();
        right = hit.left() - 4.0;
    }
    let inner = Rect::from_min_max(
        pos2(rect.left() + 10.0, rect.top()),
        pos2(right, rect.bottom()),
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.set_clip_rect(inner.intersect(ui.clip_rect()));
            icons::show(ui, icon, 14.0, icon_colour);
            ui.add_space(4.0);
            parts(ui, line, 13.0);
        },
    );
    dismissed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeChoice;
    use crate::core::tips;

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeChoice::Dark);
        ctx
    }

    /// Runs a few frames over a 400 x 100 area and returns whether the cross
    /// was clicked in the last one. `click` presses and releases at a point
    /// on the second and third frame.
    fn run(dismissable: bool, click: Option<egui::Pos2>) -> bool {
        let ctx = context();
        let mut dismissed = false;
        for frame in 0..4 {
            let mut events = Vec::new();
            if let Some(at) = click {
                match frame {
                    1 => events.push(egui::Event::PointerMoved(at)),
                    2 => events.push(egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    }),
                    3 => events.push(egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    }),
                    _ => {}
                }
            }
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 100.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(380.0, STRIP_HEIGHT));
                let line = tips::bookmark_hint(tips::Place::Folder);
                let clicked = strip(ui, rect, Icon::Lightbulb, Color32::GRAY, &line, dismissable);
                dismissed |= clicked;
            });
            output.textures_delta.clear();
        }
        dismissed
    }

    #[test]
    fn the_cross_dismisses_only_when_it_is_clicked() {
        assert!(!run(true, None));
        // The cross is 8 px in from the right edge of the strip at x = 390.
        assert!(run(true, Some(pos2(390.0 - 8.0 - 12.0, 26.0))));
        assert!(
            !run(true, Some(pos2(60.0, 26.0))),
            "the text is not a button"
        );
    }

    #[test]
    fn a_strip_without_a_cross_cannot_be_dismissed() {
        assert!(!run(false, Some(pos2(390.0 - 8.0 - 12.0, 26.0))));
    }

    #[test]
    fn every_kind_of_part_draws() {
        let ctx = context();
        let line = vec![
            Part::text("one"),
            Part::key("Ctrl + B"),
            Part::Strong("two".to_string()),
            Part::code("'"),
        ];
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| parts(ui, &line, 13.0));
        output.textures_delta.clear();
    }
}
