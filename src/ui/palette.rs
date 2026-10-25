use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, FontId, Layout, Rect, Sense, StrokeKind, UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::core::tips::{self, Place, TipsConfig};
use crate::core::ui_bridge::{
    HostChannel, HostRequest, HostSetting, Prompt, UiSnapshot, SEARCH_PROMPT,
};
use crate::search::bookmarks as bookmark_store;
use crate::search::icons::IconCache;
use crate::search::providers::bookmarks as bookmark_rows;
use crate::search::{
    Action, Ask, Choice, Context, Glyph, IconRef, Reply, ResultItem, Results, Router, Tone,
};
use crate::ui::fuzzy;
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::tip::{self, STRIP_HEIGHT};
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
const COMPACT_HEIGHT: f32 = 24.0;
const ROW_PADDING: f32 = 12.0;
const ICON_SIZE: f32 = 16.0;
const TITLE_LINE: f32 = 20.0;
const SUBTITLE_LINE: f32 = 16.0;
const RISE: f32 = 4.0;
/// How long the palette stays open after a bookmark was saved, so the
/// confirmation can be read.
const SAVED_FOR: Duration = Duration::from_secs(4);

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
    /// A plugin's question, such as a window's new name. While it is set the
    /// palette is a one-line text box that answers it, not a search.
    prompt: Option<Prompt>,
    /// The prompt's text was put in the box and is selected once, so typing
    /// replaces it.
    select_all: bool,
    /// The host's tip settings and state, refreshed from each snapshot and
    /// sent back whole after this palette changes them.
    tips: TipsConfig,
    tips_synced: bool,
    /// This opening already counted as one show of the folder tip.
    tip_counted: bool,
    /// A bookmark was just saved here: shown as a confirmation.
    saved: Option<Saved>,
    /// Made when first needed, so a palette that never looks at bookmarks
    /// never reads the file.
    bookmarks: Option<bookmark_store::Shared>,
    /// Whether the place the selected row opens is a folder, looked up once.
    place: Option<(String, Place)>,
}

/// The line over the footer.
enum Strip {
    Hint(Place),
    Saved(String),
}

struct Saved {
    phrase: String,
    /// When the palette closes by itself; None keeps it open, as a test
    /// scene needs.
    until: Option<Instant>,
}

