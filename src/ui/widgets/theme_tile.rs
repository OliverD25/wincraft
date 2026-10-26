use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Color32, CursorIcon, EventFilter, Id, Key, Modifiers, Painter, Rect, Response,
    Sense, StrokeKind, Ui,
};

use crate::core::config::ThemeName;
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::focus;

pub const MIN_WIDTH: f32 = 168.0;
pub const GAP: f32 = 12.0;
pub const MAX_COLUMNS: usize = 4;
/// 8 padding, the 96 high picture, 10, name 20, blurb 16 and 10 padding.
pub const HEIGHT: f32 = 160.0;
const PAD: f32 = 8.0;
const PICTURE_HEIGHT: f32 = 96.0;
const NAME_ROW: f32 = 20.0;
const BLURB_ROW: f32 = 16.0;

/// How long the pointer or the keyboard focus rests on a tile before the
/// whole app shows that theme.
pub const DELAY: Duration = Duration::from_millis(120);

/// What the theme picker previews. Moving from one tile to the next keeps
/// the preview that is showing until the next one takes over, so the app
/// does not flash back to the chosen theme in between.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HoverPreview {
    pending: Option<(ThemeName, Instant)>,
    shown: Option<ThemeName>,
    /// The tile Esc was pressed on: it shows no preview until the pointer
    /// or the focus moves to another tile.
    held: Option<ThemeName>,
}

impl HoverPreview {
    /// One frame: `target` is the tile under the pointer or with keyboard
    /// focus, `inside` whether the pointer or the focus is in the picker at
    /// all, gaps between tiles included. Returns the theme to preview.
    pub fn update(
        &mut self,
        target: Option<ThemeName>,
        inside: bool,
        now: Instant,
    ) -> Option<ThemeName> {
        if !inside {
            *self = Self::default();
            return None;
        }
        if self.held.is_some() && target != self.held {
            self.held = None;
        }
        match target {
            Some(name) if self.held == Some(name) => {}
            Some(name) if self.shown == Some(name) => self.pending = None,
            Some(name) => match self.pending {
                Some((waiting, since)) if waiting == name => {
                    if now.duration_since(since) >= DELAY {
                        self.shown = Some(name);
                        self.pending = None;
                    }
                }
                _ => self.pending = Some((name, now)),
            },
            None => self.pending = None,
        }
        self.shown
    }

    /// Esc: back to the chosen theme while the pointer stays where it is.
    pub fn escape(&mut self, target: Option<ThemeName>) {
        self.pending = None;
        self.shown = None;
        self.held = target;
    }

    /// A click or Enter: the chosen theme is this one now, so the preview
    /// simply stays on it until the choice comes back from the host.
    pub fn choose(&mut self, name: ThemeName) {
        self.pending = None;
        self.shown = Some(name);
        self.held = None;
    }

    pub fn shown(&self) -> Option<ThemeName> {
        self.shown
    }

    /// How long until the waiting tile's preview is due, to ask for a repaint.
    pub fn due_in(&self, now: Instant) -> Option<Duration> {
        self.pending
            .map(|(_, since)| DELAY.saturating_sub(now.duration_since(since)))
    }
}

/// Up to four tiles in a row, each at least 168 wide.
pub fn columns(width: f32) -> usize {
    (((width + GAP) / (MIN_WIDTH + GAP)).floor() as usize).clamp(1, MAX_COLUMNS)
}

pub fn tile_width(width: f32, columns: usize) -> f32 {
    (width - GAP * columns.saturating_sub(1) as f32) / columns.max(1) as f32
}

