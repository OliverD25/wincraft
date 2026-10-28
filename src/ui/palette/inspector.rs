//! The 300 px column beside the list that follows the cursor: a picture of
//! the window, the row's title and words, what WinCraft knows about it, the
//! actions with their keys, and where the row comes from.

use egui::{pos2, vec2, Align2, Rect, Sense, StrokeKind};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::CommandDetails;
use crate::core::window_info::{self, WindowDetails};
use crate::search::{PickInfo, ResultItem};
use crate::ui::palette::actions::{Marks, RowAction};
use crate::ui::palette::pinned::PinnedUpdate;
use crate::ui::palette::{badge, row_badges};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::toggle;

pub const WIDTH: f32 = 300.0;
const PADDING: f32 = 16.0;
const GAP: f32 = 14.0;
/// Windows have the most to show, so their blocks sit closer.
const WINDOW_GAP: f32 = 8.0;
const WINDOW_PREVIEW: f32 = 96.0;
const ACTION_HEIGHT: f32 = 34.0;
const META_LINE: f32 = 18.0;
const SOURCE_LINE: f32 = 16.0;

/// Everything the inspector shows for the row under the cursor.
pub struct Inspected<'a> {
    pub item: Option<&'a ResultItem>,
    /// The pinned "Update ready" row is selected: the update's own block
    /// stands in place of the row's title and words.
    pub update: Option<&'a PinnedUpdate>,
    /// What a plugin says about the selected command's state.
    pub command: Option<&'a CommandDetails>,
    /// The selected command waits for a second Enter, which does this.
    pub armed: Option<&'a str>,
    pub marks: &'a Marks,
    pub actions: &'a [RowAction],
    /// Which listed action the keyboard is on, after →.
    pub cursor: Option<usize>,
    pub details: Option<&'a WindowDetails>,
    /// The best match's picks: what was typed, and how often and when.
    pub pick: Option<(String, PickInfo)>,
    pub best: bool,
    pub source: String,
    pub interactive: bool,
}

#[derive(Default)]
pub struct Clicked {
    /// The place in `actions` of the action clicked.
    pub action: Option<usize>,
    /// The "Show on all desktops" switch was flipped to this.
    pub pin: Option<bool>,
    /// Where the live picture of the window goes, when there is one.
    pub preview: Option<Rect>,
}

/// The actions the inspector lists, with their places in the full list.
pub fn listed(actions: &[RowAction]) -> Vec<usize> {
    actions
        .iter()
        .enumerate()
        .filter(|(_, action)| action.listed)
        .map(|(index, _)| index)
        .collect()
}

