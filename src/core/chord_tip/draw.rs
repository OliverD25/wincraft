//! Paints the chord tip. The design's colours are the dark Classical
//! theme's; here every colour comes from the theme tokens, so the tip
//! follows the theme and light or dark.

use egui::Color32;

use crate::core::canvas::{Area, Canvas, Fonts, Weight};
use crate::core::layered::Image;
use crate::core::panel_look as look;
use crate::core::theme::{self, Tokens};

use super::{dot, panel_size, Content, End, Tone, HEADER, LIST_TOP, ROW, ROW_GAP};

const PAD_X: f32 = 18.0;
const CAP: f32 = 30.0;
const CAP_PAD: f32 = 9.0;
const SMALL_CAP: f32 = 26.0;
const SMALL_CAP_PAD: f32 = 7.0;
const ESC_CAP: f32 = 24.0;
const SLOT_W: f32 = 46.0;
const GAP: f32 = 5.0;
const COLUMN_GAP: f32 = 24.0;
const DOT: f32 = 5.0;
const DOT_GAP: f32 = 4.0;
const LIFT: f32 = 3.0;

// The fonts' places in `Fonts::list`.
const CAP_FONT: usize = 0;
const PLUS_FONT: usize = 1;
const SMALL_FONT: usize = 2;
const TEXT_FONT: usize = 3;

/// The tip without what moves: the dots in the empty keycap and the
/// countdown bar, which `frame` adds.
#[derive(Clone)]
pub struct Picture {
    pub base: Canvas,
    /// The empty keycap while it waits.
    slot: Option<Area>,
    /// Where the countdown bar runs.
    bar: Area,
    scale: f32,
}

fn fonts(scale: f32) -> Fonts {
    let px = |size: f32| (size * scale).round() as i32;
    Fonts::new(&[
        (Weight::Semibold, px(15.0)),
        (Weight::Regular, px(14.0)),
        (Weight::Semibold, px(13.0)),
        (Weight::Regular, px(15.0)),
    ])
}

/// How a keycap looks.
struct Cap {
    font: usize,
    height: f32,
    pad: f32,
    /// The bottom edge is thicker, like a key seen from the front.
    bottom: f32,
    edge: Color32,
    text: Color32,
    face: Color32,
}

/// A keycap with its top left at `at`; returns its width.
fn keycap(canvas: &mut Canvas, fonts: &Fonts, key: &str, at: (f32, f32), cap: &Cap, s: f32) -> f32 {
    let (text_w, text_h) = fonts.measure(key, cap.font);
    let height = (cap.height * s).round();
    let width = (text_w as f32 + 2.0 * cap.pad * s).max(height).round();
    let radius = 6.0 * s;
    let line = s.round().max(1.0);
    let bottom = (cap.bottom * s).round().max(line);
    canvas.fill(Area::new(at.0, at.1, width, height), radius, cap.edge);
    canvas.fill(
        Area::new(
            at.0 + line,
            at.1 + line,
            width - 2.0 * line,
            height - line - bottom,
        ),
        (radius - line).max(0.0),
        cap.face,
    );
    let run = fonts.run(
        key,
        cap.font,
        cap.text,
        (at.0 + (width - text_w as f32) / 2.0).round() as i32,
        (at.1 + (height - bottom + line - text_h as f32) / 2.0).round() as i32,
    );
    canvas.text(&[run], &fonts.list, 1.0);
    width
}

/// Text in `font`, its middle at `middle`, left edge at `x`; returns its
/// width.
fn text(
    canvas: &mut Canvas,
    fonts: &Fonts,
    words: &str,
    font: usize,
    colour: Color32,
    at: (f32, f32),
) -> f32 {
    let (width, height) = fonts.measure(words, font);
    let run = fonts.run(
        words,
        font,
        colour,
        at.0.round() as i32,
        (at.1 - height as f32 / 2.0).round() as i32,
    );
    canvas.text(&[run], &fonts.list, 1.0);
    width as f32
}

fn tone_colours(tone: Tone, tokens: &Tokens) -> (Color32, Color32) {
    match tone {
        Tone::Accent => (tokens.accent, tokens.text_primary),
        Tone::Danger => (tokens.danger, tokens.danger),
        Tone::Quiet => (tokens.text_disabled, tokens.text_secondary),
    }
}

