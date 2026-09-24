use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{pos2, vec2, Align, CornerRadius, Layout, Rect, Sense, Shadow, StrokeKind, UiBuilder};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest, PaletteEntry, UiSnapshot};
use crate::ui::fuzzy;
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::{keycap, row, text};

/// The panel the user sees.
pub const PANEL_WIDTH: f32 = 680.0;
pub const PANEL_HEIGHT: f32 = 420.0;
/// The shadow is drawn inside the window, so the OS window is the panel plus a
/// 24 px transparent margin on every side.
pub const MARGIN: f32 = 24.0;
pub const WIDTH: f32 = PANEL_WIDTH + MARGIN * 2.0;
pub const HEIGHT: f32 = PANEL_HEIGHT + MARGIN * 2.0;

const SEARCH_HEIGHT: f32 = 52.0;
const FOOTER_HEIGHT: f32 = 32.0;
const ROW_HEIGHT: f32 = 36.0;
const RISE: f32 = 4.0;

pub enum Outcome {
    Stay,
    Hide,
    OpenPlugin(String),
}

#[derive(Default)]
pub struct Palette {
    pub query: String,
    selected: usize,
    focused_once: bool,
    follow_selection: bool,
    matches: Vec<usize>,
}

impl Palette {
    pub fn opened(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.focused_once = false;
        self.follow_selection = true;
        self.matches.clear();
    }