/// The theme tiles for Settings → General. `preview` lives in the settings
/// state across frames. Returns the theme the user chose this frame.
pub fn picker(ui: &mut Ui, chosen: ThemeName, preview: &mut HoverPreview) -> Option<ThemeName> {
    let ctx = ui.ctx().clone();
    let width = ui.available_width();
    let columns = columns(width);
    let each = tile_width(width, columns);
    let names = ThemeName::ALL;
    let rows = names.len().div_ceil(columns);
    let height = rows as f32 * HEIGHT + rows.saturating_sub(1) as f32 * GAP;
    let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

    let grid = ui.id().with("theme-tiles");
    let ids: Vec<Id> = names.iter().map(|name| grid.with(name.key())).collect();
    let focused = ids
        .iter()
        .position(|id| ui.memory(|memory| memory.has_focus(*id)));
    if let Some(at) = focused {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                ids[at],
                EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..EventFilter::default()
                },
            )
        });
        let step = ui.input_mut(|input| {
            if input.consume_key(Modifiers::NONE, Key::ArrowLeft)
                || input.consume_key(Modifiers::NONE, Key::ArrowUp)
            {
                -1
            } else if input.consume_key(Modifiers::NONE, Key::ArrowRight)
                || input.consume_key(Modifiers::NONE, Key::ArrowDown)
            {
                1
            } else {
                0
            }
        });
        if step != 0 {
            let to = (at as i32 + step).clamp(0, names.len() as i32 - 1) as usize;
            ui.memory_mut(|memory| memory.request_focus(ids[to]));
        }
    }

    let keyboard = focus::keyboard_active(&ctx);
    let (enter, escape) = ui.input(|input| {
        (
            input.key_pressed(Key::Enter) || input.key_pressed(Key::Space),
            input.key_pressed(Key::Escape),
        )
    });
    let mut hovered = None;
    let mut focused_now = None;
    let mut picked = None;
    for (slot, name) in names.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(
                area.left() + (slot % columns) as f32 * (each + GAP),
                area.top() + (slot / columns) as f32 * (HEIGHT + GAP),
            ),
            vec2(each, HEIGHT),
        );
        let response = tile(ui, ids[slot], rect, *name, *name == chosen, keyboard);
        if response.hovered() {
            hovered = Some(*name);
        }
        if response.has_focus() {
            focused_now = Some(*name);
        }
        if response.clicked() || (enter && response.has_focus()) {
            picked = Some(*name);
        }
    }

    let pointer_inside = ctx
        .pointer_hover_pos()
        .is_some_and(|pointer| area.contains(pointer));
    let keyboard_focus = focused_now.filter(|_| keyboard);
    let target = hovered.or(keyboard_focus);
    let inside = pointer_inside || keyboard_focus.is_some();
    let now = Instant::now();
    if escape && inside {
        preview.escape(target);
    }
    if let Some(name) = picked {
        preview.choose(name);
    }
    let showing = preview.update(target, inside, now);
    if theme::previewing(&ctx) != showing {
        theme::preview(&ctx, showing);
        ctx.request_repaint();
    }
    if let Some(wait) = preview.due_in(now) {
        ctx.request_repaint_after(wait);
    }
    picked
}

