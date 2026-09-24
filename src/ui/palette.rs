use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{pos2, vec2, Align, CornerRadius, Layout, Rect, Sense, Shadow, StrokeKind, UiBuilder};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest, UiSnapshot};
use crate::search::{Action, Context, ResultItem, Results, Router};
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

const SEARCH_HEIGHT: f32 = 56.0;
const FOOTER_HEIGHT: f32 = 36.0;
const ROW_HEIGHT: f32 = 44.0;
const RISE: f32 = 4.0;

pub enum Outcome {
    Stay,
    Hide,
    OpenPlugin(String),
}

pub struct Palette {
    query: String,
    router: Router,
    results: Results,
    /// The query `results` answer; None when they are out of date.
    searched: Option<String>,
    selected: usize,
    focused_once: bool,
    follow_selection: bool,
}

impl Palette {
    pub fn new(router: Router) -> Self {
        Self {
            query: String::new(),
            router,
            results: Results::default(),
            searched: None,
            selected: 0,
            focused_once: false,
            follow_selection: false,
        }
    }

    pub fn opened(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.focused_once = false;
        self.follow_selection = true;
        self.router.opened();
        self.searched = None;
    }

    /// The host sent a new list of commands.
    pub fn invalidate(&mut self) {
        self.searched = None;
    }

    /// Asks the providers again only when the query changed, because some of
    /// them read the disk or the window list.
    fn refresh(&mut self, snapshot: &UiSnapshot) {
        if self.searched.as_deref() != Some(self.query.as_str()) {
            let context = Context {
                commands: &snapshot.commands,
                plugins: &snapshot.plugins,
            };
            self.results = self.router.search(&self.query, &context);
            self.searched = Some(self.query.clone());
        }
        if self.selected >= self.results.items.len() {
            self.selected = self.results.items.len().saturating_sub(1);
        }
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
        self.refresh(snapshot);
        let ctx = ui.ctx().clone();
        let tokens = Tokens::get(&ctx);
        let mut outcome = Outcome::Stay;
        let mut run: Option<Action> = None;

        if interactive {
            ctx.input(|input| {
                if input.key_pressed(egui::Key::Escape) {
                    outcome = Outcome::Hide;
                }
                if !self.results.items.is_empty() {
                    let count = self.results.items.len();
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
                run = self.chosen_action();
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

        // Search bar: 56 high, 16 px sides, the search icon then the query.
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
            .color(tokens.text_disabled)
            .font(theme::regular(16.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id_salt("palette-query")
            .hint_text(hint)
            .font(theme::regular(16.0))
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

        // Footer: 36 high, 1 px rule above, key hints left, wordmark right.
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
                if self.results.items.is_empty() {
                    nothing_matches(ui, list, &tokens);
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(list.height())
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let needle = self.results.needle.as_str();
                        let mut last_group: Option<&str> = None;
                        for (position, item) in self.results.items.iter().enumerate() {
                            if last_group != Some(item.group.as_str()) {
                                last_group = Some(item.group.as_str());
                                group_header(ui, &item.group);
                            }
                            let selected = position == self.selected;
                            let (clicked, rect) =
                                result_row(ui, item, needle, selected, interactive);
                            if selected && std::mem::take(&mut self.follow_selection) {
                                ui.scroll_to_rect(rect, None);
                            }
                            if clicked {
                                self.selected = position;
                                run = item.enter.as_ref().map(|choice| choice.action.clone());
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

        if let Some(action) = run {
            outcome = match action {
                Action::OpenPlugin(plugin) => Outcome::OpenPlugin(plugin),
                Action::Command(id) => {
                    to_host.send(HostRequest::RunCommand(id));
                    Outcome::Hide
                }
            };
        }
        outcome
    }

    fn chosen_action(&self) -> Option<Action> {
        let item = self.results.items.get(self.selected)?;
        item.enter.as_ref().map(|choice| choice.action.clone())
    }
}

/// Group header inside the list: 24 high, 12 px regular, padding 8 12 2.
fn group_header(ui: &mut egui::Ui, name: &str) {
    let tokens = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        text::single(
            ui,
            text::job(name, theme::regular(12.0), tokens.text_disabled, Some(14.0)),
        );
    });
    ui.add_space(2.0);
}

/// A label with the matched letters underlined in the accent colour; the
/// semibold weight is kept for headings, so the match is not shown in bold.
fn label_job(label: &str, query: &str, tokens: &Tokens) -> LayoutJob {
    let matched = if query.is_empty() {
        Vec::new()
    } else {
        fuzzy::positions(query, label).unwrap_or_default()
    };
    let plain = text::format(theme::regular(15.0), tokens.text_primary, Some(20.0));
    let mut hit = text::format(theme::regular(15.0), tokens.accent, Some(20.0));
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

/// A 44 px row: title 15 with an optional 12 px subtitle under it, and one
/// hotkey chip at the right. The cursor row is drawn from state, never from
/// hover; hover only adds the 5 % tint. A command whose plugin is off stays
/// listed at 45 % with an italic "plugin off", so its hotkey does not seem to
/// vanish.
fn result_row(
    ui: &mut egui::Ui,
    entry: &ResultItem,
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
            .max_rect(rect.shrink2(vec2(12.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            if entry.disabled {
                ui.multiply_opacity(0.45);
            }
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    text::single(ui, label_job(&entry.title, query, &tokens));
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
                });
                if !entry.subtitle.is_empty() {
                    text::single(
                        ui,
                        text::job(
                            &entry.subtitle,
                            theme::regular(12.0),
                            tokens.text_secondary,
                            Some(16.0),
                        ),
                    );
                }
            });
            if !entry.hint.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    keycap::binding(ui, &keycap::spaced(&entry.hint), false);
                });
            }
        },
    );
    (response.clicked(), rect)
}

fn nothing_matches(ui: &mut egui::Ui, list: Rect, tokens: &Tokens) {
    let block = 24.0 + 4.0 + 16.0;
    ui.add_space(((list.height() - block) / 2.0).max(0.0));
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            text::single(
                ui,
                text::job(
                    "Nothing matches",
                    theme::semibold(18.0),
                    tokens.text_primary,
                    Some(24.0),
                ),
            );
            text::caption(
                ui,
                "Try a shorter word, or press Esc to close.",
                tokens.text_secondary,
            );
        });
    });
}

/// `↵ Run  ↑↓ Move  Esc Close`, 12 px with the keys as chips, and the
/// wordmark. The arrows are drawn in the monospace family: the return arrow
/// exists only in egui's monospace fallback font.
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
        let font = if key.is_ascii() {
            theme::regular(12.0)
        } else {
            theme::mono(12.0)
        };
        keycap::hint(ui, key, font);
        ui.add_space(6.0);
        text::single(
            ui,
            text::job(label, theme::regular(12.0), tokens.text_secondary, None),
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
