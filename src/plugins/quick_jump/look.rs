//! What the strip and the notice share: their panel and the keycap.

use egui::Color32;

use crate::core::canvas::{Area, Canvas, Fonts};
use crate::core::layered::FILL_ALPHA;
use crate::core::theme::{self, Tokens};

/// Room around the panel for its shadow, in logical pixels.
pub const SHADOW: f32 = 16.0;

/// The panel's shadow, fill and 1 px rim, inset by the shadow's room.
pub fn panel(canvas: &mut Canvas, tokens: &Tokens, scale: f32) -> Area {
    let margin = (SHADOW * scale).round();
    let area = Area::new(
        margin,
        margin,
        canvas.width as f32 - 2.0 * margin,
        canvas.height as f32 - 2.0 * margin,
    );
    let radius = f32::from(tokens.radius_window) * scale;
    if tokens.shadows {
        let shadow = Area::new(area.x, area.y + 4.0 * scale, area.w, area.h);
        canvas.shadow(
            shadow,
            radius,
            margin,
            theme::with_alpha(Color32::BLACK, if tokens.dark { 0.45 } else { 0.18 }),
        );
    }
    canvas.fill(
        area,
        radius,
        theme::with_alpha(tokens.elevated_bg, FILL_ALPHA.max(0.96)),
    );
    canvas.outline(area, radius, border(tokens, scale), tokens.rim);
    area
}

/// A border's width: 1 px, 2 in High contrast, whole pixels.
pub fn border(tokens: &Tokens, scale: f32) -> f32 {
    (tokens.border_width * scale).round().max(1.0)
}

/// How a keycap looks: its font, height and colours.
pub struct Cap {
    pub font: usize,
    pub height: f32,
    pub text: Color32,
    pub edge: Color32,
    pub face: Color32,
    pub scale: f32,
}

impl Cap {
    pub fn new(tokens: &Tokens, font: usize, height: f32, scale: f32) -> Self {
        Self {
            font,
            height,
            text: tokens.text_secondary,
            edge: tokens.border,
            face: tokens.panel_bg,
            scale,
        }
    }
}

/// A keycap: a 1 px outline with a 2 px bottom edge, the key centred in it.
/// Returns its width.
pub fn keycap(canvas: &mut Canvas, fonts: &Fonts, key: &str, at: (f32, f32), cap: &Cap) -> f32 {
    let (x, y) = at;
    let (text_w, text_h) = fonts.measure(key, cap.font);
    let width = (text_w as f32 + 12.0 * cap.scale).max(cap.height).round();
    let radius = 4.0 * cap.scale;
    canvas.fill(Area::new(x, y, width, cap.height), radius, cap.edge);
    let line = cap.scale.round().max(1.0);
    let face = Area::new(
        x + line,
        y + line,
        width - 2.0 * line,
        cap.height - 3.0 * line,
    );
    canvas.fill(face, (radius - line).max(0.0), cap.face);
    let run = fonts.run(
        key,
        cap.font,
        cap.text,
        (x + (width - text_w as f32) / 2.0).round() as i32,
        (y + (cap.height - line - text_h as f32) / 2.0).round() as i32,
    );
    canvas.text(&[run], &fonts.list, 1.0);
    width
}