/// A test scene started with `--show-tip=bookmark-saved` opens on the
/// confirmation, since a screenshot sends no Enter to save one.
fn scene_saved(scene: Option<&str>) -> Option<Saved> {
    (scene == Some(tips::BOOKMARK_SAVED.id)).then(|| Saved {
        phrase: "pswds".to_string(),
        until: None,
    })
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
            prompt: None,
            select_all: false,
            tips: TipsConfig::default(),
            tips_synced: false,
            tip_counted: false,
            saved: None,
            bookmarks: None,
            place: None,
        }
    }

    pub fn opened(&mut self) {
        self.prompt = None;
        self.select_all = false;
        self.tip_counted = false;
        self.saved = scene_saved(tips::scene_tip().as_deref());
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

    /// Turns the freshly opened palette into a text box for `prompt`.
    pub fn ask(&mut self, prompt: Prompt) {
        self.query = prompt.text.clone();
        self.select_all = true;
        self.prompt = Some(prompt);
    }

    /// A search provider's question, such as the phrase for a bookmark.
    fn ask_for(&mut self, ask: Ask) {
        self.ask(Prompt::from_ask(ask));
    }

    /// The palette was hidden or WinCraft is closing: picks not saved yet
    /// are written now.
    pub fn flush_usage(&mut self) {
        self.router.flush_usage();
    }

    pub fn clear_usage(&mut self) {
        self.router.clear_usage();
        self.searched = None;
    }

    /// The host sent a new list of commands.
    pub fn invalidate(&mut self) {
        self.searched = None;
        self.tips_synced = false;
    }

    fn sync_tips(&mut self, snapshot: &UiSnapshot) {
        if !std::mem::replace(&mut self.tips_synced, true) {
            self.tips = snapshot.tips.clone();
        }
    }

    fn send_tips(&self, to_host: &Arc<HostChannel>) {
        to_host.send(HostRequest::SetHostSetting(HostSetting::Tips(
            self.tips.clone(),
        )));
    }

    fn no_bookmarks(&mut self) -> bool {
        self.bookmarks
            .get_or_insert_with(bookmark_store::shared)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    }

    /// Whether the selected row opens a folder or a file that Ctrl+B can
    /// bookmark. A web address gets no tip.
    fn selected_place(&mut self) -> Option<Place> {
        let target = self
            .selected_item()?
            .bookmark
            .as_ref()
            .and_then(bookmark_rows::path_of)?
            .to_string();
        if let Some((known, place)) = &self.place {
            if *known == target {
                return Some(*place);
            }
        }
        let place = if Path::new(&target).is_dir() {
            Place::Folder
        } else {
            Place::File
        };
        self.place = Some((target, place));
        Some(place)
    }

    /// What goes over the footer now: the confirmation after a save, or the
    /// Ctrl+B tip on a folder or file row while there are no bookmarks. The
    /// first frame the tip is drawn in counts as this opening's one show.
    fn current_strip(&mut self, to_host: &Arc<HostChannel>) -> Option<Strip> {
        if let Some(saved) = &self.saved {
            return Some(Strip::Saved(saved.phrase.clone()));
        }
        let tip = &tips::BOOKMARK_CTRL_B;
        if !self.tips.wanted(tip, self.tip_counted) {
            return None;
        }
        let place = self.selected_place()?;
        if !self.no_bookmarks() {
            return None;
        }
        if !std::mem::replace(&mut self.tip_counted, true) {
            self.tips.record_show(tip);
            self.send_tips(to_host);
        }
        Some(Strip::Hint(place))
    }

    /// A search provider's question is over. A bookmark that was saved keeps
    /// the palette open for a few seconds with a confirmation, and counts as
    /// the use of the feature the folder tip teaches.
    fn after_answer(&mut self, provider: &str, typed: &str, to_host: &Arc<HostChannel>) -> Outcome {
        let Some(phrase) = self.saved_phrase(provider, typed) else {
            return Outcome::Hide;
        };
        let mut changed = self.tips.finish(&tips::BOOKMARK_CTRL_B);
        let confirm = self.tips.wanted(&tips::BOOKMARK_SAVED, false);
        if confirm {
            self.tips.record_show(&tips::BOOKMARK_SAVED);
            changed = true;
        }
        if changed {
            self.send_tips(to_host);
        }
        if !confirm {
            return Outcome::Hide;
        }
        self.leave_prompt();
        self.saved = Some(Saved {
            phrase,
            until: Some(Instant::now() + SAVED_FOR),
        });
        Outcome::Stay
    }

    /// The phrase as saved, when the answer just given saved a bookmark.
    fn saved_phrase(&mut self, provider: &str, typed: &str) -> Option<String> {
        if provider != bookmark_rows::PROVIDER {
            return None;
        }
        self.bookmarks
            .get_or_insert_with(bookmark_store::shared)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .find(typed)
            .map(|bookmark| bookmark.phrase.clone())
    }

    /// Back to the plain palette with an empty box, as when it was opened.
    fn leave_prompt(&mut self) {
        self.prompt = None;
        self.select_all = false;
        self.query.clear();
        self.prefix = None;
        self.selected = 0;
        self.follow_selection = true;
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
        if self.prompt.is_some() {
            return self.show_prompt(ui, snapshot, to_host, appear, interactive);
        }
        let ctx = ui.ctx().clone();
        self.refresh(snapshot, &ctx);
        self.sync_tips(snapshot);
        let tokens = Tokens::get(&ctx);
        let mut outcome = Outcome::Stay;
        let mut run: Option<Action> = None;
        // Set only by Enter or a click, which run the row; Tab never picks.
        let mut picked: Option<String> = None;

        if interactive {
            let pressed = |key| ctx.input(|input| input.key_pressed(key));
            // The confirmation goes at the first key, so typing starts on a
            // clean palette.
            if self.saved.is_some()
                && ctx.input(|input| {
                    input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)
                        )
                    })
                })
            {
                self.saved = None;
            }
            // Any key but Enter cancels a pending "press Enter again".
            let other_key = ctx.input(|input| {
                input.events.iter().any(|event| match event {
                    // Ctrl+Delete is a "press again" itself, like Enter.
                    egui::Event::Key {
                        key: egui::Key::Delete,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.command => false,
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
                if run.is_some() {
                    picked = self.selected_item().and_then(|item| item.usage_key.clone());
                }
            }
            // Ctrl+B and Ctrl+Delete act on the row; the key is taken here so
            // the text box does not also act on it.
            for (key, deletes) in [(egui::Key::B, false), (egui::Key::Delete, true)] {
                let chord = key_modifiers(&ctx, key).is_some_and(|modifiers| {
                    modifiers.command && !modifiers.shift && !modifiers.alt
                });
                let choice = self.selected_item().and_then(|item| {
                    if deletes {
                        item.delete.clone()
                    } else {
                        item.bookmark.clone()
                    }
                });
                if let (true, Some(choice)) = (chord, choice) {
                    ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, key));
                    run = run.or(Some(choice.action));
                }
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

        let strip = self.current_strip(to_host);
        let (panel, painter, line) = paint_panel(ui, &tokens, appear);

        // Search bar: 56 high, 16 px sides, the search icon, the prefix chip
        // when a prefix is on, then the query.
        let search = Rect::from_min_size(panel.min, vec2(PANEL_WIDTH, SEARCH_HEIGHT));
        let rule = search_rule(ui.ctx(), &tokens, line);
        painter.hline(search.x_range(), search.bottom() - rule.width / 2.0, rule);
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

        // The tip takes the bottom of the list area; the window never changes
        // size and the list scrolls in what is left.
        let mut list_bottom = footer.top() - 4.0;
        if let Some(strip) = &strip {
            let rect = Rect::from_min_max(
                pos2(panel.left() + 8.0, list_bottom - STRIP_HEIGHT),
                pos2(panel.right() - 8.0, list_bottom),
            );
            list_bottom = rect.top() - 4.0;
            let (line, icon, colour, dismissable) = match strip {
                Strip::Hint(place) => (
                    tips::bookmark_hint(*place),
                    Icon::Lightbulb,
                    tokens.text_secondary,
                    true,
                ),
                Strip::Saved(phrase) => (
                    tips::bookmark_saved(phrase),
                    Icon::Check,
                    tokens.success,
                    false,
                ),
            };
            let dismissed = tip::strip(ui, rect, icon, colour, &line, dismissable);
            if dismissed && interactive && self.tips.finish(&tips::BOOKMARK_CTRL_B) {
                self.send_tips(to_host);
            }
        }
        let list = Rect::from_min_max(
            pos2(panel.left() + 8.0, search.bottom() + 4.0),
            pos2(panel.right() - 8.0, list_bottom),
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
                                picked = run.as_ref().and(item.usage_key.clone());
                            }
                        }
                    });
            },
        );

        if let Some(until) = self.saved.as_ref().and_then(|saved| saved.until) {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                outcome = Outcome::Hide;
            } else {
                ctx.request_repaint_after(left);
            }
        }

        // Focus arrives a frame or two after the window is shown, so hiding on
        // "not focused" before it has ever been focused would close the palette
        // in the same breath as opening it.
        match ctx.input(|input| input.viewport().focused) {
            Some(true) => self.focused_once = true,
            Some(false) if self.focused_once && interactive => outcome = Outcome::Hide,
            _ => {}
        }

        if let Some(key) = picked {
            self.router
                .record_pick(&key, self.prefix.as_deref(), &self.query, &snapshot.search);
        }
        self.router.save_usage_soon();
        if let Some(action) = run {
            outcome = self.perform(action, snapshot, to_host);
        }
        outcome
    }

    /// What choosing a row, or pressing one of its keys, does.
    fn perform(
        &mut self,
        action: Action,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
    ) -> Outcome {
        match action {
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
                    Reply::Ask(ask) => {
                        self.ask_for(ask);
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
        }
    }

    /// The prompt: the plugin's chip, the text box, and one row saying what
    /// Enter will do. Enter answers, Esc or a click elsewhere cancels.
    fn show_prompt(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
        appear: f32,
        interactive: bool,
    ) -> Outcome {
        let Some(prompt) = self.prompt.clone() else {
            return Outcome::Hide;
        };
        let ctx = ui.ctx().clone();
        let tokens = Tokens::get(&ctx);
        let mut outcome = Outcome::Stay;
        let mut answer = false;
        if interactive {
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                outcome = Outcome::Hide;
            }
            answer = key_modifiers(&ctx, egui::Key::Enter).is_some();
        }

        let (panel, painter, line) = paint_panel(ui, &tokens, appear);
        let search = Rect::from_min_size(panel.min, vec2(PANEL_WIDTH, SEARCH_HEIGHT));
        let rule = search_rule(ui.ctx(), &tokens, line);
        painter.hline(search.x_range(), search.bottom() - rule.width / 2.0, rule);
        let icon = Rect::from_center_size(
            pos2(search.left() + 16.0 + 8.0, search.center().y),
            vec2(16.0, 16.0),
        );
        icons::paint(&painter, icon, Icon::ChevronRight, tokens.text_disabled);
        let chip = prefix_chip(
            &ctx,
            &painter,
            pos2(icon.right() + 12.0, search.center().y),
            "",
            &prompt.chip,
            &tokens,
        );
        let field = Rect::from_min_max(
            pos2(chip.right() + 10.0, search.top()),
            pos2(search.right() - 16.0, search.bottom() - line.width),
        );
        let hint = egui::RichText::new(&prompt.placeholder)
            .color(tokens.text_disabled)
            .font(theme::regular(16.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id_salt("palette-prompt")
            .hint_text(hint)
            .font(theme::regular(16.0))
            .text_color(tokens.text_primary)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .vertical_align(Align::Center)
            .lock_focus(true)
            .desired_width(field.width());
        let response = ui.put(field, edit);
        if interactive && !response.has_focus() {
            response.request_focus();
        }
        // Kept until the box has focus: the window takes a frame or two to
        // get it, and a selection made before that is lost.
        if self.select_all && response.has_focus() {
            if let Some(mut state) = egui::TextEdit::load_state(&ctx, response.id) {
                let end = egui::text::CCursor::new(self.query.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        end,
                    )));
                state.store(&ctx, response.id);
                self.select_all = false;
            }
        }

        let typed = self.query.trim().to_string();
        let can_answer = !typed.is_empty() || prompt.empty_action.is_some();
        let item = ResultItem {
            group: prompt.chip.clone(),
            title: prompt_row_title(&prompt, &typed),
            subtitle: prompt.subtitle.clone(),
            icon: IconRef::Glyph(Glyph::Command),
            // Only the label is used, for the footer; Enter is handled here.
            enter: can_answer.then(|| Choice {
                label: if typed.is_empty() {
                    prompt.empty_action.clone().unwrap_or_default()
                } else {
                    prompt.chip.clone()
                },
                action: Action::SetPrefix(String::new()),
            }),
            ..Default::default()
        };

        let footer = Rect::from_min_max(
            pos2(panel.left(), panel.bottom() - FOOTER_HEIGHT),
            panel.max,
        );
        painter.hline(footer.x_range(), footer.top() + line.width / 2.0, line);
        let hints = footer_keys(Some(&item), None, Some("Cancel"));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(footer.shrink2(vec2(16.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| footer_hints(ui, &tokens, &hints),
        );

        let list = Rect::from_min_max(
            pos2(panel.left() + 8.0, search.bottom() + 4.0),
            pos2(panel.right() - 8.0, footer.top() - 4.0),
        );
        let icons = self.icons.get_or_insert_with(|| IconCache::new(&ctx));
        let clicked = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(list)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(4.0);
                    result_row(ui, &item, "", true, interactive, icons).0
                },
            )
            .inner;
        answer |= clicked;

        match ctx.input(|input| input.viewport().focused) {
            Some(true) => self.focused_once = true,
            Some(false) if self.focused_once && interactive => outcome = Outcome::Hide,
            _ => {}
        }

        if answer && can_answer {
            outcome = self.answer_prompt(&prompt, typed, snapshot, to_host);
        }
        outcome
    }

    /// Enter in the prompt: a plugin's question goes to the host; a search
    /// provider's is answered here, and it may answer with another question.
    fn answer_prompt(
        &mut self,
        prompt: &Prompt,
        typed: String,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
    ) -> Outcome {
        self.sync_tips(snapshot);
        match prompt.plugin.strip_prefix(SEARCH_PROMPT) {
            Some(provider) => {
                match self
                    .router
                    .answered(provider, &prompt.payload, &typed, &context(snapshot))
                {
                    Some(ask) => {
                        self.ask_for(ask);
                        Outcome::Stay
                    }
                    None => self.after_answer(provider, &typed, to_host),
                }
            }
            None => {
                to_host.send(HostRequest::PromptAnswer {
                    plugin: prompt.plugin.clone(),
                    id: prompt.id,
                    target: prompt.target,
                    text: typed,
                });
                Outcome::Hide
            }
        }
    }
}