    /// Empty query: everything, in the host's order, which is already grouped.
    /// Otherwise the best matches, still grouped by plugin, with the groups in
    /// the order of their best match.
    fn rank(&mut self, snapshot: &UiSnapshot) {
        let query = self.query.trim();
        self.matches = if query.is_empty() {
            (0..snapshot.commands.len()).collect()
        } else {
            let scored: Vec<(i32, usize)> = snapshot
                .commands
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    fuzzy::score_command(query, &entry.group, &entry.label)
                        .map(|points| (points, index))
                })
                .collect();
            let best_in_group = |group: &str| {
                scored
                    .iter()
                    .filter(|(_, index)| snapshot.commands[*index].group == group)
                    .map(|(points, _)| *points)
                    .max()
                    .unwrap_or(i32::MIN)
            };
            let mut ordered = scored.clone();
            ordered.sort_by(|a, b| {
                let group_a = &snapshot.commands[a.1].group;
                let group_b = &snapshot.commands[b.1].group;
                best_in_group(group_b)
                    .cmp(&best_in_group(group_a))
                    .then_with(|| group_a.cmp(group_b))
                    .then(b.0.cmp(&a.0))
                    .then(a.1.cmp(&b.1))
            });
            ordered.into_iter().map(|(_, index)| index).collect()
        };
        if self.selected >= self.matches.len() {
            self.selected = self.matches.len().saturating_sub(1);
        }
    }

    fn chosen<'a>(&self, snapshot: &'a UiSnapshot) -> Option<&'a PaletteEntry> {
        let index = *self.matches.get(self.selected)?;
        snapshot.commands.get(index)
    }

    /// `appear` runs from 0 to 1 while the palette fades in, and back to 0 as
    /// it fades out; `interactive` is false during the fade out.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
        appear: f32,
        interactive: bool,
    ) -> Outcome {
        self.rank(snapshot);
        let ctx = ui.ctx().clone();
        let tokens = Tokens::get(&ctx);
        let mut outcome = Outcome::Stay;
        let mut run: Option<&PaletteEntry> = None;

        if interactive {
            ctx.input(|input| {
                if input.key_pressed(egui::Key::Escape) {
                    outcome = Outcome::Hide;
                }
                if !self.matches.is_empty() {
                    let count = self.matches.len();
                    if input.key_pressed(egui::Key::ArrowDown) {
                        self.selected = (self.selected + 1) % count;
                        self.follow_selection = true;
                    }
                    if input.key_pressed(egui::Key::ArrowUp) {
                        self.selected = (self.selected + count - 1) % count;
                        self.follow_selection = true;
                    }
                }
            });
            if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                run = self.chosen(snapshot);
            }
        }

        ui.multiply_opacity(appear);
        let window = ui.max_rect();
        let panel = Rect::from_min_size(
            window.min + vec2(MARGIN, MARGIN + (1.0 - appear) * RISE),
            vec2(PANEL_WIDTH, PANEL_HEIGHT),
        );
        let radius = CornerRadius::same(8);
        let painter = ui.painter().clone();
        painter.add(
            Shadow {
                offset: [0, 12],
                blur: 32,
                spread: 0,
                color: tokens.palette_shadow,
            }
            .as_shape(panel, radius),
        );
        painter.rect(
            panel,
            radius,
            tokens.elevated_bg,
            theme::stroke(&ctx, 1.0, tokens.border),
            StrokeKind::Inside,
        );
        let line = theme::stroke(&ctx, 1.0, tokens.border);

        // Search bar: 52 high, 16 px sides, the search icon then the query.
        let search = Rect::from_min_size(panel.min, vec2(PANEL_WIDTH, SEARCH_HEIGHT));
        painter.hline(search.x_range(), search.bottom() - line.width / 2.0, line);
        let icon = Rect::from_center_size(
            pos2(search.left() + 16.0 + 8.0, search.center().y),
            vec2(16.0, 16.0),
        );
        icons::paint(&painter, icon, Icon::Search, tokens.text_disabled);
        let field = Rect::from_min_max(
            pos2(icon.right() + 12.0, search.top()),
            pos2(search.right() - 16.0, search.bottom() - line.width),
        );
        let hint = egui::RichText::new("Type a command\u{2026}")
            .italics()
            .color(tokens.text_disabled)
            .font(theme::regular(15.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id_salt("palette-query")
            .hint_text(hint)
            .font(theme::regular(15.0))
            .text_color(tokens.text_primary)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .vertical_align(Align::Center)
            .desired_width(field.width());
        let response = ui.put(field, edit);
        // Asked for on every frame, not once: the window only receives focus a
        // frame or two after it is shown, and egui drops a focus request made
        // before that, which left the first keystrokes going nowhere.
        if interactive && !response.has_focus() {
            response.request_focus();
        }
        if response.changed() {
            self.selected = 0;
            self.follow_selection = true;
        }

        // Footer: 32 high, 1 px rule above, key hints left, wordmark right.
        let footer = Rect::from_min_max(
            pos2(panel.left(), panel.bottom() - FOOTER_HEIGHT),
            panel.max,
        );
        painter.hline(footer.x_range(), footer.top() + line.width / 2.0, line);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(footer.shrink2(vec2(16.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| footer_hints(ui, &tokens),
        );

        // The list scrolls inside the panel; the window never changes size.
        let list = Rect::from_min_max(
            pos2(panel.left() + 8.0, search.bottom() + 4.0),
            pos2(panel.right() - 8.0, footer.top() - 4.0),
        );
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(list)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(list.intersect(ui.clip_rect()));
                if self.matches.is_empty() {
                    nothing_matches(ui, list, &tokens);
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(list.height())
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let rows = self.matches.clone();
                        let query = self.query.trim().to_string();
                        let mut last_group: Option<&str> = None;
                        for (position, index) in rows.into_iter().enumerate() {
                            let entry = &snapshot.commands[index];
                            if last_group != Some(entry.group.as_str()) {
                                last_group = Some(entry.group.as_str());
                                group_header(ui, &entry.group);
                            }
                            let selected = position == self.selected;
                            let (clicked, rect) =
                                command_row(ui, entry, &query, selected, interactive);
                            if selected && std::mem::take(&mut self.follow_selection) {
                                ui.scroll_to_rect(rect, None);
                            }
                            if clicked {
                                self.selected = position;
                                run = Some(entry);
                            }
                        }
                    });
            },
        );

        // Focus arrives a frame or two after the window is shown, so hiding on
        // "not focused" before it has ever been focused would close the palette
        // in the same breath as opening it.
        match ctx.input(|input| input.viewport().focused) {
            Some(true) => self.focused_once = true,
            Some(false) if self.focused_once && interactive => outcome = Outcome::Hide,
            _ => {}
        }

        if let Some(entry) = run {
            outcome = match (&entry.plugin, entry.disabled) {
                (Some(plugin), true) => Outcome::OpenPlugin(plugin.clone()),
                _ => {
                    to_host.send(HostRequest::RunCommand(entry.id));
                    Outcome::Hide
                }
            };
        }
        outcome
    }
}

/// Group header inside the list: 11 px caps, padding 10 8 4.
fn group_header(ui: &mut egui::Ui, name: &str) {
    let tokens = Tokens::get(ui.ctx());
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        text::single(ui, text::section_job(name, tokens.text_disabled));
    });
    ui.add_space(4.0);
}