/// The tip while it waits (`ending` None) or as it ended.
pub fn render(content: &Content, ending: Option<&End>, tokens: &Tokens, s: f32) -> Picture {
    let px = |logical: f32| logical * s;
    let margin = (look::SHADOW * s).round();
    let list = ending.is_none();
    let (width, height) = panel_size(content.rows.len(), list);
    let mut canvas = Canvas::new(
        (px(width) + 2.0 * margin).round() as i32,
        (px(height) + 2.0 * margin).round() as i32,
    );
    let panel = look::panel(&mut canvas, tokens, s);
    let border = look::border(tokens, s);
    let radius = f32::from(tokens.radius_window) * s;
    match ending.map(End::tone) {
        Some(Tone::Accent) => canvas.outline(panel, radius, border, tokens.accent),
        Some(Tone::Danger) => {
            canvas.outline(panel, radius, border, theme::with_alpha(tokens.danger, 0.5))
        }
        _ => {}
    }
    let fonts = fonts(s);
    let tone = ending.map_or(Tone::Accent, End::tone);
    let (cap_colour, text_colour) = tone_colours(tone, tokens);
    let big = Cap {
        font: CAP_FONT,
        height: CAP,
        pad: CAP_PAD,
        bottom: 3.0,
        edge: cap_colour,
        text: cap_colour,
        face: tokens.elevated_bg,
    };
    let middle = panel.y + px(HEADER) / 2.0;
    let top = middle - px(CAP) / 2.0;
    let mut x = panel.x + px(PAD_X);
    let right = panel.right() - px(PAD_X);
    let mut keys: Vec<&str> = content.keys.iter().map(String::as_str).collect();
    if let Some(key) = ending.and_then(End::key) {
        keys.push(key);
    }
    let plus = |canvas: &mut Canvas, x: f32| {
        let w = text(
            canvas,
            &fonts,
            "+",
            PLUS_FONT,
            tokens.text_disabled,
            (x + px(GAP), middle),
        );
        x + w + 2.0 * px(GAP)
    };
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            x = plus(&mut canvas, x);
        }
        x += keycap(&mut canvas, &fonts, key, (x, top), &big, s);
    }
    let mut slot = None;
    match ending {
        None => {
            if !keys.is_empty() {
                x = plus(&mut canvas, x);
            }
            let area = Area::new(x, top, px(SLOT_W).round(), px(CAP).round());
            canvas.dashed(
                area,
                6.0 * s,
                s.round().max(1.0),
                theme::with_alpha(tokens.accent, 0.6),
                Some(3.0 * s),
            );
            slot = Some(area);
            esc_cancel(&mut canvas, &fonts, tokens, right, middle, s);
        }
        Some(end) => {
            let room = (right - x - px(12.0)).max(0.0);
            let words = fonts.fit(&end.text(), TEXT_FONT, room.round() as i32);
            text(
                &mut canvas,
                &fonts,
                &words,
                TEXT_FONT,
                text_colour,
                (x + px(12.0), middle),
            );
        }
    }
    if list && !content.rows.is_empty() {
        rows(&mut canvas, &fonts, content, panel, tokens, s);
    }
    let bar_h = px(super::BAR).round().max(1.0);
    let bar = Area::new(
        panel.x + radius,
        panel.bottom() - border - bar_h,
        panel.w - 2.0 * radius,
        bar_h,
    );
    canvas.fill(bar, 0.0, theme::with_alpha(tokens.text_primary, 0.06));
    if matches!(ending, Some(End::Matched { .. })) {
        canvas.fill(bar, 0.0, tokens.accent);
    }
    Picture {
        base: canvas,
        slot,
        bar,
        scale: s,
    }
}