/// The prompt's row: what Enter does with the text typed, or with none.
fn prompt_row_title(prompt: &Prompt, typed: &str) -> String {
    if !typed.is_empty() {
        return format!("{} \u{201c}{typed}\u{201d}", prompt.action);
    }
    prompt
        .empty_action
        .clone()
        .unwrap_or_else(|| "Type, then press Enter".to_string())
}

/// The rule under the search field: the plain border, or an accent line
/// in themes that draw one.
fn search_rule(ctx: &egui::Context, tokens: &Tokens, line: egui::Stroke) -> egui::Stroke {
    if tokens.search_line > 0.0 {
        theme::stroke(ctx, tokens.search_line, tokens.accent)
    } else {
        line
    }
}

/// The panel with its shadow and border, rising as it fades in. Returns
/// the panel's rectangle, a painter and the 1 px rule.
fn paint_panel(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    appear: f32,
) -> (Rect, egui::Painter, egui::Stroke) {
    let ctx = ui.ctx().clone();
    ui.multiply_opacity(appear);
    let window = ui.max_rect();
    let panel = Rect::from_min_size(
        window.min + vec2(MARGIN, MARGIN + (1.0 - appear) * RISE),
        vec2(PANEL_WIDTH, PANEL_HEIGHT),
    );
    let radius = CornerRadius::same(tokens.radius_window);
    let painter = ui.painter().clone();
    if tokens.shadows {
        painter.add(theme::window_shadow(tokens).as_shape(panel, radius));
    }
    painter.rect(
        panel,
        radius,
        tokens.elevated_bg,
        theme::line(&ctx, tokens.rim),
        StrokeKind::Inside,
    );
    (panel, painter, theme::line(&ctx, tokens.border))
}

