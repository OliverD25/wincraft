//! Paints the hint. No design exists yet, so this is the plainest version
//! from the theme tokens: the glow in `accent`, the pill in the notices'
//! panel look (`core::panel_look`). Sizes are physical pixels; `s` is the
//! monitor's scale.

use egui::Color32;

use crate::core::canvas::{Area, Canvas, Fonts, Weight};
use crate::core::layered::Image;
use crate::core::panel_look as look;
use crate::core::theme::{self, Tokens};

use super::{arrow_first, Dir, CORNER, GLOW, LINE};

/// How strong the soft part is next to the line.
const GLOW_ALPHA: f32 = 0.55;
const PILL_HEIGHT: f32 = 40.0;
const PILL_PAD_X: f32 = 16.0;
const CAPTION_GAP: f32 = 10.0;
const ARROW_GAP: f32 = 6.0;
/// The words are cut to fit this.
const WORDS_MAX: f32 = 420.0;

const CAPTION_FONT: usize = 0;
const WORDS_FONT: usize = 1;
const ARROW_FONT: usize = 2;

/// How far the glow reaches out from the frame, whole physical pixels.
pub fn reach(s: f32) -> i32 {
    ((LINE + GLOW) * s).ceil() as i32 + 1
}

/// The glow for a box `size` big, with the window's frame at `frame` in
/// it. Only the pixels near the edge are worked out, so a maximised
/// window's full-screen box stays quick.
pub fn glow(size: (i32, i32), frame: Area, inside: bool, tokens: &Tokens, s: f32) -> Image {
    let mut canvas = Canvas::new(size.0, size.1);
    let radius = if inside { 0.0 } else { CORNER * s };
    let line = (LINE * s).max(1.0);
    let soft = GLOW * s;
    let band = line + soft + 2.0 + radius;
    let colour = tokens.accent;
    let near = |value: f32, edge: f32| (value - edge).abs() <= band;
    for y in 0..canvas.height {
        let py = y as f32 + 0.5;
        let whole_row = near(py, frame.y) || near(py, frame.bottom());
        let paint = |canvas: &mut Canvas, x: i32| {
            let px = x as f32 + 0.5;
            let distance = frame.distance(px, py, radius);
            let out = if inside { -distance } else { distance };
            if out > line {
                let fade = (1.0 - (out - line) / soft.max(1.0)).clamp(0.0, 1.0);
                canvas.blend(x, y, colour, fade * fade * GLOW_ALPHA);
            }
            let cover = (0.5 + out)
                .clamp(0.0, 1.0)
                .min((line + 0.5 - out).clamp(0.0, 1.0));
            canvas.blend(x, y, colour, cover);
        };
        if whole_row {
            for x in 0..canvas.width {
                paint(&mut canvas, x);
            }
            continue;
        }
        let columns = |edge: f32| {
            let low = ((edge - band).floor() as i32).max(0);
            let high = ((edge + band).ceil() as i32).min(canvas.width);
            low..high
        };
        let (left, right) = (columns(frame.x), columns(frame.right()));
        let right = right.start.max(left.end)..right.end;
        for x in left.chain(right) {
            paint(&mut canvas, x);
        }
    }
    canvas.into_image()
}

/// The pill: "WinCraft" small and quiet (Principle 1: the user knows who
/// is talking), then the words with the arrow on the side of the other
/// monitor. Returns the picture and where the panel sits in it (the rest
/// is room for its shadow).
pub fn pill(words: &str, dir: Dir, tokens: &Tokens, s: f32) -> (Image, Area) {
    let px = |size: f32| (size * s).round() as i32;
    let fonts = Fonts::new(&[
        (Weight::Regular, px(12.0)),
        (Weight::Regular, px(14.0)),
        (Weight::Semibold, px(14.0)),
    ]);
    let caption = "WinCraft";
    let words = fonts.fit(words, WORDS_FONT, px(WORDS_MAX));
    let (caption_w, _) = fonts.measure(caption, CAPTION_FONT);
    let (words_w, _) = fonts.measure(&words, WORDS_FONT);
    let (arrow_w, _) = fonts.measure(dir.arrow(), ARROW_FONT);
    let inner =
        caption_w as f32 + CAPTION_GAP * s + words_w as f32 + ARROW_GAP * s + arrow_w as f32;
    let margin = (look::SHADOW * s).round();
    let panel_w = (inner + 2.0 * PILL_PAD_X * s).round();
    let panel_h = (PILL_HEIGHT * s).round();
    let mut canvas = Canvas::new(
        (panel_w + 2.0 * margin) as i32,
        (panel_h + 2.0 * margin) as i32,
    );
    let panel = look::panel(&mut canvas, tokens, s);
    let middle = panel.y + panel.h / 2.0;
    let mut x = panel.x + PILL_PAD_X * s;
    let put = |canvas: &mut Canvas, text: &str, font: usize, colour: Color32, x: f32| {
        let (width, height) = fonts.measure(text, font);
        let run = fonts.run(
            text,
            font,
            colour,
            x.round() as i32,
            (middle - height as f32 / 2.0).round() as i32,
        );
        canvas.text(&[run], &fonts.list, 1.0);
        width as f32
    };
    x += put(&mut canvas, caption, CAPTION_FONT, tokens.text_disabled, x) + CAPTION_GAP * s;
    if arrow_first(dir) {
        x += put(&mut canvas, dir.arrow(), ARROW_FONT, tokens.accent, x) + ARROW_GAP * s;
        put(&mut canvas, &words, WORDS_FONT, tokens.text_primary, x);
    } else {
        x += put(&mut canvas, &words, WORDS_FONT, tokens.text_primary, x) + ARROW_GAP * s;
        put(&mut canvas, dir.arrow(), ARROW_FONT, tokens.accent, x);
    }
    (canvas.into_image(), panel)
}

/// The window the `--show-window-hint` scene draws the glow around: a
/// plain panel of WinCraft's own with a title, so no real window is used.
pub fn test_window(size: (i32, i32), tokens: &Tokens, s: f32) -> Image {
    let mut canvas = Canvas::new(size.0, size.1);
    let area = Area::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    let radius = CORNER * s;
    canvas.fill(area, radius, tokens.window_bg);
    canvas.outline(area, radius, look::border(tokens, s), tokens.rim);
    let fonts = Fonts::new(&[(Weight::Semibold, (14.0 * s).round() as i32)]);
    let run = fonts.run(
        "WinCraft test window",
        0,
        tokens.text_secondary,
        (16.0 * s).round() as i32,
        (12.0 * s).round() as i32,
    );
    canvas.text(&[run], &fonts.list, 1.0);
    canvas.fill(
        Area::new(0.0, 40.0 * s, size.0 as f32, 1.0_f32.max(s.round())),
        0.0,
        theme::with_alpha(tokens.text_primary, 0.08),
    );
    canvas.into_image()
}