/// "WinCraft" (Principle 1: the user knows who holds the keys) and "Esc
/// Cancel" at the right end of the first line.
fn esc_cancel(
    canvas: &mut Canvas,
    fonts: &Fonts,
    tokens: &Tokens,
    right: f32,
    middle: f32,
    s: f32,
) {
    let (word_w, _) = fonts.measure("Cancel", PLUS_FONT);
    let (esc_w, _) = fonts.measure("Esc", SMALL_FONT);
    let (name_w, _) = fonts.measure("WinCraft", PLUS_FONT);
    let cap_w = (esc_w as f32 + 2.0 * SMALL_CAP_PAD * s)
        .max(ESC_CAP * s)
        .round();
    let start = right - word_w as f32 - 7.0 * s - cap_w;
    text(
        canvas,
        fonts,
        "WinCraft",
        PLUS_FONT,
        tokens.text_disabled,
        (start - 16.0 * s - name_w as f32, middle),
    );
    let cap = Cap {
        font: SMALL_FONT,
        height: ESC_CAP,
        pad: SMALL_CAP_PAD,
        bottom: 1.0,
        edge: theme::with_alpha(tokens.text_primary, 0.14),
        text: tokens.text_secondary,
        face: tokens.elevated_bg,
    };
    let width = keycap(
        canvas,
        fonts,
        "Esc",
        (start, middle - ESC_CAP * s / 2.0),
        &cap,
        s,
    );
    text(
        canvas,
        fonts,
        "Cancel",
        PLUS_FONT,
        tokens.text_secondary,
        (start + width + 7.0 * s, middle),
    );
}

/// The keys that can follow, in two columns, filled row by row.
fn rows(
    canvas: &mut Canvas,
    fonts: &Fonts,
    content: &Content,
    panel: Area,
    tokens: &Tokens,
    s: f32,
) {
    let px = |logical: f32| logical * s;
    let divider = panel.y + px(HEADER);
    canvas.fill(
        Area::new(panel.x, divider, panel.w, s.round().max(1.0)),
        0.0,
        theme::with_alpha(tokens.text_primary, 0.08),
    );
    let left = panel.x + px(PAD_X);
    let column = (panel.w - 2.0 * px(PAD_X) - px(COLUMN_GAP)) / 2.0;
    let first = divider + s.round().max(1.0) + px(LIST_TOP);
    let cap = Cap {
        font: SMALL_FONT,
        height: SMALL_CAP,
        pad: SMALL_CAP_PAD,
        bottom: 2.0,
        edge: theme::with_alpha(tokens.text_primary, 0.2),
        text: tokens.text_primary,
        face: tokens.elevated_bg,
    };
    for (index, row) in content.rows.iter().enumerate() {
        let x = left + (index % 2) as f32 * (column + px(COLUMN_GAP));
        let middle = first + (index / 2) as f32 * px(ROW + ROW_GAP) + px(ROW) / 2.0;
        let width = keycap(
            canvas,
            fonts,
            &row.key,
            (x, middle - px(SMALL_CAP) / 2.0),
            &cap,
            s,
        );
        let room = (column - width - px(12.0)).max(0.0);
        let words = fonts.fit(&row.text, TEXT_FONT, room.round() as i32);
        text(
            canvas,
            fonts,
            &words,
            TEXT_FONT,
            tokens.text_primary,
            (x + width + px(12.0), middle),
        );
    }
}

/// The picture at `ms` into the wait with `left` of the time to go: the
/// dots in the empty keycap and the bar.
pub fn frame(picture: &Picture, ms: u64, left: f32, tokens: &Tokens) -> Image {
    let mut canvas = picture.base.clone();
    let s = picture.scale;
    if let Some(slot) = picture.slot {
        let side = (DOT * s).round().max(2.0);
        let gap = DOT_GAP * s;
        let all = 3.0 * side + 2.0 * gap;
        let x0 = slot.x + (slot.w - all) / 2.0;
        let y0 = slot.y + (slot.h - side) / 2.0 - s;
        for index in 0..3 {
            let (alpha, lift) = dot(ms, index);
            canvas.fill(
                Area::new(
                    x0 + index as f32 * (side + gap),
                    y0 - LIFT * s * lift,
                    side,
                    side,
                ),
                side / 2.0,
                theme::with_alpha(tokens.accent, alpha),
            );
        }
        let length = (picture.bar.w * left.clamp(0.0, 1.0)).round();
        if length > 0.0 {
            canvas.fill(
                Area::new(picture.bar.x, picture.bar.y, length, picture.bar.h),
                0.0,
                tokens.accent,
            );
        }
    }
    canvas.into_image()
}