/// A label with the matched letters underlined in the accent colour; there is
/// only one weight per family, so the match cannot be shown in bold.
fn label_job(label: &str, query: &str, tokens: &Tokens) -> LayoutJob {
    let matched = if query.is_empty() {
        Vec::new()
    } else {
        fuzzy::positions(query, label).unwrap_or_default()
    };
    let plain = text::format(theme::regular(14.0), tokens.text_primary, Some(20.0));
    let mut hit = text::format(theme::regular(14.0), tokens.accent, Some(20.0));
    hit.underline = egui::Stroke::new(1.0, tokens.accent);
    let mut job = LayoutJob::default();
    for (index, character) in label.chars().enumerate() {
        let format = if matched.contains(&index) {
            hit.clone()
        } else {
            plain.clone()
        };
        let mut buffer = [0u8; 4];
        job.append(character.encode_utf8(&mut buffer), 0.0, format);
    }
    job
}

/// A 36 px row. The cursor row is drawn from state, never from hover; hover
/// only adds the 5 % tint. A command whose plugin is off stays listed at 45 %
/// with an italic "plugin off", so its hotkey does not seem to vanish.
fn command_row(
    ui: &mut egui::Ui,
    entry: &PaletteEntry,
    query: &str,
    selected: bool,
    interactive: bool,
) -> (bool, Rect) {
    let tokens = Tokens::get(ui.ctx());
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), sense);
    if !ui.is_rect_visible(rect) {
        return (false, rect);
    }
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
    if selected {
        row::paint_selection(ui, rect);
    } else if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 4, tokens.hover_bg.gamma_multiply(hover));
    }

    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(8.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            if entry.disabled {
                ui.multiply_opacity(0.45);
            }
            ui.spacing_mut().item_spacing.x = 0.0;
            text::single(ui, label_job(&entry.label, query, &tokens));
            if entry.disabled {
                ui.add_space(10.0);
                let mut hint = text::job(
                    "plugin off",
                    theme::regular(12.0),
                    tokens.text_disabled,
                    None,
                );
                if let Some(section) = hint.sections.first_mut() {
                    section.format.italics = true;
                }
                text::single(ui, hint);
            }
            if !entry.hint.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for key in keycap::split(&entry.hint).into_iter().rev() {
                        keycap::chip(ui, key, 11.0, false);
                    }
                });
            }
        },
    );
    (response.clicked(), rect)
}

fn nothing_matches(ui: &mut egui::Ui, list: Rect, tokens: &Tokens) {
    let block = 26.0 + 4.0 + 17.0;
    ui.add_space(((list.height() - block) / 2.0).max(0.0));
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            text::single(
                ui,
                text::job(
                    "Nothing matches",
                    theme::semibold(22.0),
                    tokens.text_primary,
                    Some(26.0),
                ),
            );
            text::secondary(
                ui,
                "Try a shorter word, or press Esc to close.",
                tokens.text_secondary,
            );
        });
    });
}

/// `↵ Run  ↑↓ Move  Esc Close` and the wordmark. The key symbols are drawn in
/// the monospace family: the return arrow exists only in egui's monospace
/// fallback font, and neither arrow exists in Lora.
fn footer_hints(ui: &mut egui::Ui, tokens: &Tokens) {
    for (index, (key, label)) in [
        ("\u{21B5}", "Run"),
        ("\u{2191}\u{2193}", "Move"),
        ("Esc", "Close"),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            ui.add_space(16.0);
        }
        footer_key(ui, key, tokens);
        ui.add_space(6.0);
        text::single(
            ui,
            text::job(label, theme::regular(11.0), tokens.text_disabled, None),
        );
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        text::single(
            ui,
            text::job(
                "WinCraft",
                theme::semibold(13.0),
                tokens.text_disabled,
                None,
            ),
        );
    });
}

/// The footer's key hints: mono 11, padding 0 5, a plain 1 px border.
fn footer_key(ui: &mut egui::Ui, key: &str, tokens: &Tokens) {
    let galley =
        ui.painter()
            .layout_no_wrap(key.to_string(), theme::mono(11.0), tokens.text_disabled);
    let size = vec2(galley.size().x + 10.0, galley.size().y);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_stroke(
        rect,
        3,
        theme::stroke(ui.ctx(), 1.0, tokens.border),
        StrokeKind::Inside,
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        tokens.text_disabled,
    );
}
