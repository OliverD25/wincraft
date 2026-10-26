//! The slot strip that shows while QuickJump waits for a digit: a tile per
//! slot, and in pin mode the window that is about to be pinned. It is a
//! click-through panel at the bottom of the screen, like the taskbar
//! number hint, and never takes the keyboard from the app in front.

use egui::Color32;
use windows_sys::Win32::Foundation::POINT;

use crate::core::canvas::{Area, Canvas, Fonts, Weight};
use crate::core::layered::{Image, Layered};
use crate::core::theme::{self, Tokens};
use crate::search::icons::Pixels;
use crate::ui::taskbar_badges;
use crate::ui::widgets::icons::Icon;

use super::look::{self, Cap};
use super::matching::{SlotView, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Jump,
    Pin,
}

/// The colour of a line of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Secondary,
    Disabled,
}

impl Tone {
    pub fn colour(self, tokens: &Tokens) -> Color32 {
        match self {
            Tone::Accent => tokens.accent,
            Tone::Secondary => tokens.text_secondary,
            Tone::Disabled => tokens.text_disabled,
        }
    }
}

/// The window the pin strip is about to pin.
pub struct Target<'a> {
    pub name: String,
    pub place: String,
    pub icon: Option<&'a Pixels>,
    /// The slot that holds it already.
    pub slot: Option<usize>,
}

pub struct Content<'a> {
    pub mode: Mode,
    pub slots: &'a [SlotView],
    /// One per slot.
    pub icons: Vec<Option<&'a Pixels>>,
    pub target: Option<Target<'a>>,
    pub hint: String,
}

/// The line under a tile's name.
pub fn status(mode: Mode, view: &SlotView, target_slot: Option<usize>) -> (String, Tone) {
    let text = |words: &str, tone| (words.to_string(), tone);
    match (mode, view.state) {
        (Mode::Jump, State::InFront) => text("in front", Tone::Accent),
        (Mode::Jump, State::Open) => (view.place.clone(), Tone::Secondary),
        (Mode::Jump, State::NotOpen) => text("not open", Tone::Disabled),
        (Mode::Jump, State::Empty) => text("empty", Tone::Disabled),
        (Mode::Pin, State::Empty) => text("Pin here", Tone::Accent),
        (Mode::Pin, _) if target_slot == Some(view.slot) => text("Pinned here", Tone::Accent),
        (Mode::Pin, _) => text("Replace", Tone::Secondary),
    }
}

/// The line under the tiles. `jump_keys` is the jump shortcut as the user
/// set it; without one the second step is left out.
pub fn hint(mode: Mode, jump_keys: &str, target: &str) -> String {
    match mode {
        Mode::Jump if jump_keys.is_empty() => "Press a number to jump \u{b7} Esc".to_string(),
        Mode::Jump => {
            format!("Press a number to jump \u{b7} {jump_keys} again for the list \u{b7} Esc")
        }
        Mode::Pin => format!("Press a number to pin {target} \u{b7} Esc"),
    }
}

// Sizes in logical pixels, from the design.
const PAD: f32 = 12.0;
const GAP: f32 = 10.0;
const TILE_W: f32 = 84.0;
const TILE_H: f32 = 116.0;
const TILE_GAP: f32 = 6.0;
const TILE_PAD: f32 = 8.0;
const TILE_RADIUS: f32 = 6.0;
const KEYCAP: f32 = 22.0;
const BADGE_H: f32 = 18.0;
const ICON: f32 = 28.0;
const NAME_MAX: f32 = 68.0;
const COLUMN: f32 = 184.0;
const TARGET_ICON: f32 = 32.0;
const LINE: f32 = 16.0;
const COUNTDOWN: f32 = 2.0;
const NOT_OPEN: f32 = 0.45;

// The fonts' places in `Fonts::list`.
const SMALL: usize = 0;
const SMALL_SEMI: usize = 1;
const NAME: usize = 2;

fn size(mode: Mode, scale: f32) -> (i32, i32) {
    let tiles = 10.0 * TILE_W + 9.0 * TILE_GAP;
    let column = match mode {
        Mode::Jump => 0.0,
        Mode::Pin => COLUMN + 2.0 * PAD + 1.0,
    };
    let width = 2.0 * look::SHADOW + 2.0 * PAD + column + tiles;
    let height = 2.0 * look::SHADOW + 2.0 * PAD + LINE + GAP + TILE_H + GAP + LINE;
    (
        (width * scale).round() as i32,
        (height * scale).round() as i32,
    )
}

