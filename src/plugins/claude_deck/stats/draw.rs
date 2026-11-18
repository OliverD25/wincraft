//! Drawing the Stats tab: the Today and 7 days buttons, then the top
//! chats and the groups as bars, the days as columns (7 days only), the
//! models as one stacked bar, and the busiest hours as a strip of 24
//! columns. Every bar has a tooltip with the raw token counts; a click on
//! a listed chat opens it, as on the Chats tab.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Color32, CursorIcon, Galley, Painter, Rect, Sense, Stroke, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::HostChannel;
use crate::plugins::claude_deck::open_app::open_chat;
use crate::plugins::claude_deck::rows::Look;
use crate::plugins::claude_deck::window::DeckUi;
use crate::ui::widgets::text;

use super::view::{self, Bar};
use super::Spans;

const PAD: f32 = 12.0;
const SIZE: f32 = 13.0;
const SMALL: f32 = 12.0;
const BUTTON_H: f32 = 24.0;
const BUTTON_PAD: f32 = 10.0;
const SECTION: f32 = 28.0;
const BAR_ROW: f32 = 32.0;
const BAR_H: f32 = 6.0;
const COLUMNS_H: f32 = 56.0;
const LABEL_H: f32 = 18.0;
const STACK_H: f32 = 10.0;
const LEGEND_ROW: f32 = 20.0;
const DOT: f32 = 8.0;
const BOTTOM: f32 = 12.0;

fn one_line(painter: &Painter, words: &str, size: f32, colour: Color32, width: f32) -> Arc<Galley> {
    let mut job: LayoutJob = text::job(words, theme::regular(size), colour, None);
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    painter.layout_job(job)
}

fn put(painter: &Painter, at: egui::Pos2, galley: Arc<Galley>) -> f32 {
    let width = galley.size().x;
    painter.galley(at, galley, Color32::PLACEHOLDER);
    width
}

/// One frame of the Stats tab.
pub fn show(ui: &mut Ui, look: &Look, state: &mut DeckUi, to_host: &HostChannel) {
    let tokens = look.tokens;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.add_space(PAD - 4.0);
            buttons(ui, state, tokens);
            let Some(latest) = state.stats.latest.clone() else {
                message(ui, &view::counting(0, 0), tokens);
                return;
            };
            if let Some(progress) = latest.counting {
                message(ui, &view::counting(progress.done, progress.total), tokens);
                return;
            }
            let span = match state.stats.span {
                Spans::Today => &latest.today_span,
                Spans::Week => &latest.week,
            };
            if span.chats.is_empty() {
                message(ui, view::EMPTY, tokens);
                return;
            }
            message(ui, view::NOTE, tokens);
            title(ui, view::CHATS, tokens);
            let chats = view::chat_bars(span, &state.chats, &state.stats.chat_of);
            for bar in &chats {
                if bar_row(ui, bar, tokens) {
                    if let Some(chat) = bar
                        .chat
                        .as_ref()
                        .and_then(|id| state.chats.iter().find(|c| &c.id == id))
                    {
                        open_chat(chat.window, &chat.id, &chat.title, to_host);
                    }
                }
            }
            title(ui, view::GROUPS, tokens);
            for bar in view::group_bars(span, &state.chats, &state.stats.chat_of, &state.saved) {
                bar_row(ui, &bar, tokens);
            }
            if state.stats.span == Spans::Week {
                title(ui, view::DAYS, tokens);
                let labels: Vec<String> = span
                    .days
                    .iter()
                    .map(|(day, _)| view::day_label(*day, latest.today))
                    .collect();
                let values: Vec<f64> = span.days.iter().map(|(_, w)| *w).collect();
                columns(
                    ui,
                    &values,
                    span.weight,
                    &labels,
                    &|at| labels[at].clone(),
                    tokens,
                );
            }
            title(ui, view::MODELS, tokens);
            models(ui, &view::model_bars(span), tokens);
            title(ui, view::HOURS, tokens);
            let labels: Vec<String> = (0..24)
                .map(|h| {
                    if h % 6 == 0 {
                        h.to_string()
                    } else {
                        String::new()
                    }
                })
                .collect();
            columns(
                ui,
                &span.hours,
                span.weight,
                &labels,
                &|h| format!("{h:02}:00\u{2013}{:02}:00", (h + 1) % 24),
                tokens,
            );
            ui.add_space(BOTTOM);
        });
}

