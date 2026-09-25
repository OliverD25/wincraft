use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, FontId, Layout, Rect, Sense, Shadow, StrokeKind,
    UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest, UiSnapshot};
use crate::search::icons::IconCache;
use crate::search::{
    Action, Choice, Context, Glyph, IconRef, Reply, ResultItem, Results, Router, Tone,
};
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
const COMPACT_HEIGHT: f32 = 24.0;
const ROW_PADDING: f32 = 12.0;
const ICON_SIZE: f32 = 16.0;
const TITLE_LINE: f32 = 20.0;
const SUBTITLE_LINE: f32 = 16.0;
const RISE: f32 = 4.0;

pub enum Outcome {
    Stay,
    Hide,
    OpenPlugin(String),
}

pub struct Palette {
    query: String,
    /// The prefix the user typed, drawn as a chip before the search box. The
    /// query holds only what came after it.
    prefix: Option<String>,
    router: Router,
    results: Results,
    /// The prefix and query `results` answer; None when they are out of date.
    searched: Option<(Option<String>, String)>,
    selected: usize,
    focused_once: bool,
    follow_selection: bool,
    /// The query was changed in code, so the text cursor goes to its end.
    cursor_to_end: bool,
    /// Made on the first frame, because it needs the egui context.
    icons: Option<IconCache>,
}

impl Palette {
    pub fn new(router: Router) -> Self {
        Self {
            query: String::new(),
            prefix: None,
            router,
            results: Results::default(),
            searched: None,
            selected: 0,
            focused_once: false,
            follow_selection: false,
            cursor_to_end: false,
            icons: None,
        }
    }

    pub fn opened(&mut self) {
        self.query.clear();
        self.prefix = None;
        self.selected = 0;
        self.focused_once = false;
        self.follow_selection = true;
        self.router.opened();
        self.searched = None;
        if let Some(icons) = &mut self.icons {
            icons.forget_windows();
        }
    }

    /// The host sent a new list of commands.
    pub fn invalidate(&mut self) {
        self.searched = None;
    }

    /// Asks the providers again only when the query changed, because some of
    /// them read the disk or the window list, or when one of them has new
    /// background results. While one still waits for some, the palette wakes
    /// up every 100 ms to check, since nothing else would repaint it.
    fn refresh(&mut self, snapshot: &UiSnapshot, ctx: &egui::Context) {
        let (news, waiting) = self.router.poll();
        if news {
            self.searched = None;
        }
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let wanted = (self.prefix.clone(), self.query.clone());
        if self.searched.as_ref() != Some(&wanted) {
            let at_end = self.selected + 1 >= self.results.items.len();
            self.results =
                self.router
                    .search(self.prefix.as_deref(), &self.query, &context(snapshot));
            // Output streaming in keeps the newest line in view, unless the
            // user moved the cursor up to read something.
            if news && at_end && self.results.follows_end {
                self.selected = self.results.items.len().saturating_sub(1);
                self.follow_selection = true;
            }
            self.searched = Some(wanted);
        }
        if self.selected >= self.results.items.len() {
            self.selected = self.results.items.len().saturating_sub(1);
        }
    }

    fn selected_item(&self) -> Option<&ResultItem> {
        self.results.items.get(self.selected)
    }

    /// Puts `text` in the box as if it had been typed, so a prefix at its
    /// start becomes the chip. Runs nothing.
    pub fn type_in(&mut self, text: &str) {
        match self.router.split(text) {
            Some((prefix, rest)) => {
                let rest = rest.to_string();
                self.prefix = Some(prefix);
                self.set_query(rest);
            }
            None => self.set_query(text.to_string()),
        }
    }

    fn set_query(&mut self, text: String) {
        self.query = text;
        self.selected = 0;
        self.follow_selection = true;
        self.cursor_to_end = true;
    }

    fn set_prefix(&mut self, prefix: Option<String>) {
        self.prefix = prefix;
        self.set_query(String::new());
    }

    /// The selected row's Tab action, when its provider says it can run.
    fn tab_action(&mut self) -> Option<Choice> {
        let choice = self.selected_item()?.tab_action.clone()?;
        let Action::Provider { provider, command } = &choice.action else {
            return Some(choice);
        };
        self.router.available(provider, command).then_some(choice)
    }