/// One tile: the mini Settings window in that theme and the current mode,
/// its name, one line about it, and "Current" on the chosen one.
fn tile(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    name: ThemeName,
    current: bool,
    keyboard: bool,
) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover =
        ui.ctx()
            .animate_bool_with_time(id.with("hover"), response.hovered() && !current, 0.08);
    let (fill, border) = if current {
        (tokens.selection_bg, tokens.accent)
    } else {
        (tokens.hover_bg.gamma_multiply(hover), tokens.border)
    };
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        tokens.radius_tile,
        fill,
        theme::line(ui.ctx(), border),
        StrokeKind::Inside,
    );
    if current && tokens.selection_ring {
        painter.rect_stroke(
            rect,
            tokens.radius_tile,
            theme::stroke(ui.ctx(), 2.0, tokens.accent),
            StrokeKind::Inside,
        );
    }

    let picture = Rect::from_min_size(
        rect.min + vec2(PAD, PAD),
        vec2(rect.width() - 2.0 * PAD, PICTURE_HEIGHT),
    );
    let look = Tokens::of(name, tokens.dark);
    if look.glass_alpha < 1.0 {
        // A glass theme shows a colourful desktop through its window, so the
        // tile reads as glass at a glance.
        glass_desktop(&painter, picture, &look);
        let mut glass = look;
        glass.window_bg = look.window_bg.gamma_multiply(look.glass_alpha);
        mini_settings(
            &painter,
            picture.shrink2(vec2(10.0, 8.0)),
            &glass,
            ui.ctx().pixels_per_point(),
        );
    } else {
        mini_settings(&painter, picture, &look, ui.ctx().pixels_per_point());
    }

    let name_top = picture.bottom() + 10.0;
    let label = painter.layout_no_wrap(
        name.label().to_string(),
        theme::semibold(14.0),
        tokens.text_primary,
    );
    painter.galley(
        pos2(
            rect.left() + PAD,
            name_top + (NAME_ROW - label.size().y) / 2.0,
        ),
        label,
        tokens.text_primary,
    );
    if current {
        current_badge(ui, &painter, rect.right() - PAD, name_top + NAME_ROW / 2.0);
    }
    let mut blurb = egui::text::LayoutJob::simple_singleline(
        name.blurb().to_string(),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    blurb.wrap = egui::text::TextWrapping {
        max_width: (rect.width() - 2.0 * PAD).max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let blurb = painter.layout_job(blurb);
    painter.galley(
        pos2(
            rect.left() + PAD,
            name_top + NAME_ROW + (BLURB_ROW - blurb.size().y) / 2.0,
        ),
        blurb,
        tokens.text_secondary,
    );

    if response.has_focus() && keyboard {
        focus::paint(ui, rect, f32::from(tokens.radius_tile));
    }
    response
}

/// "Current", right-aligned at `right`, as an outlined chip.
fn current_badge(ui: &Ui, painter: &Painter, right: f32, centre_y: f32) {
    let tokens = Tokens::get(ui.ctx());
    let galley = painter.layout_no_wrap("Current".to_string(), theme::regular(12.0), tokens.accent);
    let size = vec2(galley.size().x + 12.0, 18.0);
    let chip = Rect::from_min_size(pos2(right - size.x, centre_y - size.y / 2.0), size);
    painter.rect_stroke(
        chip,
        tokens.radius_row,
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    painter.galley(chip.center() - galley.size() / 2.0, galley, tokens.accent);
}

/// A blurred desktop behind a glass tile's window: soft warm, blue and plum
/// spots on a dark or light ground, like the design's desk.
fn glass_desktop(painter: &Painter, rect: Rect, tokens: &Tokens) {
    let ground = if tokens.dark {
        Color32::from_rgb(0x12, 0x15, 0x1c)
    } else {
        Color32::from_rgb(0xe9, 0xed, 0xf3)
    };
    painter.rect_filled(rect, tokens.radius_window.div_ceil(2), ground);
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let spots = if tokens.dark {
        [
            (0.18, 0.30, Color32::from_rgb(0xb8, 0x69, 0x2a)),
            (0.82, 0.72, Color32::from_rgb(0x2b, 0x4f, 0x94)),
            (0.60, 0.10, Color32::from_rgb(0x6b, 0x3a, 0x5e)),
        ]
    } else {
        [
            (0.22, 0.30, Color32::from_rgb(0xf4, 0xc4, 0x8a)),
            (0.80, 0.75, Color32::from_rgb(0x9c, 0xc0, 0xee)),
            (0.60, 0.08, Color32::from_rgb(0xe4, 0xb6, 0xd0)),
        ]
    };
    let reach = rect.width().max(rect.height()) * 0.45;
    for (x, y, colour) in spots {
        let centre = pos2(
            rect.left() + rect.width() * x,
            rect.top() + rect.height() * y,
        );
        // Many faint discs, largest first, make a soft round glow.
        const RINGS: usize = 12;
        for ring in 0..RINGS {
            let share = 1.0 - ring as f32 / RINGS as f32;
            painter.circle_filled(centre, reach * share, colour.gamma_multiply(0.12));
        }
    }
}

/// A Settings window in miniature, drawn only with `tokens`: the window, a
/// 40 px side menu with one item selected, a title, two rows with a rule
/// under each, and an accent chip. Lines stand in for text, so nothing in it
/// is text that could fall under 12 px.
pub fn mini_settings(painter: &Painter, rect: Rect, tokens: &Tokens, pixels_per_point: f32) {
    let line =
        |colour| egui::Stroke::new(theme::snap(tokens.border_width, pixels_per_point), colour);
    let half = |radius: u8| radius.div_ceil(2);
    painter.rect(
        rect,
        half(tokens.radius_window),
        tokens.window_bg,
        line(tokens.rim),
        StrokeKind::Inside,
    );
    let inset = rect.shrink(theme::snap(tokens.border_width, pixels_per_point));
    let menu = Rect::from_min_max(inset.min, pos2(inset.left() + 40.0, inset.bottom()));
    painter.vline(menu.right(), menu.y_range(), line(tokens.border));

    let bar = |at: Rect, colour| painter.rect_filled(at, 1, colour);
    for index in 0..3 {
        let row = Rect::from_min_size(
            pos2(menu.left() + 4.0, menu.top() + 10.0 + index as f32 * 14.0),
            vec2(menu.width() - 8.0, 10.0),
        );
        let selected = index == 0;
        if selected {
            painter.rect_filled(row, half(tokens.radius_row), tokens.selection_bg);
            if tokens.selection_ring {
                painter.rect_stroke(
                    row,
                    half(tokens.radius_row),
                    egui::Stroke::new(theme::snap(1.0, pixels_per_point), tokens.accent),
                    StrokeKind::Inside,
                );
            }
            if tokens.pill_width > 0.0 {
                let pill = theme::snap(tokens.pill_width.min(2.0), pixels_per_point);
                painter.rect_filled(
                    Rect::from_min_size(row.min, vec2(pill, row.height())),
                    0,
                    tokens.accent,
                );
            }
        }
        let colour = if selected {
            tokens.accent
        } else {
            tokens.text_secondary
        };
        bar(
            Rect::from_center_size(
                pos2(row.left() + 4.0 + 10.0, row.center().y),
                vec2(20.0, 3.0),
            ),
            colour,
        );
    }

    let left = menu.right() + 8.0;
    let right = inset.right() - 8.0;
    bar(
        Rect::from_min_size(pos2(left, inset.top() + 10.0), vec2(48.0, 6.0)),
        tokens.text_primary,
    );
    for (index, top) in [28.0, 56.0].into_iter().enumerate() {
        let top = inset.top() + top;
        bar(
            Rect::from_min_size(pos2(left, top), vec2((right - left - 30.0).min(64.0), 3.0)),
            tokens.text_primary,
        );
        bar(
            Rect::from_min_size(pos2(left, top + 7.0), vec2(40.0, 3.0)),
            tokens.text_disabled,
        );
        if index == 0 {
            let chip = Rect::from_min_size(pos2(right - 20.0, top - 1.0), vec2(20.0, 10.0));
            painter.rect_filled(chip, half(tokens.radius_row), tokens.accent);
        } else {
            let chip = Rect::from_min_size(pos2(right - 20.0, top - 1.0), vec2(20.0, 10.0));
            painter.rect_stroke(
                chip,
                half(tokens.radius_row),
                line(tokens.border),
                StrokeKind::Inside,
            );
        }
        painter.hline(left..=right, top + 18.0, line(tokens.border));
    }
    // Where the theme puts an accent line under the search field, the
    // picture has one under its title.
    if tokens.search_line > 0.0 {
        painter.hline(
            left..=right,
            inset.top() + 20.0,
            egui::Stroke::new(
                theme::snap(tokens.search_line, pixels_per_point),
                tokens.accent,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeChoice;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_tile_previews_after_the_delay_and_leaving_puts_the_choice_back() {
        let start = Instant::now();
        let mut preview = HoverPreview::default();
        let paper = Some(ThemeName::Paper);
        assert_eq!(preview.update(paper, true, start), None);
        assert_eq!(preview.update(paper, true, at(start, 60)), None);
        assert!(preview.due_in(at(start, 60)).is_some());
        assert_eq!(preview.update(paper, true, at(start, 130)), paper);
        assert_eq!(preview.due_in(at(start, 130)), None);
        assert_eq!(preview.update(None, false, at(start, 200)), None);
        assert_eq!(preview, HoverPreview::default());
    }

    #[test]
    fn moving_between_tiles_keeps_the_preview_until_the_next_one_is_due() {
        let start = Instant::now();
        let mut preview = HoverPreview::default();
        preview.update(Some(ThemeName::Paper), true, start);
        preview.update(Some(ThemeName::Paper), true, at(start, 120));
        // The gap between two tiles is still inside the picker.
        assert_eq!(
            preview.update(None, true, at(start, 150)),
            Some(ThemeName::Paper)
        );
        assert_eq!(
            preview.update(Some(ThemeName::Terminal), true, at(start, 160)),
            Some(ThemeName::Paper)
        );
        assert_eq!(
            preview.update(Some(ThemeName::Terminal), true, at(start, 290)),
            Some(ThemeName::Terminal)
        );
    }

    #[test]
    fn escape_goes_back_until_another_tile_is_reached() {
        let start = Instant::now();
        let mut preview = HoverPreview::default();
        preview.update(Some(ThemeName::Paper), true, start);
        preview.update(Some(ThemeName::Paper), true, at(start, 130));
        preview.escape(Some(ThemeName::Paper));
        assert_eq!(preview.shown(), None);
        assert_eq!(
            preview.update(Some(ThemeName::Paper), true, at(start, 400)),
            None
        );
        assert_eq!(
            preview.update(Some(ThemeName::Paper), true, at(start, 900)),
            None,
            "resting on the same tile does not bring it back"
        );
        preview.update(Some(ThemeName::Terminal), true, at(start, 1000));
        assert_eq!(
            preview.update(Some(ThemeName::Terminal), true, at(start, 1200)),
            Some(ThemeName::Terminal)
        );
    }

    #[test]
    fn a_click_keeps_showing_the_chosen_theme_without_waiting() {
        let start = Instant::now();
        let mut preview = HoverPreview::default();
        preview.update(Some(ThemeName::HighContrast), true, start);
        preview.choose(ThemeName::HighContrast);
        assert_eq!(
            preview.update(Some(ThemeName::HighContrast), true, at(start, 10)),
            Some(ThemeName::HighContrast)
        );
        assert_eq!(preview.update(None, false, at(start, 20)), None);
    }

    #[test]
    fn up_to_four_tiles_fit_in_a_row() {
        assert_eq!(columns(100.0), 1);
        assert_eq!(columns(347.0), 1);
        assert_eq!(columns(348.0), 2);
        assert_eq!(columns(708.0), 4);
        assert_eq!(columns(2000.0), 4);
        let each = tile_width(708.0, 4);
        assert!(each >= MIN_WIDTH - 0.01);
    }

    fn frame(
        ctx: &egui::Context,
        preview: &mut HoverPreview,
        events: Vec<egui::Event>,
    ) -> Option<ThemeName> {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut picked = None;
        let mut output = ctx.run_ui(input, |ui| {
            picked = picker(ui, ThemeName::Classic, preview);
        });
        output.textures_delta.clear();
        picked
    }

    #[test]
    fn the_picker_draws_every_theme_and_a_click_chooses_one() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let mut preview = HoverPreview::default();
        for _ in 0..2 {
            assert_eq!(frame(&ctx, &mut preview, Vec::new()), None);
        }
        // The second tile of four in a row starts at 180 (168 + 12).
        let on_fluent = pos2(260.0, 60.0);
        frame(
            &ctx,
            &mut preview,
            vec![egui::Event::PointerMoved(on_fluent)],
        );
        let press = |pressed| egui::Event::PointerButton {
            pos: on_fluent,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(&ctx, &mut preview, vec![press(true)]);
        let picked = frame(&ctx, &mut preview, vec![press(false)]);
        assert_eq!(picked, Some(ThemeName::Fluent));
        assert_eq!(theme::previewing(&ctx), Some(ThemeName::Fluent));

        frame(
            &ctx,
            &mut preview,
            vec![egui::Event::PointerMoved(pos2(400.0, 500.0))],
        );
        assert_eq!(theme::previewing(&ctx), None, "leaving ends the preview");
    }

    #[test]
    fn the_mini_window_draws_for_every_theme() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for name in ThemeName::ALL {
                for dark in [true, false] {
                    mini_settings(
                        ui.painter(),
                        Rect::from_min_size(pos2(0.0, 0.0), vec2(152.0, 96.0)),
                        &Tokens::of(name, dark),
                        1.5,
                    );
                }
            }
        });
        output.textures_delta.clear();
    }
}