/// The Today and 7 days buttons.
fn buttons(ui: &mut Ui, state: &mut DeckUi, tokens: &Tokens) {
    let width = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(width, BUTTON_H), Sense::hover());
    let painter = ui.painter_at(area);
    let mut x = area.left() + PAD;
    for (span, name) in [(Spans::Today, view::TODAY), (Spans::Week, view::WEEK)] {
        let on = state.stats.span == span;
        let font = if on {
            theme::semibold(SMALL)
        } else {
            theme::regular(SMALL)
        };
        let colour = if on {
            tokens.text_primary
        } else {
            tokens.text_secondary
        };
        let words = painter.layout_job(text::job(name, font, colour, None));
        let rect = Rect::from_min_size(
            pos2(x, area.top()),
            vec2(words.size().x + 2.0 * BUTTON_PAD, BUTTON_H),
        );
        let response = ui
            .interact(rect, ui.id().with(("cd-stats-span", name)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        if on {
            painter.rect_filled(rect, 4.0, tokens.selection_bg);
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, tokens.accent),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, 4.0, tokens.hover_bg);
        }
        let size = words.size();
        painter.galley(rect.center() - size / 2.0, words, Color32::PLACEHOLDER);
        if response.clicked() {
            state.stats.span = span;
        }
        x = rect.right() + 6.0;
    }
}

fn message(ui: &mut Ui, words: &str, tokens: &Tokens) {
    let width = ui.available_width();
    let mut job = text::job(words, theme::regular(SMALL), tokens.text_secondary, None);
    job.wrap.max_width = (width - 2.0 * PAD).max(1.0);
    let galley = ui.painter().layout_job(job);
    let (area, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 12.0), Sense::hover());
    ui.painter().galley(
        pos2(area.left() + PAD, area.top() + 8.0),
        galley,
        Color32::PLACEHOLDER,
    );
}

fn title(ui: &mut Ui, words: &str, tokens: &Tokens) {
    let width = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(width, SECTION), Sense::hover());
    let painter = ui.painter_at(area);
    let galley = painter.layout_job(text::job(
        words,
        theme::semibold(SMALL),
        tokens.text_secondary,
        None,
    ));
    let y = area.bottom() - 6.0 - galley.size().y;
    put(&painter, pos2(area.left() + PAD, y), galley);
}

/// A name with its share on the right and a bar under it. True when it
/// was clicked.
fn bar_row(ui: &mut Ui, bar: &Bar, tokens: &Tokens) -> bool {
    let width = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(width, BAR_ROW), Sense::hover());
    let sense = if bar.chat.is_some() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let mut response = ui.interact(area, ui.id().with(("cd-stats-bar", &bar.name)), sense);
    if bar.chat.is_some() {
        response = response.on_hover_cursor(CursorIcon::PointingHand);
    }
    let painter = ui.painter_at(area);
    if response.hovered() {
        painter.rect_filled(area, 0.0, tokens.hover_bg);
    }
    let left = area.left() + PAD;
    let right = area.right() - PAD;
    let share = painter.layout_job(text::job(
        &bar.share,
        theme::regular(SMALL),
        tokens.text_secondary,
        None,
    ));
    let share_w = share.size().x;
    put(&painter, pos2(right - share_w, area.top() + 4.0), share);
    let name = one_line(
        &painter,
        &bar.name,
        SIZE,
        tokens.text_primary,
        right - left - share_w - 8.0,
    );
    put(&painter, pos2(left, area.top() + 3.0), name);
    let track = Rect::from_min_max(
        pos2(left, area.bottom() - BAR_H - 5.0),
        pos2(right, area.bottom() - 5.0),
    );
    painter.rect_filled(track, BAR_H / 2.0, tokens.border);
    let fill = Rect::from_min_size(
        track.min,
        vec2(
            (track.width() * bar.length.clamp(0.0, 1.0)).max(BAR_H),
            BAR_H,
        ),
    );
    painter.rect_filled(fill, BAR_H / 2.0, tokens.accent);
    let clicked = response.clicked();
    response.on_hover_text(&bar.tip);
    clicked
}