/// The active prefix and its provider's name, such as "< Windows": the
/// prefix in accent mono, the name in secondary text, on the keycap chip's
/// panel fill, 1 px border and radius 4. Returns the chip's rectangle so the
/// query can start after it. A prompt's chip has a name and no prefix.
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
    let gap = if prefix.is_empty() { 0.0 } else { 6.0 };
    let width = 8.0 + mark.size().x + gap + label.size().x + 8.0;
    let rect = Rect::from_min_size(
        pos2(left_centre.x, left_centre.y - height / 2.0),
        vec2(width, height),
    );
    painter.rect(
        rect,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(ctx, tokens.border),
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
            rect.left() + 8.0 + mark_width + gap,
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
        if let Some(choice) = &item.bookmark {
            keys.push(("Ctrl+B", choice.label.clone()));
        }
        if let Some(choice) = &item.delete {
            keys.push(("Ctrl+Del", choice.label.clone()));
        }
    }
    let escape = escape.unwrap_or("Close").to_string();
    // Rows that can be bookmarked or removed carry two more keys than the
    // footer has room for; the least needed ones are left out of the footer
    // (they still work).
    if item.is_some_and(|item| item.bookmark.is_some() || item.delete.is_some()) {
        for droppable in ["Ctrl+\u{21B5}", "Shift+\u{21B5}"] {
            if footer_width(&keys) + hint_width("Esc", &escape) <= FOOTER_ROOM {
                break;
            }
            keys.retain(|(key, _)| *key != droppable);
        }
    }
    // With three row actions at the 12 px chip size the footer would run into
    // the wordmark, and the arrow keys need no reminder.
    if keys.len() < 3 {
        keys.push(("\u{2191}\u{2193}", "Move".to_string()));
    }
    keys.push(("Esc", escape));
    keys
}

/// What the key hints may take of the footer: the panel less its side
/// padding and the wordmark at the right.
const FOOTER_ROOM: f32 = 540.0;

/// A close guess at the width of one hint: the keycap, the gap, the label
/// and the space before the next hint.
fn hint_width(key: &str, label: &str) -> f32 {
    (key.chars().count() as f32 * 7.0 + 14.0) + 6.0 + label.chars().count() as f32 * 6.2 + 16.0
}