    /// Tab: the row's Tab action or completion, if it has one. On a row of
    /// the `?` list it switches to that prefix, the same as Enter.
    fn complete(&mut self) -> Option<Action> {
        if let Some(choice) = self.tab_action() {
            return Some(choice.action);
        }
        let item = self.selected_item()?;
        if let Some(Choice {
            action: action @ Action::SetPrefix(_),
            ..
        }) = &item.enter
        {
            return Some(action.clone());
        }
        let text = item.tab.as_ref()?.text.clone();
        self.set_query(text);
        None
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
        let ctx = ui.ctx().clone();
        self.refresh(snapshot, &ctx);
        let tokens = Tokens::get(&ctx);
        let mut outcome = Outcome::Stay;
        let mut run: Option<Action> = None;

        if interactive {
            let pressed = |key| ctx.input(|input| input.key_pressed(key));
            // Any key but Enter cancels a pending "press Enter again".
            let other_key = ctx.input(|input| {
                input.events.iter().any(|event| match event {
                    egui::Event::Key { key, pressed, .. } => *pressed && *key != egui::Key::Enter,
                    egui::Event::Text(_) => true,
                    _ => false,
                })
            });
            if other_key {
                self.router.interrupted();
            }
            if pressed(egui::Key::Escape) && !self.router.escape(self.prefix.as_deref()) {
                outcome = Outcome::Hide;
            }
            if !self.results.items.is_empty() {
                let count = self.results.items.len();
                if pressed(egui::Key::ArrowDown) {
                    self.selected = (self.selected + 1) % count;
                    self.follow_selection = true;
                }
                if pressed(egui::Key::ArrowUp) {
                    self.selected = (self.selected + count - 1) % count;
                    self.follow_selection = true;
                }
            }
            if let Some(modifiers) = key_modifiers(&ctx, egui::Key::Enter) {
                run = self.selected_item().and_then(|item| {
                    let slot = if modifiers.command {
                        &item.ctrl_enter
                    } else if modifiers.shift {
                        &item.shift_enter
                    } else {
                        &item.enter
                    };
                    slot.as_ref().map(|choice| choice.action.clone())
                });
            }
            let copy_all = key_modifiers(&ctx, egui::Key::C)
                .is_some_and(|modifiers| modifiers.command && modifiers.shift);
            if copy_all {
                if let Some(text) = self.router.copy_all(self.prefix.as_deref()) {
                    to_host.send(HostRequest::RunAction(Action::Copy(text)));
                }
            }
            if pressed(egui::Key::Tab) {
                run = run.or(self.complete());
            }
            // Checked before the text box sees the key: it is the Backspace
            // pressed on an already empty box that takes the prefix away.
            if pressed(egui::Key::Backspace) && self.query.is_empty() && self.prefix.is_some() {
                self.set_prefix(None);
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

        // Search bar: 56 high, 16 px sides, the search icon, the prefix chip
        // when a prefix is on, then the query.
        let search = Rect::from_min_size(panel.min, vec2(PANEL_WIDTH, SEARCH_HEIGHT));
        painter.hline(search.x_range(), search.bottom() - line.width / 2.0, line);
        let icon = Rect::from_center_size(
            pos2(search.left() + 16.0 + 8.0, search.center().y),
            vec2(16.0, 16.0),
        );
        icons::paint(&painter, icon, Icon::Search, tokens.text_disabled);
        let mut field_left = icon.right() + 12.0;
        let placeholder = match &self.prefix {
            Some(prefix) => {
                let (name, placeholder) = self.router.chip(prefix, &self.query);
                let chip = prefix_chip(
                    &ctx,
                    &painter,
                    pos2(field_left, search.center().y),
                    prefix,
                    name,
                    &tokens,
                );
                field_left = chip.right() + 10.0;
                placeholder
            }
            None => "Type a command, app or window\u{2026}",
        };
        let field = Rect::from_min_max(
            pos2(field_left, search.top()),
            pos2(search.right() - 16.0, search.bottom() - line.width),
        );
        let hint = egui::RichText::new(placeholder)
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
            .lock_focus(true)
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
            if self.prefix.is_none() {
                if let Some((prefix, rest)) = self.router.split(&self.query) {
                    let rest = rest.to_string();
                    self.prefix = Some(prefix);
                    self.set_query(rest);
                }
            }
        }
        if std::mem::take(&mut self.cursor_to_end) {
            if let Some(mut state) = egui::TextEdit::load_state(&ctx, response.id) {
                let end = egui::text::CCursor::new(self.query.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(&ctx, response.id);
            }
        }

        // Footer: 36 high, 1 px rule above, key hints left, wordmark right.
        let footer = Rect::from_min_max(
            pos2(panel.left(), panel.bottom() - FOOTER_HEIGHT),
            panel.max,
        );
        painter.hline(footer.x_range(), footer.top() + line.width / 2.0, line);
        let tab_action = self.tab_action().map(|choice| choice.label);
        let hints = footer_keys(self.selected_item(), tab_action, self.results.escape_label);
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(footer.shrink2(vec2(16.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| footer_hints(ui, &tokens, &hints),
        );

        // The list scrolls inside the panel; the window never changes size.
        let list = Rect::from_min_max(
            pos2(panel.left() + 8.0, search.bottom() + 4.0),
            pos2(panel.right() - 8.0, footer.top() - 4.0),
        );
        let waiting = self.prefix.is_some() && self.query.trim().is_empty();
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(list)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(list.intersect(ui.clip_rect()));
                if self.results.items.is_empty() {
                    // Right after a prefix the grey text in the box says what
                    // to type; "Nothing matches" would be wrong.
                    if !waiting {
                        nothing_matches(ui, list, &tokens);
                    }
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(list.height())
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let icons = self.icons.get_or_insert_with(|| IconCache::new(&ctx));
                        let needle = self.results.needle.as_str();
                        let mut last_group: Option<&str> = None;
                        for (position, item) in self.results.items.iter().enumerate() {
                            if last_group != Some(item.group.as_str()) {
                                last_group = Some(item.group.as_str());
                                group_header(ui, &item.group);
                            }
                            let selected = position == self.selected;
                            let (clicked, rect) =
                                result_row(ui, item, needle, selected, interactive, icons);
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
                Action::SetPrefix(prefix) => {
                    self.set_prefix(Some(prefix).filter(|prefix| !prefix.is_empty()));
                    Outcome::Stay
                }
                Action::Provider { provider, command } => {
                    match self.router.act(&provider, &command, &context(snapshot)) {
                        Reply::Stay => {
                            self.searched = None;
                            Outcome::Stay
                        }
                        Reply::ClearQuery => {
                            self.set_query(String::new());
                            Outcome::Stay
                        }
                        Reply::Switch { provider, text } => {
                            if let Some(prefix) = self.router.prefix_of(provider) {
                                self.prefix = Some(prefix);
                                self.set_query(text);
                            }
                            Outcome::Stay
                        }
                        Reply::Hide => Outcome::Hide,
                    }
                }
                Action::Command(id) => {
                    to_host.send(HostRequest::RunCommand(id));
                    Outcome::Hide
                }
                other => {
                    to_host.send(HostRequest::RunAction(other));
                    Outcome::Hide
                }
            };
        }
        outcome
    }
}

/// The active prefix and its provider's name, such as "< Windows": the
/// prefix in accent mono, the name in secondary text, on the keycap chip's
/// panel fill, 1 px border and radius 4. Returns the chip's rectangle so the
/// query can start after it.
fn prefix_chip(
    ctx: &egui::Context,
    painter: &egui::Painter,
    left_centre: egui::Pos2,
    prefix: &str,
    name: &str,
    tokens: &Tokens,
) -> Rect {
    let mark = painter.layout_no_wrap(prefix.to_string(), theme::mono(13.0), tokens.accent);
    let label = painter.layout_no_wrap(
        name.to_string(),
        theme::regular(13.0),
        tokens.text_secondary,
    );
    let height = mark.size().y.max(label.size().y) + 6.0;
    let width = 8.0 + mark.size().x + 6.0 + label.size().x + 8.0;
    let rect = Rect::from_min_size(
        pos2(left_centre.x, left_centre.y - height / 2.0),
        vec2(width, height),
    );
    painter.rect(
        rect,
        4,
        tokens.panel_bg,
        theme::stroke(ctx, 1.0, tokens.border),
        StrokeKind::Inside,
    );
    let mark_width = mark.size().x;
    painter.galley(
        pos2(rect.left() + 8.0, rect.center().y - mark.size().y / 2.0),
        mark,
        tokens.accent,
    );
    painter.galley(
        pos2(
            rect.left() + 8.0 + mark_width + 6.0,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        tokens.text_secondary,
    );
    rect
}

/// The modifiers held when `key` went down this frame. Read from the key's
/// own event: a quick Ctrl+Enter can arrive together with the Ctrl release
/// in one frame, and the frame's modifier state would then say no Ctrl.
fn key_modifiers(ctx: &egui::Context, key: egui::Key) -> Option<egui::Modifiers> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key: pressed_key,
                pressed: true,
                modifiers,
                ..
            } if *pressed_key == key => Some(*modifiers),
            _ => None,
        })
    })
}