/// Even columns with a label under some, each with its share on hover.
fn columns(
    ui: &mut Ui,
    values: &[f64],
    whole: f64,
    labels: &[String],
    name: &dyn Fn(usize) -> String,
    tokens: &Tokens,
) {
    let width = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(width, COLUMNS_H + LABEL_H), Sense::hover());
    let painter = ui.painter_at(area);
    let left = area.left() + PAD;
    let step = (area.width() - 2.0 * PAD) / values.len().max(1) as f32;
    let gap = (step * 0.2).clamp(1.0, 6.0);
    let base = area.top() + COLUMNS_H;
    for (at, height) in view::heights(values).into_iter().enumerate() {
        let x = left + step * at as f32;
        let slot = Rect::from_min_max(pos2(x, area.top()), pos2(x + step, base));
        let h = (COLUMNS_H - 4.0) * height;
        let column = Rect::from_min_max(
            pos2(x + gap / 2.0, base - h.max(1.0)),
            pos2(x + step - gap / 2.0, base),
        );
        let colour = if height > 0.0 {
            tokens.accent
        } else {
            tokens.border
        };
        painter.rect_filled(column, 2.0, colour);
        if let Some(label) = labels.get(at).filter(|l| !l.is_empty()) {
            let galley = painter.layout_job(text::job(
                label,
                theme::regular(SMALL),
                tokens.text_secondary,
                None,
            ));
            let w = galley.size().x;
            let lx = if step >= w { x + (step - w) / 2.0 } else { x };
            put(&painter, pos2(lx, base + 2.0), galley);
        }
        ui.interact(
            slot,
            ui.id().with(("cd-stats-col", at, values.len())),
            Sense::hover(),
        )
        .on_hover_text(format!(
            "{}: {}",
            name(at),
            view::percent(values[at], whole)
        ));
    }
}

/// The colours of the stacked models bar, in order.
fn model_colour(at: usize, tokens: &Tokens) -> Color32 {
    let colours = [
        tokens.accent,
        tokens.success,
        tokens.warning,
        tokens.info,
        tokens.danger,
    ];
    colours.get(at).copied().unwrap_or(tokens.text_secondary)
}

/// One bar cut by model, and a line for each model under it.
fn models(ui: &mut Ui, bars: &[Bar], tokens: &Tokens) {
    let width = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(width, STACK_H + 6.0), Sense::hover());
    let painter = ui.painter_at(area);
    let track = Rect::from_min_size(
        pos2(area.left() + PAD, area.top()),
        vec2(area.width() - 2.0 * PAD, STACK_H),
    );
    painter.rect_filled(track, 2.0, tokens.border);
    let mut x = track.left();
    for (at, bar) in bars.iter().enumerate() {
        let w = track.width() * bar.length.clamp(0.0, 1.0);
        let part = Rect::from_min_size(pos2(x, track.top()), vec2(w, STACK_H));
        painter.rect_filled(part, 0.0, model_colour(at, tokens));
        x += w;
    }
    for (at, bar) in bars.iter().enumerate() {
        let (row, _) = ui.allocate_exact_size(vec2(width, LEGEND_ROW), Sense::hover());
        let painter = ui.painter_at(row);
        let middle = row.center().y;
        painter.circle_filled(
            pos2(row.left() + PAD + DOT / 2.0, middle),
            DOT / 2.0,
            model_colour(at, tokens),
        );
        let share = painter.layout_job(text::job(
            &bar.share,
            theme::regular(SMALL),
            tokens.text_secondary,
            None,
        ));
        let share_w = share.size().x;
        let share_h = share.size().y;
        put(
            &painter,
            pos2(row.right() - PAD - share_w, middle - share_h / 2.0),
            share,
        );
        let name = one_line(
            &painter,
            &bar.name,
            SMALL,
            tokens.text_primary,
            row.width() - 3.0 * PAD - DOT - share_w,
        );
        let name_h = name.size().y;
        put(
            &painter,
            pos2(row.left() + PAD + DOT + 6.0, middle - name_h / 2.0),
            name,
        );
        ui.interact(row, ui.id().with(("cd-stats-model", at)), Sense::hover())
            .on_hover_text(&bar.tip);
    }
}