pub fn show(ui: &mut egui::Ui, rect: Rect, inspected: &Inspected) -> Clicked {
    let tokens = Tokens::get(ui.ctx());
    let mut clicked = Clicked::default();
    let painter = ui.painter_at(rect);
    let inner = rect.shrink(PADDING);
    let mut y = inner.top();
    let window = inspected.item.and_then(ResultItem::window);
    let gap = if window.is_some() { WINDOW_GAP } else { GAP };
    let bottom = inner.bottom() - SOURCE_LINE - gap;

    if !inspected.marks.is_empty() {
        y = marks_block(&painter, inner, y, inspected.marks, &tokens);
    } else if let Some(update) = inspected.update {
        y = update_block(ui, inner, y, update, &tokens);
    } else if let Some(item) = inspected.item {
        if window.is_some() {
            let preview =
                Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), WINDOW_PREVIEW));
            painter.rect(
                preview,
                tokens.radius_tile,
                tokens.panel_bg,
                theme::line(ui.ctx(), tokens.border),
                StrokeKind::Inside,
            );
            clicked.preview = Some(preview.shrink(1.0));
            y = preview.bottom() + gap;
        }
        y = title_block(ui, inner, y, item, inspected.best, &tokens);
        if window.is_none() && !item.subtitle.is_empty() {
            y += 4.0;
            let job = crate::ui::widgets::text::job(
                &item.subtitle,
                theme::regular(13.0),
                tokens.text_secondary,
                Some(18.0),
            );
            let mut job = job;
            job.wrap = egui::text::TextWrapping {
                max_width: inner.width(),
                max_rows: 3,
                break_anywhere: false,
                overflow_character: Some('\u{2026}'),
            };
            let galley = painter.layout_job(job);
            let height = galley.size().y;
            painter.galley(pos2(inner.left(), y), galley, tokens.text_secondary);
            y += height;
        }
        let live = inspected
            .armed
            .map(|text| (text.to_string(), tokens.warning))
            .or_else(|| {
                inspected
                    .command
                    .filter(|details| !details.status.is_empty())
                    .map(|details| (details.status.clone(), tokens.accent))
            });
        if let Some((text, colour)) = live {
            y += 8.0;
            let mut job =
                crate::ui::widgets::text::job(&text, theme::semibold(13.0), colour, Some(18.0));
            job.wrap = egui::text::TextWrapping {
                max_width: inner.width(),
                max_rows: 2,
                break_anywhere: false,
                overflow_character: Some('\u{2026}'),
            };
            let galley = painter.layout_job(job);
            let height = galley.size().y;
            painter.galley(pos2(inner.left(), y), galley, colour);
            y += height;
        }
        let mut meta: Vec<(String, String)> = Vec::new();
        if let Some(details) = inspected.command {
            meta.extend(details.lines.iter().cloned());
        }
        if let Some(details) = inspected.details.filter(|_| window.is_some()) {
            let process = details.process_line();
            if !process.is_empty() {
                meta.push(("Process".to_string(), process));
            }
            let place = details.place_line();
            if !place.is_empty() {
                meta.push(("Monitor".to_string(), place));
            }
            if let Some(bytes) = details.memory {
                meta.push(("Memory".to_string(), window_info::memory_text(bytes)));
            }
        }
        if let Some((typed, pick)) = &inspected.pick {
            let times = if pick.this_month == 1 {
                "once".to_string()
            } else {
                format!("{} times", pick.this_month)
            };
            meta.push((
                "Picked".to_string(),
                format!("for \u{201c}{typed}\u{201d}: {times} this month"),
            ));
            meta.push((
                "Last picked".to_string(),
                crate::core::clock::when(pick.last, crate::search::usage::now()),
            ));
        }
        if !meta.is_empty() {
            y += gap;
            for (key, value) in meta {
                meta_row(&painter, inner, y, &key, &value, &tokens);
                y += META_LINE;
            }
        }
        if let (Some(hwnd), Some(pinned)) = (window, inspected.details.and_then(|d| d.pinned)) {
            y += 6.0;
            let row = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 24.0));
            painter.text(
                pos2(row.left(), row.center().y),
                Align2::LEFT_CENTER,
                "Show on all desktops",
                theme::regular(13.0),
                tokens.text_primary,
            );
            let switch = Rect::from_min_size(
                pos2(row.right() - 40.0, row.center().y - 10.0),
                vec2(40.0, 20.0),
            );
            let mut on = pinned;
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(switch)
                    .id_salt(("pin", hwnd)),
            );
            if inspected.interactive && toggle::toggle(&mut child, &mut on).changed() {
                clicked.pin = Some(on);
            }
            y = row.bottom();
        }
    }

    let listed = listed(inspected.actions);
    if !listed.is_empty() {
        y += gap;
        let room = ((bottom - y) / ACTION_HEIGHT).floor().max(0.0) as usize;
        for (place, index) in listed.iter().take(room).enumerate() {
            let action = &inspected.actions[*index];
            let row =
                Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), ACTION_HEIGHT));
            let id = ui.id().with(("inspector-action", place));
            let sense = if inspected.interactive {
                Sense::click()
            } else {
                Sense::hover()
            };
            let response = ui.interact(row, id, sense);
            let on_cursor = inspected.cursor == Some(place);
            if on_cursor {
                painter.rect_filled(row, tokens.radius_row, tokens.selection_bg);
            } else if response.hovered() {
                painter.rect_filled(row, tokens.radius_row, tokens.hover_bg);
            }
            painter.hline(
                row.x_range(),
                row.bottom() - 0.5,
                theme::line(ui.ctx(), tokens.border),
            );
            let colour = if place == 0 {
                tokens.accent
            } else {
                tokens.text_primary
            };
            let caps = action.key.caps();
            let font = if caps.is_ascii() {
                theme::regular(12.0)
            } else {
                theme::mono(12.0)
            };
            let cap = painter.layout_no_wrap(caps.to_string(), font, tokens.text_secondary);
            let cap_rect = Rect::from_min_size(
                pos2(
                    row.right() - 8.0 - cap.size().x - 12.0,
                    row.center().y - 10.0,
                ),
                vec2(cap.size().x + 12.0, 20.0),
            );
            let mut job =
                crate::ui::widgets::text::job(&action.label, theme::regular(13.0), colour, None);
            job.wrap = egui::text::TextWrapping {
                max_width: (cap_rect.left() - row.left() - 16.0).max(0.0),
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            let label = painter.layout_job(job);
            painter.galley(
                pos2(row.left() + 8.0, row.center().y - label.size().y / 2.0),
                label,
                colour,
            );
            painter.rect(
                cap_rect,
                tokens.radius_row,
                tokens.panel_bg,
                theme::line(ui.ctx(), tokens.border),
                StrokeKind::Inside,
            );
            painter.galley(
                cap_rect.center() - cap.size() / 2.0,
                cap,
                tokens.text_secondary,
            );
            if response.clicked() {
                clicked.action = Some(*index);
            }
            y = row.bottom();
        }
    }

    if !inspected.source.is_empty() {
        let mut job = crate::ui::widgets::text::job(
            &inspected.source,
            theme::regular(12.0),
            tokens.text_disabled,
            Some(SOURCE_LINE),
        );
        job.wrap = egui::text::TextWrapping {
            max_width: inner.width(),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(inner.left(), inner.bottom() - SOURCE_LINE),
            galley,
            tokens.text_disabled,
        );
    }
    clicked
}