/// What every provider may read, from the host's latest snapshot.
fn context(snapshot: &UiSnapshot) -> Context<'_> {
    Context {
        commands: &snapshot.commands,
        plugins: &snapshot.plugins,
        search: &snapshot.search,
        folder: None,
    }
}

/// The keys that do something on the selected row, then the two that always
/// work. A Tab completion without a label works but is not advertised.
fn footer_keys(
    item: Option<&ResultItem>,
    tab_action: Option<String>,
    escape: Option<&'static str>,
) -> Vec<(&'static str, String)> {
    let mut keys = Vec::new();
    if let Some(item) = item {
        if let Some(choice) = &item.enter {
            keys.push(("\u{21B5}", choice.label.clone()));
        }
        if let Some(choice) = &item.shift_enter {
            keys.push(("Shift+\u{21B5}", choice.label.clone()));
        }
        if let Some(choice) = &item.ctrl_enter {
            keys.push(("Ctrl+\u{21B5}", choice.label.clone()));
        }
        if let Some(label) = tab_action {
            keys.push(("Tab", label));
        } else if let Some(completion) = item.tab.as_ref().filter(|tab| !tab.label.is_empty()) {
            keys.push(("Tab", completion.label.clone()));
        }
    }
    // With three row actions at the 12 px chip size the footer would run into
    // the wordmark, and the arrow keys need no reminder.
    if keys.len() < 3 {
        keys.push(("\u{2191}\u{2193}", "Move".to_string()));
    }
    keys.push(("Esc", escape.unwrap_or("Close").to_string()));
    keys
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
fn label_job(
    label: &str,
    query: &str,
    font: FontId,
    colour: Color32,
    tokens: &Tokens,
) -> LayoutJob {
    let matched = if query.is_empty() {
        Vec::new()
    } else {
        fuzzy::positions(query, label).unwrap_or_default()
    };
    let plain = text::format(font.clone(), colour, Some(TITLE_LINE));
    let mut hit = text::format(font, tokens.accent, Some(TITLE_LINE));
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

/// A 44 px row: the 16 px picture, the title 15 with an optional 12 px
/// subtitle under it, and one hotkey chip at the right; a compact row is one
/// 24 px line of monospace output. The cursor row is drawn from state, never
/// from hover; hover only adds the 5 % tint. A command whose plugin is off
/// stays listed at 45 % with an italic "plugin off", so its hotkey does not
/// seem to vanish.
fn result_row(
    ui: &mut egui::Ui,
    entry: &ResultItem,
    query: &str,
    selected: bool,
    interactive: bool,
    icons: &mut IconCache,
) -> (bool, Rect) {
    let tokens = Tokens::get(ui.ctx());
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let height = if entry.compact {
        COMPACT_HEIGHT
    } else {
        ROW_HEIGHT
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
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

    let inner = rect.shrink2(vec2(ROW_PADDING, 0.0));
    // A child that does not move the list's cursor: the row already took its
    // 44 px, and a scope would put the cursor back at the row's middle.
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut child;
    if entry.disabled {
        ui.multiply_opacity(0.45);
    }
    let painter = ui.painter().clone();
    let picture = Rect::from_min_size(
        pos2(inner.left(), rect.center().y - ICON_SIZE / 2.0),
        vec2(ICON_SIZE, ICON_SIZE),
    );
    paint_icon(ui.ctx(), &painter, picture, &entry.icon, icons, &tokens);

    let mut text_right = inner.right();
    if !entry.hint.is_empty() {
        let chips = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            keycap::binding(ui, &keycap::spaced(&entry.hint), false).rect
        });
        text_right = chips.inner.left() - 12.0;
    }
    let text_left = picture.right() + 12.0;

    let off = entry.disabled.then(|| {
        let mut job = text::job(
            "plugin off",
            theme::regular(12.0),
            tokens.text_disabled,
            None,
        );
        if let Some(section) = job.sections.first_mut() {
            section.format.italics = true;
        }
        painter.layout_job(job)
    });
    let off_width = off.as_ref().map(|galley| galley.size().x + 10.0);
    let title_width = text_right - text_left - off_width.unwrap_or(0.0);

    let colour = match entry.tone {
        Tone::Normal => tokens.text_primary,
        Tone::Warning => tokens.warning,
        Tone::Danger => tokens.danger,
    };
    let font = if entry.compact {
        theme::mono(13.0)
    } else {
        theme::regular(15.0)
    };
    let mut title = label_job(&entry.title, query, font, colour, &tokens);
    title.wrap = one_line(title_width);
    let title = painter.layout_job(title);
    let top = if entry.subtitle.is_empty() || entry.compact {
        rect.center().y - TITLE_LINE / 2.0
    } else {
        rect.center().y - (TITLE_LINE + SUBTITLE_LINE) / 2.0
    };
    let title_width = title.size().x;
    painter.galley(pos2(text_left, top), title, colour);
    if let Some(off) = off {
        let y = top + (TITLE_LINE - off.size().y) / 2.0;
        painter.galley(
            pos2(text_left + title_width + 10.0, y),
            off,
            tokens.text_disabled,
        );
    }
    if !entry.subtitle.is_empty() && !entry.compact {
        let mut subtitle = text::job(
            &entry.subtitle,
            theme::regular(12.0),
            tokens.text_secondary,
            Some(SUBTITLE_LINE),
        );
        subtitle.wrap = one_line(text_right - text_left);
        let subtitle = painter.layout_job(subtitle);
        painter.galley(
            pos2(text_left, top + TITLE_LINE),
            subtitle,
            tokens.text_secondary,
        );
    }
    (response.clicked(), rect)
}

/// Long window titles and paths end in an ellipsis instead of running under
/// the keycaps.
fn one_line(width: f32) -> egui::text::TextWrapping {
    egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    }
}