fn footer_width(keys: &[(&str, String)]) -> f32 {
    keys.iter().map(|(key, label)| hint_width(key, label)).sum()
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

/// A label with only the main run of the match in the accent colour, as the
/// design handout asks; the semibold weight is kept for headings, so the
/// match is not shown in bold.
fn label_job(
    label: &str,
    query: &str,
    font: FontId,
    colour: Color32,
    tokens: &Tokens,
) -> LayoutJob {
    let matched = fuzzy::main_run(query, label).unwrap_or(0..0);
    let plain = text::format(font.clone(), colour, Some(TITLE_LINE));
    let hit = text::format(font, tokens.accent, Some(TITLE_LINE));
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
        tokens.row_height
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
        ui.painter().rect_filled(
            rect,
            tokens.radius_row,
            tokens.hover_bg.gamma_multiply(hover),
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rename_prompt(renamed: bool) -> Prompt {
        Prompt {
            plugin: "window_namer".to_string(),
            id: 1,
            target: 42,
            chip: "Rename".to_string(),
            placeholder: "Name for \u{201c}Untitled - Notepad\u{201d}".to_string(),
            text: String::new(),
            action: "Rename to".to_string(),
            empty_action: renamed.then(|| "Clear the name".to_string()),
            subtitle: "notepad".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn the_prompt_row_says_what_enter_will_do() {
        let prompt = rename_prompt(true);
        assert_eq!(
            prompt_row_title(&prompt, "Work notes"),
            "Rename to \u{201c}Work notes\u{201d}"
        );
        assert_eq!(prompt_row_title(&prompt, ""), "Clear the name");
        assert_eq!(
            prompt_row_title(&rename_prompt(false), ""),
            "Type, then press Enter"
        );
    }

    #[test]
    fn a_prompt_fills_the_box_and_opening_the_palette_drops_it() {
        let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
        let mut prompt = rename_prompt(true);
        prompt.text = "Mail".to_string();
        palette.opened();
        palette.ask(prompt);
        assert_eq!(palette.query, "Mail");
        assert!(palette.select_all);
        assert!(palette.prompt.is_some());
        palette.opened();
        assert!(palette.prompt.is_none());
        assert!(palette.query.is_empty());
    }

    fn row_with_keys(group: &str) -> ResultItem {
        let choice = |label: &str| {
            Some(Choice {
                label: label.to_string(),
                action: Action::SetPrefix(String::new()),
            })
        };
        ResultItem {
            group: group.to_string(),
            enter: choice("Open"),
            shift_enter: choice("Show in folder"),
            ctrl_enter: choice("Copy path"),
            bookmark: choice("Bookmark"),
            ..Default::default()
        }
    }

    fn key_names(keys: &[(&str, String)]) -> Vec<String> {
        keys.iter().map(|(key, _)| key.to_string()).collect()
    }

    #[test]
    fn a_row_that_can_be_bookmarked_advertises_ctrl_b() {
        let keys = footer_keys(Some(&row_with_keys("Files")), None, None);
        assert!(keys.contains(&("Ctrl+B", "Bookmark".to_string())));
        assert_eq!(keys.last(), Some(&("Esc", "Close".to_string())));
        // A row without the slot shows no such key.
        let plain = ResultItem {
            enter: Some(Choice {
                label: "Open".to_string(),
                action: Action::SetPrefix(String::new()),
            }),
            ..Default::default()
        };
        assert!(!key_names(&footer_keys(Some(&plain), None, None)).contains(&"Ctrl+B".to_string()));
    }

    #[test]
    fn the_footer_leaves_out_the_least_needed_keys_instead_of_overflowing() {
        let mut folder = row_with_keys("Folders");
        folder.delete = folder.bookmark.clone().map(|mut choice| {
            choice.label = "Remove".to_string();
            choice
        });
        let keys = footer_keys(Some(&folder), Some("Browse".to_string()), None);
        let width: f32 = footer_width(&keys);
        assert!(width <= FOOTER_ROOM, "{width} for {:?}", key_names(&keys));
        // What the person most needs stays: open, browse, bookmark, remove.
        let names = key_names(&keys);
        for wanted in ["\u{21B5}", "Tab", "Ctrl+B", "Ctrl+Del", "Esc"] {
            assert!(names.contains(&wanted.to_string()), "{wanted} in {names:?}");
        }
        assert!(!names.contains(&"Ctrl+\u{21B5}".to_string()), "{names:?}");
    }

    #[test]
    fn rows_without_the_new_keys_keep_their_footer_as_it_was() {
        let choice = |label: &str| {
            Some(Choice {
                label: label.to_string(),
                action: Action::SetPrefix(String::new()),
            })
        };
        let item = ResultItem {
            enter: choice("Open"),
            shift_enter: choice("Show in folder"),
            ctrl_enter: choice("Copy path"),
            ..Default::default()
        };
        let names = key_names(&footer_keys(Some(&item), Some("Browse".to_string()), None));
        assert_eq!(
            names,
            ["\u{21B5}", "Shift+\u{21B5}", "Ctrl+\u{21B5}", "Tab", "Esc"]
        );
    }

    fn bookmark_palette(store: &crate::search::bookmarks::Shared) -> Palette {
        use crate::search::providers::bookmarks::Bookmarks;
        let mut palette = Palette::new(Router::new(
            vec![(None, Box::new(Bookmarks::with_store(store.clone())))],
            &Default::default(),
        ));
        palette.bookmarks = Some(store.clone());
        palette
    }

    fn new_store() -> crate::search::bookmarks::Shared {
        Arc::new(std::sync::Mutex::new(
            crate::search::bookmarks::Store::in_memory(),
        ))
    }

    fn ctrl_b(target: &str, name: &str) -> Action {
        crate::search::providers::bookmarks::choice(target, name)
            .unwrap()
            .action
    }

    #[test]
    fn ctrl_b_turns_the_palette_into_a_phrase_box_prefilled_with_the_name() {
        let store = new_store();
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        palette.opened();
        let outcome = palette.perform(
            ctrl_b(r"D:\Docs\pswds_OneDrive", "pswds_OneDrive"),
            &snapshot,
            &bridge.to_host,
        );
        assert!(matches!(outcome, Outcome::Stay));
        let prompt = palette.prompt.clone().expect("a question is asked");
        assert_eq!(prompt.plugin, "search:bookmarks");
        assert_eq!(prompt.chip, "Bookmark");
        assert_eq!(prompt.subtitle, r"D:\Docs\pswds_OneDrive");
        assert_eq!(palette.query, "pswds_onedrive");
        assert!(palette.select_all, "typing replaces the suggestion");
        assert_eq!(
            prompt_row_title(&prompt, "pswds"),
            "Save bookmark as \u{201c}pswds\u{201d}"
        );
    }

    #[test]
    fn enter_in_that_box_saves_the_bookmark_here_without_the_host() {
        let store = new_store();
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        palette.opened();
        palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
        let prompt = palette.prompt.clone().unwrap();
        let outcome =
            palette.answer_prompt(&prompt, "pswds".to_string(), &snapshot, &bridge.to_host);
        assert!(
            matches!(outcome, Outcome::Stay),
            "the palette stays for the confirmation"
        );
        assert_eq!(
            store.lock().unwrap().find("pswds").unwrap().target,
            r"D:\Docs\a"
        );
        // The answer itself never goes to the host; only the tips' state does.
        while let Ok(request) = bridge.host_rx.try_recv() {
            assert!(
                matches!(request, HostRequest::SetHostSetting(HostSetting::Tips(_))),
                "only the tips' state is sent"
            );
        }
    }

    fn saved_phrase_of(palette: &Palette) -> Option<&str> {
        palette.saved.as_ref().map(|saved| saved.phrase.as_str())
    }

    #[test]
    fn saving_shows_the_confirmation_and_counts_as_using_the_ctrl_b_tip() {
        let store = new_store();
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        palette.opened();
        palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
        let prompt = palette.prompt.clone().unwrap();
        palette.answer_prompt(&prompt, "  Pswds ".to_string(), &snapshot, &bridge.to_host);
        assert_eq!(saved_phrase_of(&palette), Some("Pswds"));
        assert!(palette.prompt.is_none(), "back in the plain palette");
        assert!(palette.query.is_empty());
        let until = palette.saved.as_ref().and_then(|saved| saved.until);
        assert!(until.is_some_and(|at| at > Instant::now()), "closes later");
        assert!(palette.tips.state[tips::BOOKMARK_CTRL_B.id].done);
        assert_eq!(palette.tips.state[tips::BOOKMARK_SAVED.id].shown, 1);
        let mut sent = None;
        while let Ok(HostRequest::SetHostSetting(HostSetting::Tips(tips))) =
            bridge.host_rx.try_recv()
        {
            sent = Some(tips);
        }
        assert_eq!(sent.as_ref(), Some(&palette.tips), "the host keeps it");
    }

    #[test]
    fn with_tips_off_saving_closes_the_palette_as_before() {
        let store = new_store();
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let mut snapshot = UiSnapshot::default();
        snapshot.tips.show = false;
        palette.opened();
        palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
        let prompt = palette.prompt.clone().unwrap();
        let outcome =
            palette.answer_prompt(&prompt, "pswds".to_string(), &snapshot, &bridge.to_host);
        assert!(matches!(outcome, Outcome::Hide));
        assert!(palette.saved.is_none());
        assert!(store.lock().unwrap().find("pswds").is_some(), "still saved");
    }

    #[test]
    fn the_confirmation_retires_after_three_saves() {
        let store = new_store();
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        for number in 1..=4 {
            palette.opened();
            palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
            let prompt = palette.prompt.clone().unwrap();
            let phrase = format!("phrase{number}");
            let outcome = palette.answer_prompt(&prompt, phrase, &snapshot, &bridge.to_host);
            let stays = matches!(outcome, Outcome::Stay);
            assert_eq!(stays, number <= 3, "save {number}");
        }
    }

    #[test]
    fn a_failed_save_is_not_confirmed() {
        let store = new_store();
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        palette.opened();
        let prompt = Prompt {
            plugin: format!("{SEARCH_PROMPT}bookmarks"),
            payload: "target\t".to_string(),
            ..Default::default()
        };
        let outcome = palette.answer_prompt(&prompt, String::new(), &snapshot, &bridge.to_host);
        assert!(matches!(outcome, Outcome::Hide));
        assert!(palette.saved.is_none());
    }

    /// The width and height of the window the palette is drawn in.
    fn window() -> egui::Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(WIDTH, HEIGHT))
    }

    /// One frame of the palette in a window of its size.
    fn frame(
        palette: &mut Palette,
        ctx: &egui::Context,
        snapshot: &UiSnapshot,
        bridge: &crate::core::ui_bridge::Bridge,
        events: Vec<egui::Event>,
    ) -> Outcome {
        let input = egui::RawInput {
            screen_rect: Some(window()),
            events,
            ..Default::default()
        };
        let mut outcome = Outcome::Stay;
        let mut output = ctx.run_ui(input, |ui| {
            outcome = palette.show(ui, snapshot, &bridge.to_host, 1.0, true);
        });
        output.textures_delta.clear();
        outcome
    }

    fn drawing_context() -> egui::Context {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(
            &ctx,
            crate::core::config::ThemeName::Classic,
            crate::core::config::ThemeChoice::Dark,
        );
        ctx
    }

    #[test]
    fn the_confirmation_closes_the_palette_after_four_seconds_and_goes_at_the_first_key() {
        let store = new_store();
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        let ctx = drawing_context();
        palette.opened();
        palette.saved = Some(Saved {
            phrase: "pswds".to_string(),
            until: Some(Instant::now() + SAVED_FOR),
        });
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert!(matches!(
            frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new()),
            Outcome::Stay
        ));
        assert!(palette.saved.is_some(), "still showing");
        // Typing clears it and the palette stays open for the typing.
        let outcome = frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![egui::Event::Text("d".to_string())],
        );
        assert!(matches!(outcome, Outcome::Stay));
        assert!(palette.saved.is_none());

        palette.saved = Some(Saved {
            phrase: "pswds".to_string(),
            until: Some(Instant::now() - Duration::from_millis(1)),
        });
        assert!(matches!(
            frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new()),
            Outcome::Hide
        ));
    }

    #[test]
    fn a_scene_can_open_on_the_confirmation_and_it_never_times_out() {
        let saved = scene_saved(Some("bookmark-saved")).unwrap();
        assert_eq!(saved.phrase, "pswds");
        assert!(saved.until.is_none());
        assert!(scene_saved(Some("bookmark-ctrl-b")).is_none());
        assert!(scene_saved(None).is_none());
    }

    /// A provider whose rows each open one place, so Ctrl+B applies.
    struct Places(Vec<String>);

    impl crate::search::SearchProvider for Places {
        fn id(&self) -> &'static str {
            "places"
        }
        fn name(&self) -> &'static str {
            "Places"
        }
        fn description(&self) -> &'static str {
            "test rows"
        }
        fn blended(&self) -> bool {
            true
        }
        fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
            self.0
                .iter()
                .map(|target| ResultItem {
                    group: "Places".to_string(),
                    title: target.clone(),
                    bookmark: crate::search::providers::bookmarks::choice(target, "name"),
                    enter: Some(Choice {
                        label: "Open".to_string(),
                        action: Action::Open(target.into()),
                    }),
                    ..Default::default()
                })
                .collect()
        }
    }

    /// A folder of the test's own under the temp folder, so tests running at
    /// the same time do not remove each other's.
    fn scratch_folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wincraft-tips-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A palette over one test row per target, typed into so the rows show,
    /// with the bookmarks in `store`.
    fn places_palette(targets: &[&str], store: &crate::search::bookmarks::Shared) -> Palette {
        let rows = Places(targets.iter().map(|target| target.to_string()).collect());
        let mut palette = Palette::new(Router::new(
            vec![(None, Box::new(rows))],
            &Default::default(),
        ));
        palette.bookmarks = Some(store.clone());
        palette.opened();
        palette.type_in("x");
        palette
    }

    /// What one more frame of the palette puts over the footer.
    fn hint(palette: &mut Palette, bridge: &crate::core::ui_bridge::Bridge) -> Option<Place> {
        let snapshot = UiSnapshot::default();
        frame(palette, &drawing_context(), &snapshot, bridge, Vec::new());
        match palette.current_strip(&bridge.to_host) {
            Some(Strip::Hint(place)) => Some(place),
            _ => None,
        }
    }

    #[test]
    fn a_folder_row_gets_the_tip_and_a_file_row_gets_the_file_wording() {
        let folder = scratch_folder("kinds");
        let file = folder.join("note.txt");
        std::fs::write(&file, "x").unwrap();
        let folder_text = folder.display().to_string();
        let file_text = file.display().to_string();
        let bridge = crate::core::ui_bridge::create();
        let ctx = drawing_context();
        let snapshot = UiSnapshot::default();

        let mut palette = places_palette(&[&folder_text, &file_text], &new_store());
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert_eq!(hint(&mut palette, &bridge), Some(Place::Folder));
        palette.selected = 1;
        assert_eq!(hint(&mut palette, &bridge), Some(Place::File));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn rows_that_cannot_be_bookmarked_as_a_place_get_no_tip() {
        let bridge = crate::core::ui_bridge::create();
        let ctx = drawing_context();
        let snapshot = UiSnapshot::default();
        let mut palette = places_palette(&["https://example.com"], &new_store());
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert_eq!(hint(&mut palette, &bridge), None, "an address");

        let mut none = Palette::new(Router::new(Vec::new(), &Default::default()));
        none.bookmarks = Some(new_store());
        none.opened();
        frame(&mut none, &ctx, &snapshot, &bridge, Vec::new());
        assert_eq!(hint(&mut none, &bridge), None, "no row at all");
    }

    #[test]
    fn once_a_bookmark_exists_the_tip_does_not_come_back() {
        let folder = scratch_folder("exists").display().to_string();
        let bridge = crate::core::ui_bridge::create();
        let store = new_store();
        let mut palette = places_palette(&[&folder], &store);
        assert_eq!(hint(&mut palette, &bridge), Some(Place::Folder));
        store.lock().unwrap().set("any", r"D:\any", 1);
        palette.opened();
        palette.type_in("x");
        assert_eq!(hint(&mut palette, &bridge), None);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn the_tip_counts_one_show_per_opening_not_per_frame_and_retires_after_three() {
        let folder = scratch_folder("count").display().to_string();
        let bridge = crate::core::ui_bridge::create();
        let ctx = drawing_context();
        let snapshot = UiSnapshot::default();
        let mut palette = places_palette(&[&folder], &new_store());
        let shown = |palette: &Palette| {
            palette
                .tips
                .state
                .get(tips::BOOKMARK_CTRL_B.id)
                .map_or(0, |state| state.shown)
        };
        for opening in 1..=3u32 {
            palette.opened();
            palette.type_in("x");
            for _ in 0..5 {
                frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
                assert_eq!(hint(&mut palette, &bridge), Some(Place::Folder));
            }
            assert_eq!(shown(&palette), opening, "opening {opening}");
        }
        palette.opened();
        palette.type_in("x");
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert_eq!(hint(&mut palette, &bridge), None, "the fourth opening");
        assert_eq!(shown(&palette), 3);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn every_counted_show_is_sent_to_the_host_once() {
        let folder = scratch_folder("sent").display().to_string();
        let bridge = crate::core::ui_bridge::create();
        let ctx = drawing_context();
        let snapshot = UiSnapshot::default();
        let mut palette = places_palette(&[&folder], &new_store());
        for _ in 0..4 {
            frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        }
        let mut sent = 0;
        while let Ok(HostRequest::SetHostSetting(HostSetting::Tips(tips))) =
            bridge.host_rx.try_recv()
        {
            assert_eq!(tips.state[tips::BOOKMARK_CTRL_B.id].shown, 1);
            sent += 1;
        }
        assert_eq!(sent, 1);
    }

    #[test]
    fn the_cross_retires_the_tip_for_good_and_the_switch_hides_it() {
        let folder = scratch_folder("cross").display().to_string();
        let bridge = crate::core::ui_bridge::create();
        let ctx = drawing_context();
        let snapshot = UiSnapshot::default();
        let mut palette = places_palette(&[&folder], &new_store());
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        // The strip is the last 32 px of the list area, and the cross sits
        // 8 px in from its right edge.
        let cross = pos2(
            MARGIN + PANEL_WIDTH - 8.0 - 8.0 - 12.0,
            MARGIN + PANEL_HEIGHT - FOOTER_HEIGHT - 4.0 - STRIP_HEIGHT / 2.0,
        );
        let press = |pressed| egui::Event::PointerButton {
            pos: cross,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![egui::Event::PointerMoved(cross)],
        );
        frame(&mut palette, &ctx, &snapshot, &bridge, vec![press(true)]);
        frame(&mut palette, &ctx, &snapshot, &bridge, vec![press(false)]);
        assert!(palette.tips.state[tips::BOOKMARK_CTRL_B.id].done);
        assert_eq!(hint(&mut palette, &bridge), None, "gone in this opening");
        palette.opened();
        palette.type_in("x");
        assert_eq!(hint(&mut palette, &bridge), None, "and in the next");

        let mut off = places_palette(&[&folder], &new_store());
        let mut snapshot = UiSnapshot::default();
        snapshot.tips.show = false;
        frame(&mut off, &ctx, &snapshot, &bridge, Vec::new());
        assert_eq!(hint(&mut off, &bridge), None);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_new_snapshot_brings_in_what_the_settings_page_changed() {
        let folder = scratch_folder("snapshot").display().to_string();
        let bridge = crate::core::ui_bridge::create();
        let ctx = drawing_context();
        let mut snapshot = UiSnapshot::default();
        let mut palette = places_palette(&[&folder], &new_store());
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert!(palette.tips.show);
        snapshot.tips.show = false;
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert!(
            palette.tips.show,
            "the snapshot is read once, until a new one"
        );
        palette.invalidate();
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert!(!palette.tips.show);
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_taken_phrase_asks_again_and_the_second_enter_replaces_it() {
        let store = new_store();
        store.lock().unwrap().set("pswds", r"D:\old", 1);
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        palette.opened();
        palette.perform(ctrl_b(r"D:\new", "new"), &snapshot, &bridge.to_host);
        let first = palette.prompt.clone().unwrap();
        let outcome =
            palette.answer_prompt(&first, "pswds".to_string(), &snapshot, &bridge.to_host);
        assert!(matches!(outcome, Outcome::Stay), "still in the box");
        let second = palette.prompt.clone().unwrap();
        assert_eq!(second.action, "Replace bookmark");
        assert_eq!(palette.query, "pswds");
        assert_eq!(
            store.lock().unwrap().find("pswds").unwrap().target,
            r"D:\old"
        );
        let outcome =
            palette.answer_prompt(&second, "pswds".to_string(), &snapshot, &bridge.to_host);
        assert!(matches!(outcome, Outcome::Stay), "the confirmation shows");
        assert_eq!(saved_phrase_of(&palette), Some("pswds"));
        assert_eq!(
            store.lock().unwrap().find("pswds").unwrap().target,
            r"D:\new"
        );
    }

    fn ctrl_key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn ctrl_delete_pressed_twice_removes_the_selected_bookmark() {
        let store = new_store();
        store.lock().unwrap().set("pswds", r"D:\x", 1);
        let mut palette = bookmark_palette(&store);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(
            &ctx,
            crate::core::config::ThemeName::Classic,
            crate::core::config::ThemeChoice::Dark,
        );
        palette.opened();
        palette.type_in("'");
        let frame = |palette: &mut Palette, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                palette.show(ui, &snapshot, &bridge.to_host, 1.0, true);
            });
            output.textures_delta.clear();
        };
        // The first frames install the fonts and fill the list.
        frame(&mut palette, Vec::new());
        frame(&mut palette, Vec::new());
        let titles = |palette: &Palette| -> Vec<String> {
            palette
                .results
                .items
                .iter()
                .map(|item| item.title.clone())
                .collect()
        };
        assert_eq!(titles(&palette), ["pswds", "Add bookmark\u{2026}"]);
        // The first press only asks for another; the row says so.
        frame(&mut palette, vec![ctrl_key(egui::Key::Delete)]);
        frame(&mut palette, Vec::new());
        assert_eq!(palette.results.items[0].tone, Tone::Warning);
        assert_eq!(store.lock().unwrap().items().len(), 1);
        // The second press removes it. Ctrl+Delete itself must not count as
        // "another key", which would cancel the first press.
        frame(&mut palette, vec![ctrl_key(egui::Key::Delete)]);
        frame(&mut palette, Vec::new());
        assert!(store.lock().unwrap().items().is_empty());
        assert_eq!(
            titles(&palette),
            ["No bookmarks yet", "Add bookmark\u{2026}"]
        );
    }

    #[test]
    fn a_plugins_answer_still_goes_to_the_host() {
        let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
        let bridge = crate::core::ui_bridge::create();
        let outcome = palette.answer_prompt(
            &rename_prompt(true),
            "Work".to_string(),
            &UiSnapshot::default(),
            &bridge.to_host,
        );
        assert!(matches!(outcome, Outcome::Hide));
        assert!(matches!(
            bridge.host_rx.try_recv(),
            Ok(HostRequest::PromptAnswer { id: 1, .. })
        ));
    }
}