/// The strip without its countdown line, which `with_countdown` adds.
pub fn render(content: &Content, scale: f32, tokens: &Tokens) -> Canvas {
    let px = |logical: f32| logical * scale;
    let (width, height) = size(content.mode, scale);
    let mut canvas = Canvas::new(width, height);
    let panel = look::panel(&mut canvas, tokens, scale);
    let fonts = Fonts::new(&[
        (Weight::Regular, px(12.0).round() as i32),
        (Weight::Semibold, px(12.0).round() as i32),
        (Weight::Semibold, px(14.0).round() as i32),
    ]);
    let left = panel.x + px(PAD);
    let mut y = panel.y + px(PAD);

    let header = fonts.run(
        "QuickJump",
        SMALL_SEMI,
        tokens.text_secondary,
        left as i32,
        y as i32,
    );
    let word_x = left as i32 + header.width + px(6.0).round() as i32;
    let (word, colour) = match content.mode {
        Mode::Jump => ("Jump", tokens.text_disabled),
        Mode::Pin => ("Pin", tokens.accent),
    };
    let word = fonts.run(word, SMALL, colour, word_x, y as i32);
    canvas.text(&[header, word], &fonts.list, 1.0);
    y += px(LINE + GAP);

    let mut x = left;
    if let (Mode::Pin, Some(target)) = (content.mode, &content.target) {
        draw_target(&mut canvas, &fonts, target, (x, y), tokens, scale);
        x += px(COLUMN + PAD);
        canvas.fill(
            Area::new(x, y, look::border(tokens, scale), px(TILE_H)),
            0.0,
            tokens.border,
        );
        x += px(PAD) + 1.0;
    }
    let target_slot = content.target.as_ref().and_then(|target| target.slot);
    for (view, icon) in content.slots.iter().zip(&content.icons) {
        let tile = Area::new(x, y, px(TILE_W), px(TILE_H));
        let (status, tone) = status(content.mode, view, target_slot);
        draw_tile(
            &mut canvas,
            &fonts,
            Tile {
                area: tile,
                view,
                icon: *icon,
                status: &status,
                tone,
                mode: content.mode,
            },
            tokens,
            scale,
        );
        x += px(TILE_W + TILE_GAP);
    }
    y += px(TILE_H + GAP);
    let hint = fonts.fit(
        &content.hint,
        SMALL,
        (panel.w - 2.0 * px(PAD)).round() as i32,
    );
    let hint = fonts.run(&hint, SMALL, tokens.text_secondary, left as i32, y as i32);
    canvas.text(&[hint], &fonts.list, 1.0);
    canvas
}

fn draw_target(
    canvas: &mut Canvas,
    fonts: &Fonts,
    target: &Target,
    at: (f32, f32),
    tokens: &Tokens,
    scale: f32,
) {
    let px = |logical: f32| logical * scale;
    let (x, mut y) = at;
    let label = fonts.run(
        "Window to pin",
        SMALL,
        tokens.text_disabled,
        x as i32,
        y as i32,
    );
    canvas.text(&[label], &fonts.list, 1.0);
    y += px(LINE + 8.0);
    let frame = Area::new(x, y, px(TARGET_ICON + 8.0), px(TARGET_ICON + 8.0));
    canvas.outline(
        frame,
        px(TILE_RADIUS),
        look::border(tokens, scale),
        tokens.accent,
    );
    if let Some(icon) = target.icon {
        canvas.icon(icon, x + px(4.0), y + px(4.0), px(TARGET_ICON), 1.0);
    }
    y += px(TARGET_ICON + 8.0 + 8.0);
    let name = fonts.fit(&target.name, NAME, px(COLUMN).round() as i32);
    let name = fonts.run(&name, NAME, tokens.text_primary, x as i32, y as i32);
    y += name.height as f32 + px(2.0);
    let place = fonts.fit(&target.place, SMALL, px(COLUMN).round() as i32);
    let place = fonts.run(&place, SMALL, tokens.text_secondary, x as i32, y as i32);
    canvas.text(&[name, place], &fonts.list, 1.0);
}

struct Tile<'a> {
    area: Area,
    view: &'a SlotView,
    icon: Option<&'a Pixels>,
    status: &'a str,
    tone: Tone,
    mode: Mode,
}