/// "3 windows marked", then each window's title and place.
fn marks_block(
    painter: &egui::Painter,
    inner: Rect,
    mut y: f32,
    marks: &Marks,
    tokens: &Tokens,
) -> f32 {
    let count = marks.len();
    let heading = if count == 1 {
        "1 window marked".to_string()
    } else {
        format!("{count} windows marked")
    };
    painter.text(
        pos2(inner.left(), y),
        Align2::LEFT_TOP,
        heading,
        theme::semibold(18.0),
        tokens.text_primary,
    );
    y += 28.0;
    for marked in marks.list().iter().take(6) {
        let line = if marked.place.is_empty() {
            marked.title.clone()
        } else {
            format!("{} \u{b7} {}", marked.title, marked.place)
        };
        let mut job = crate::ui::widgets::text::job(
            &line,
            theme::regular(13.0),
            tokens.text_secondary,
            Some(20.0),
        );
        job.wrap = egui::text::TextWrapping {
            max_width: inner.width(),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        let galley = painter.layout_job(job);
        painter.galley(pos2(inner.left(), y), galley, tokens.text_secondary);
        y += 20.0;
    }
    if count > 6 {
        painter.text(
            pos2(inner.left(), y),
            Align2::LEFT_TOP,
            format!("and {} more", count - 6),
            theme::regular(12.0),
            tokens.text_disabled,
        );
        y += 18.0;
    }
    y
}

/// The title at 18 semibold, wrapped to two lines, and the row's badges.
fn title_block(
    ui: &egui::Ui,
    inner: Rect,
    y: f32,
    item: &ResultItem,
    best: bool,
    tokens: &Tokens,
) -> f32 {
    let painter = ui.painter();
    let size = if item.answer { 24.0 } else { 18.0 };
    let mut job = crate::ui::widgets::text::job(
        &item.title,
        theme::semibold(size),
        tokens.text_primary,
        Some(size + 6.0),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: inner.width(),
        max_rows: 2,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    let height = galley.size().y;
    painter.galley(pos2(inner.left(), y), galley, tokens.text_primary);
    let mut y = y + height;
    let badges = row_badges(item, best);
    if !badges.is_empty() {
        y += 6.0;
        let mut x = inner.left();
        for one in &badges {
            x = badge(ui, pos2(x, y), one, tokens).right() + 6.0;
        }
        y += 18.0;
    }
    y
}

/// The update's block (design 1b after the up arrow): "Update WinCraft",
/// the versions in the mono font with the new one in the accent, what
/// Update does, and the release notes in a "What's new" box.
fn update_block(ui: &egui::Ui, inner: Rect, y: f32, update: &PinnedUpdate, tokens: &Tokens) -> f32 {
    let painter = ui.painter();
    let wrapped = |text: &str, font: egui::FontId, colour, line: f32, rows: usize, width: f32| {
        let mut job = crate::ui::widgets::text::job(text, font, colour, Some(line));
        job.wrap = egui::text::TextWrapping {
            max_width: width,
            max_rows: rows,
            break_anywhere: false,
            overflow_character: Some('\u{2026}'),
        };
        painter.layout_job(job)
    };
    let title = wrapped(
        "Update WinCraft",
        theme::semibold(20.0),
        tokens.text_primary,
        26.0,
        1,
        inner.width(),
    );
    let mut y = y;
    let height = title.size().y;
    painter.galley(pos2(inner.left(), y), title, tokens.text_primary);
    y += height + 6.0;

    let mut versions = egui::text::LayoutJob::default();
    for (text, colour) in [
        (update.current.as_str(), tokens.text_secondary),
        ("  \u{2192}  ", tokens.text_disabled),
        (update.version.as_str(), tokens.accent),
    ] {
        versions.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: theme::mono(13.0),
                color: colour,
                ..Default::default()
            },
        );
    }
    let versions = painter.layout_job(versions);
    let height = versions.size().y;
    painter.galley(pos2(inner.left(), y), versions, tokens.text_secondary);
    y += height + 6.0;

    let said = wrapped(
        &update.description(),
        theme::regular(13.0),
        tokens.text_secondary,
        19.0,
        4,
        inner.width(),
    );
    let height = said.size().y;
    painter.galley(pos2(inner.left(), y), said, tokens.text_secondary);
    y += height;

    let news = update.news(4);
    if !news.is_empty() {
        y += GAP;
        let pad = vec2(14.0, 12.0);
        let width = inner.width() - pad.x * 2.0;
        let label = painter.layout_no_wrap(
            "What's new".to_string(),
            theme::regular(12.0),
            tokens.text_disabled,
        );
        let body = wrapped(
            &news.join("\n"),
            theme::regular(13.0),
            tokens.text_primary,
            19.0,
            5,
            width,
        );
        let height = pad.y * 2.0 + label.size().y + 6.0 + body.size().y;
        let frame = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), height));
        painter.rect(
            frame,
            tokens.radius_row,
            egui::Color32::TRANSPARENT,
            theme::line(ui.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        let label_height = label.size().y;
        painter.galley(frame.min + pad, label, tokens.text_disabled);
        painter.galley(
            frame.min + pad + vec2(0.0, label_height + 6.0),
            body,
            tokens.text_primary,
        );
        y = frame.bottom();
    }
    y
}

/// "Process   claude.exe · PID 18240": the key on the left, the value on
/// the right, both 12 px.
fn meta_row(painter: &egui::Painter, inner: Rect, y: f32, key: &str, value: &str, tokens: &Tokens) {
    let key_galley =
        painter.layout_no_wrap(key.to_string(), theme::regular(12.0), tokens.text_disabled);
    let key_width = key_galley.size().x;
    painter.galley(
        pos2(inner.left(), y + 1.0),
        key_galley,
        tokens.text_disabled,
    );
    let mut job =
        crate::ui::widgets::text::job(value, theme::regular(12.0), tokens.text_secondary, None);
    job.wrap = egui::text::TextWrapping {
        max_width: (inner.width() - key_width - 12.0).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(inner.right() - galley.size().x, y + 1.0),
        galley,
        tokens.text_secondary,
    );
}

/// The window's icon in the middle of the preview, for a window DWM has
/// no picture of, such as one on another desktop.
pub fn no_picture(painter: &egui::Painter, preview: Rect, tokens: &Tokens) {
    let icon = Rect::from_center_size(preview.center(), vec2(24.0, 24.0));
    icons::paint(painter, icon, Icon::Monitor, tokens.text_disabled);
}
