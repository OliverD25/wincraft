//! Pictures for the options of a choice setting (`Sketch` in the plugin
//! interface): window shapes in the theme's colours, so they suit every
//! theme, light and dark.

use egui::{pos2, vec2, Color32, CursorIcon, Painter, Rect, Response, Sense, StrokeKind, Ui, Vec2};

use crate::core::theme::{self, Tokens};
use crate::core::traits::{Sketch, SketchLook};

const BOX: Vec2 = Vec2::new(120.0, 60.0);
const PAD: f32 = 8.0;
const CAPTION_GAP: f32 = 6.0;
const TILE_GAP: f32 = 12.0;
const SHAPE_RADIUS: u8 = 2;
/// How strongly a picture that is not chosen shows.
const NOT_CHOSEN: f32 = 0.55;

/// One tile per option, side by side, wrapping on a narrow page: the
/// picture over the option's name. The chosen tile has the accent outline.
/// A click on another tile returns its index; the switch beside the row
/// stays the keyboard's way to choose.
pub fn options(
    ui: &mut Ui,
    sketches: &[Sketch],
    names: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    let mut picked = None;
    // Every tile is as tall as the one with the longest name, so a name on
    // two lines does not put the tiles out of line.
    let captions = names
        .iter()
        .map(|name| {
            ui.painter()
                .layout(
                    name.to_string(),
                    theme::regular(12.0),
                    Color32::PLACEHOLDER,
                    BOX.x,
                )
                .size()
                .y
        })
        .fold(0.0, f32::max);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(TILE_GAP, TILE_GAP);
        for (at, (sketch, name)) in sketches.iter().zip(names).enumerate() {
            let chosen = selected == Some(at);
            if tile(ui, sketch, name, chosen, captions).clicked() && !chosen {
                picked = Some(at);
            }
        }
    });
    picked
}

fn tile(ui: &mut Ui, sketch: &Sketch, name: &str, chosen: bool, captions: f32) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let colour = if chosen {
        tokens.accent
    } else {
        tokens.text_secondary
    };
    let caption = ui
        .painter()
        .layout(name.to_string(), theme::regular(12.0), colour, BOX.x);
    let size = vec2(
        BOX.x + 2.0 * PAD,
        PAD + BOX.y + CAPTION_GAP + captions + PAD,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let fill = if response.hovered() && !chosen {
        tokens.hover_bg
    } else {
        Color32::TRANSPARENT
    };
    let edge = if chosen { tokens.accent } else { tokens.border };
    ui.painter().rect(
        rect,
        tokens.radius_row,
        fill,
        theme::line(ui.ctx(), edge),
        StrokeKind::Inside,
    );
    let picture = Rect::from_min_size(rect.min + vec2(PAD, PAD), BOX);
    let strength = if chosen { 1.0 } else { NOT_CHOSEN };
    paint(ui, &tokens, sketch, picture, strength);
    ui.painter().galley(
        pos2(picture.left(), picture.bottom() + CAPTION_GAP),
        caption,
        colour,
    );
    response
}

/// Draws the shapes of `sketch` into `area` (120 x 60), at `strength`
/// (1 is full colour).
pub fn paint(ui: &Ui, tokens: &Tokens, sketch: &Sketch, area: Rect, strength: f32) {
    let painter: &Painter = ui.painter();
    let ctx = ui.ctx();
    for shape in sketch.shapes {
        let [x, y, w, h] = shape.rect;
        let rect = Rect::from_min_size(area.min + vec2(x, y), vec2(w, h));
        let (fill, edge, width) = match shape.look {
            SketchLook::Panel => (tokens.elevated_bg, tokens.border, 1.0),
            SketchLook::Faint => (
                tokens.text_disabled.gamma_multiply(0.35),
                Color32::TRANSPARENT,
                0.0,
            ),
            SketchLook::Window => (tokens.window_bg, tokens.text_secondary, 1.0),
            SketchLook::Selected => (tokens.window_bg, tokens.accent, 2.0),
            SketchLook::Text => (tokens.text_disabled, Color32::TRANSPARENT, 0.0),
        };
        let fade = |colour: Color32| {
            if strength >= 1.0 {
                colour
            } else {
                colour.gamma_multiply(strength)
            }
        };
        // A shape that covers another stays solid, so the one behind does
        // not show through; only its edge fades.
        let fill = match shape.look {
            SketchLook::Faint | SketchLook::Text => fade(fill),
            _ => fill,
        };
        painter.rect(
            rect,
            SHAPE_RADIUS,
            fill,
            theme::stroke(ctx, width, fade(edge)),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{ThemeChoice, ThemeName};
    use crate::core::traits::SketchShape;

    const TWO: &[SketchShape] = &[
        SketchShape {
            rect: [0.0, 0.0, 120.0, 60.0],
            look: SketchLook::Panel,
        },
        SketchShape {
            rect: [10.0, 10.0, 30.0, 20.0],
            look: SketchLook::Selected,
        },
    ];

    #[test]
    fn the_tiles_draw_in_every_theme_and_a_click_is_only_on_another_option() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let sketches = [Sketch { shapes: TWO }, Sketch { shapes: TWO }];
        for name in ThemeName::ALL {
            for mode in [ThemeChoice::Dark, ThemeChoice::Light] {
                theme::apply(&ctx, name, mode);
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let picked = options(ui, &sketches, &["A row", "A fan"], Some(1));
                    assert_eq!(picked, None);
                });
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn tiles_wrap_on_a_narrow_page() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let sketches = [Sketch { shapes: TWO }, Sketch { shapes: TWO }];
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 400.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let top = ui.cursor().top();
            options(
                ui,
                &sketches,
                &["A row centred on the stack", "A fan"],
                None,
            );
            let height = ui.min_rect().bottom() - top;
            assert!(height > 2.0 * BOX.y, "two tiles on two lines, got {height}");
            assert!(ui.min_rect().width() <= 200.0);
        });
        output.textures_delta.clear();
    }
}