fn draw_tile(canvas: &mut Canvas, fonts: &Fonts, tile: Tile, tokens: &Tokens, scale: f32) {
    let px = |logical: f32| logical * scale;
    let Tile {
        area,
        view,
        icon,
        status,
        tone,
        mode,
    } = tile;
    let radius = px(TILE_RADIUS);
    let line = look::border(tokens, scale);
    match (mode, view.state) {
        (Mode::Jump, State::Empty) => {
            canvas.dashed(area, radius, line, tokens.text_disabled, Some(px(4.0)))
        }
        (Mode::Pin, State::Empty) => {
            canvas.dashed(area, radius, line, tokens.accent, Some(px(4.0)))
        }
        (Mode::Jump, State::InFront) => {
            canvas.fill(area, radius, tokens.selection_bg);
            canvas.outline(area, radius, line, tokens.accent);
        }
        _ => {
            canvas.fill(area, radius, tokens.panel_bg);
            canvas.outline(area, radius, line, tokens.border);
        }
    }
    let inner = area.inset(px(TILE_PAD));
    let digit = view.digit.to_string();
    look::keycap(
        canvas,
        fonts,
        &digit,
        (inner.x, inner.y),
        &Cap::new(tokens, SMALL_SEMI, px(KEYCAP), scale),
    );
    if let Some(badge) = &view.badge {
        let (text_w, text_h) = fonts.measure(badge, SMALL);
        let badge_w = (text_w as f32 + px(12.0)).max(px(BADGE_H));
        let badge_area = Area::new(inner.right() - badge_w, inner.y, badge_w, px(BADGE_H));
        canvas.fill(badge_area, px(BADGE_H / 2.0), tokens.accent_tint);
        let run = fonts.run(
            badge,
            SMALL,
            tokens.accent,
            (badge_area.x + (badge_w - text_w as f32) / 2.0).round() as i32,
            (badge_area.y + (px(BADGE_H) - text_h as f32) / 2.0).round() as i32,
        );
        canvas.text(&[run], &fonts.list, 1.0);
    }
    let dim = if view.state == State::NotOpen {
        NOT_OPEN
    } else {
        1.0
    };
    let icon_y = inner.y + px(KEYCAP + 6.0);
    let icon_x = area.x + (area.w - px(ICON)) / 2.0;
    match (mode, view.state, icon) {
        (Mode::Pin, State::Empty, _) => {
            let side = px(20.0);
            canvas.glyph(
                Icon::Pin,
                area.x + (area.w - side) / 2.0,
                icon_y + (px(ICON) - side) / 2.0,
                side,
                tokens.accent,
            );
        }
        (_, State::Empty, _) => {}
        (_, _, Some(icon)) => canvas.icon(icon, icon_x, icon_y, px(ICON), dim),
        (_, _, None) => canvas.fill(
            Area::new(icon_x, icon_y, px(ICON), px(ICON)),
            px(6.0),
            theme::with_alpha(tokens.text_disabled, 0.35 * dim),
        ),
    }
    let mut runs = Vec::new();
    let text_y = icon_y + px(ICON + 6.0);
    if view.is_pinned() {
        let name = fonts.fit(&view.name, SMALL_SEMI, px(NAME_MAX).round() as i32);
        let (name_w, _) = fonts.measure(&name, SMALL_SEMI);
        runs.push(fonts.run(
            &name,
            SMALL_SEMI,
            theme::with_alpha(tokens.text_primary, dim),
            (area.x + (area.w - name_w as f32) / 2.0).round() as i32,
            text_y.round() as i32,
        ));
    }
    let status = fonts.fit(status, SMALL, px(NAME_MAX).round() as i32);
    let (status_w, _) = fonts.measure(&status, SMALL);
    runs.push(fonts.run(
        &status,
        SMALL,
        tone.colour(tokens),
        (area.x + (area.w - status_w as f32) / 2.0).round() as i32,
        (text_y + px(LINE + 2.0)).round() as i32,
    ));
    canvas.text(&runs, &fonts.list, 1.0);
}

/// The base picture with the countdown line along the panel's top edge,
/// `left` of it still to go (1 at the start, 0 at the end).
pub fn with_countdown(base: &Canvas, left: f32, tokens: &Tokens, scale: f32) -> Image {
    let mut canvas = base.clone();
    let margin = (look::SHADOW * scale).round();
    let radius = f32::from(tokens.radius_window) * scale;
    let full = canvas.width as f32 - 2.0 * margin - 2.0 * radius;
    let length = (full * left.clamp(0.0, 1.0)).round();
    if length > 0.0 {
        canvas.fill(
            Area::new(
                margin + radius,
                margin + look::border(tokens, scale),
                length,
                (COUNTDOWN * scale).round(),
            ),
            0.0,
            tokens.accent,
        );
    }
    canvas.into_image()
}

/// The strip on screen; dropping it takes it away.
pub struct Strip {
    window: Option<Layered>,
    base: Canvas,
    at: POINT,
    scale: f32,
}