/// A shell icon appears a frame or two after its row, once the icon thread
/// has read it; until then the slot stays empty rather than flashing a stand-in.
fn paint_icon(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    icon: &IconRef,
    icons: &mut IconCache,
    tokens: &Tokens,
) {
    match icon {
        IconRef::None => {}
        IconRef::Glyph(glyph) => {
            let drawn = match glyph {
                Glyph::Command => Icon::ChevronRight,
                Glyph::Search => Icon::Search,
                Glyph::Calculator => Icon::Calculator,
                Glyph::Globe => Icon::Globe,
                Glyph::Terminal => Icon::Terminal,
            };
            icons::paint(painter, rect, drawn, tokens.text_disabled);
        }
        IconRef::Path(_) | IconRef::Window { .. } => {
            if let Some(texture) = icons.get(ctx, icon) {
                let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(texture.id(), rect, whole, egui::Color32::WHITE);
            }
        }
    }
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

/// Such as `↵ Open  Shift+↵ Show in folder  ↑↓ Move  Esc Close`, 12 px with
/// the keys as chips, and the wordmark. The arrows are drawn in the monospace
/// family: the return arrow exists only in egui's monospace fallback font.
fn footer_hints(ui: &mut egui::Ui, tokens: &Tokens, keys: &[(&str, String)]) {
    for (index, (key, label)) in keys.iter().enumerate() {
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