impl Strip {
    /// Shows the strip at the bottom of the monitor with the window in
    /// front, 24 px above the work area's edge.
    pub fn show(content: &Content) -> Option<Self> {
        let (work, scale) = taskbar_badges::work_area(taskbar_badges::front_monitor())?;
        let tokens = theme::current_tokens();
        let base = render(content, scale, &tokens);
        let lift = ((24.0 - look::SHADOW) * scale).round() as i32;
        let at = taskbar_badges::bottom_centre(work, (base.width, base.height), lift);
        let image = with_countdown(&base, 1.0, &tokens, scale);
        let window = taskbar_badges::present(&image, at);
        window.as_ref()?;
        Some(Self {
            window,
            base,
            at,
            scale,
        })
    }

    /// Draws the countdown line shorter.
    pub fn tick(&mut self, left: f32) {
        let tokens = theme::current_tokens();
        let image = with_countdown(&self.base, left, &tokens, self.scale);
        if let Some(window) = &self.window {
            window.present(&image, self.at);
        }
    }
}

impl Drop for Strip {
    fn drop(&mut self) {
        if let Some(window) = &mut self.window {
            window.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::matching::slot_views;
    use super::super::matching::tests::window;
    use super::super::pins::tests::pin;
    use super::super::pins::Slots;
    use super::*;
    use crate::core::config::ThemeName;

    fn views() -> Vec<SlotView> {
        let mut slots = Slots::default();
        slots.pin(0, pin("Claude", None));
        slots.pin(1, pin("Chrome", None));
        slots.pin(2, pin("Code", None));
        let mut far = window(2, "Chrome", "Docs");
        far.desktop = Some(2);
        far.other_desktop = true;
        slot_views(&slots, &[window(1, "Claude", "Claude"), far], 1)
    }

    #[test]
    fn each_tile_says_what_a_digit_would_do() {
        let views = views();
        let jump = |slot: usize| status(Mode::Jump, &views[slot], None);
        assert_eq!(jump(0), ("in front".to_string(), Tone::Accent));
        assert_eq!(jump(2), ("not open".to_string(), Tone::Disabled));
        assert_eq!(jump(3), ("empty".to_string(), Tone::Disabled));
        let pin_to = |slot: usize| status(Mode::Pin, &views[slot], Some(0));
        assert_eq!(pin_to(3), ("Pin here".to_string(), Tone::Accent));
        assert_eq!(pin_to(1), ("Replace".to_string(), Tone::Secondary));
        assert_eq!(pin_to(0), ("Pinned here".to_string(), Tone::Accent));
    }

    #[test]
    fn the_hint_names_the_users_keys_and_the_window() {
        assert_eq!(
            hint(Mode::Jump, "Win+Alt+Q", ""),
            "Press a number to jump \u{b7} Win+Alt+Q again for the list \u{b7} Esc"
        );
        assert_eq!(
            hint(Mode::Jump, "", ""),
            "Press a number to jump \u{b7} Esc"
        );
        assert_eq!(
            hint(Mode::Pin, "Win+Alt+Q", "Claude"),
            "Press a number to pin Claude \u{b7} Esc"
        );
    }

    #[test]
    fn both_strips_draw_in_every_theme_and_the_countdown_shrinks() {
        let views = views();
        let icon = Pixels {
            side: 1,
            rgba: vec![10, 20, 30, 255],
        };
        for (name, dark) in ThemeName::ALL
            .into_iter()
            .zip([true, false, true, false, true])
        {
            let tokens = Tokens::of(name, dark);
            for mode in [Mode::Jump, Mode::Pin] {
                let content = Content {
                    mode,
                    slots: &views,
                    icons: (0..10).map(|slot| (slot < 2).then_some(&icon)).collect(),
                    target: Some(Target {
                        name: "Claude".to_string(),
                        place: "Desktop 2 \u{b7} Centre".to_string(),
                        icon: Some(&icon),
                        slot: Some(0),
                    }),
                    hint: hint(mode, "Win+Alt+Q", "Claude"),
                };
                let base = render(&content, 1.0, &tokens);
                assert_eq!((base.width, base.height), size(mode, 1.0));
                let full = with_countdown(&base, 1.0, &tokens, 1.0);
                let none = with_countdown(&base, 0.0, &tokens, 1.0);
                let top = (look::SHADOW + look::border(&tokens, 1.0)) as i32;
                let lit = |image: &Image| {
                    (0..image.width)
                        .filter(|x| image.pixels[(top * image.width + x) as usize] >> 24 == 255)
                        .count()
                };
                assert!(lit(&full) > lit(&none), "{name:?} {mode:?}");
            }
        }
    }
}
