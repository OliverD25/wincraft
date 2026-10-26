mod actions;
mod inspector;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Sense, StrokeKind,
    UiBuilder,
};
use windows_sys::Win32::Foundation::HWND;

use crate::core::theme::{self, Tokens};
use crate::core::tips::{self, Place, TipsConfig};
use crate::core::ui_bridge::{
    HostChannel, HostRequest, HostSetting, Page, Prompt, UiSnapshot, SEARCH_PROMPT,
};
use crate::core::virtual_desktops::Pinner;
use crate::core::window_info::{self, WindowDetails};
use crate::search::bookmarks as bookmark_store;
use crate::search::icons::IconCache;
use crate::search::providers::bookmarks as bookmark_rows;
use crate::search::{
    Action, Ask, Choice, Context, Glyph, IconRef, Reply, ResultItem, Results, Router, Tone,
    SUGGESTED_GROUP,
};
use crate::ui::dwm_thumbs::{self, Placement, Thumbnail};
use crate::ui::fuzzy;
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::tip::{self, STRIP_HEIGHT};
use crate::ui::widgets::{keycap, row, text};
use actions::{Hint, Key, Marks, RowAction, Run, Situation};
use inspector::Inspected;

pub use actions::is_narrow;

/// The panel the user sees. On a monitor narrower than 1100 px the panel
/// is the list alone and the inspector opens over it.
pub const PANEL_WIDTH: f32 = 900.0;
pub const NARROW_PANEL_WIDTH: f32 = PANEL_WIDTH - inspector::WIDTH;
pub const PANEL_HEIGHT: f32 = 520.0;
/// The shadow is drawn inside the window, so the OS window is the panel plus a
/// 24 px transparent margin on every side.
pub const MARGIN: f32 = 24.0;
pub const WIDTH: f32 = PANEL_WIDTH + MARGIN * 2.0;
pub const HEIGHT: f32 = PANEL_HEIGHT + MARGIN * 2.0;

const SEARCH_HEIGHT: f32 = 56.0;
const FOOTER_HEIGHT: f32 = 40.0;
const COMPACT_HEIGHT: f32 = 24.0;
const ROW_PADDING: f32 = 12.0;
const ROW_MIN_HEIGHT: f32 = 44.0;
const ICON_SLOT: f32 = 26.0;
const CHECK_SIZE: f32 = 16.0;
const GROUP_HEADER: f32 = 28.0;
const TITLE_LINE: f32 = 20.0;
const ANSWER_LINE: f32 = 26.0;
const SUBTITLE_LINE: f32 = 16.0;
const RISE: f32 = 4.0;
/// How long the palette stays open after a bookmark was saved, so the
/// confirmation can be read.
const SAVED_FOR: Duration = Duration::from_secs(4);

pub enum Outcome {
    Stay,
    Hide,
    OpenPlugin(String),
    /// Settings → Keyboard shortcuts on a command: its id and name.
    OpenShortcut {
        id: String,
        label: String,
    },
    /// The footer's gear, Ctrl+, or Ctrl+P.
    OpenSettings(Page),
}

pub struct Palette {
    /// Windows' glass shows behind the panel: the window is the panel, with
    /// no room around it for a shadow of its own.
    glass: bool,
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
    /// The monitor it opened on is narrower than 1100 px.
    narrow: bool,
    /// On a narrow monitor, the inspector is open over the list.
    overlay: bool,
    /// After →, the keyboard is in the inspector's action list, here.
    action_cursor: Option<usize>,
    marks: Marks,
    /// The results are the "Try instead" rows, since nothing matched.
    trying: bool,
    /// The palette's own window, which the live window picture is drawn on.
    window: isize,
    thumbnail: Option<(isize, Thumbnail)>,
    /// What the inspector shows about the selected window, read once when
    /// the cursor reaches it.
    details: Option<(isize, WindowDetails)>,
    /// Made when first needed; None inside when the shell's interface for
    /// pinning is not there.
    pinner: Option<Option<Pinner>>,
    /// The search box's id from the last frame, to read its text cursor
    /// before it is drawn again.
    query_id: Option<egui::Id>,
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

/// Where a run came from, so only a row's own Enter or click counts as a
/// pick for the ranking.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Via {
    Pick,
    Other,
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
            narrow: false,
            glass: false,
            overlay: false,
            action_cursor: None,
            marks: Marks::default(),
            trying: false,
            window: 0,
            thumbnail: None,
            details: None,
            pinner: None,
            query_id: None,
        }
    }

    /// The palette's own window, which the live picture of the selected
    /// window is drawn on.
    pub fn set_window(&mut self, hwnd: isize) {
        self.window = hwnd;
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
        self.overlay = false;
        self.action_cursor = None;
        self.marks.clear();
        self.details = None;
        self.thumbnail = None;
        if let Some(icons) = &mut self.icons {
            icons.forget_windows();
        }
    }

    /// The palette's window was hidden: the live picture goes with it.
    pub fn hidden(&mut self) {
        self.thumbnail = None;
    }

    /// On a monitor narrower than 1100 px the panel is the list alone.
    pub fn set_narrow(&mut self, narrow: bool) {
        self.narrow = narrow;
    }

    pub fn panel_width(&self) -> f32 {
        if self.narrow {
            NARROW_PANEL_WIDTH
        } else {
            PANEL_WIDTH
        }
    }

    /// The OS window's width: the panel and the shadow margins.
    pub fn window_width(&self) -> f32 {
        self.panel_width() + self.margin() * 2.0
    }

    pub fn window_height(&self) -> f32 {
        PANEL_HEIGHT + self.margin() * 2.0
    }

    /// The transparent room for the panel's shadow; none with the glass,
    /// where the window is the panel.
    pub fn margin(&self) -> f32 {
        if self.glass {
            0.0
        } else {
            MARGIN
        }
    }

    pub fn glass(&self) -> bool {
        self.glass
    }

    pub fn set_glass(&mut self, on: bool) {
        self.glass = on;
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
    /// up every 100 ms to check, since nothing else would repaint it. When
    /// nothing matches a query typed without a prefix, the rows are the
    /// "Try instead" ones.
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
            self.trying = self.results.items.is_empty()
                && self.prefix.is_none()
                && !self.query.trim().is_empty();
            if self.trying {
                self.results.items = actions::try_instead(&self.query, &snapshot.search);
            }
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
        self.action_cursor = None;
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

    /// The selected row is the best match from the picks.
    fn on_best_match(&self) -> bool {
        self.results.best_match && self.selected == 0 && !self.trying
    }

    /// What the selected row can do; with windows marked, what the marked
    /// windows can do together.
    fn current_actions(&mut self) -> Vec<RowAction> {
        if !self.marks.is_empty() {
            return actions::marks_actions(&self.marks.windows());
        }
        let tab_action = self.tab_action();
        let best_match = self.on_best_match();
        let Some(item) = self.selected_item() else {
            return Vec::new();
        };
        let situation = Situation {
            tab_action,
            best_match,
            marked: item.window().is_some_and(|hwnd| self.marks.contains(hwnd)),
        };
        actions::row_actions(item, &situation)
    }

    /// The text cursor is at the end of the box with nothing selected, so →
    /// is free to open the actions and Ctrl+C to copy a row.
    fn text_cursor_at_end(&self, ctx: &egui::Context) -> bool {
        self.text_cursor(ctx).is_none_or(|(primary, secondary)| {
            primary == secondary && primary == self.query.chars().count()
        })
    }

    fn nothing_selected_in_box(&self, ctx: &egui::Context) -> bool {
        self.text_cursor(ctx)
            .is_none_or(|(primary, secondary)| primary == secondary)
    }

    fn text_cursor(&self, ctx: &egui::Context) -> Option<(usize, usize)> {
        let state = egui::TextEdit::load_state(ctx, self.query_id?)?;
        let range = state.cursor.char_range()?;
        Some((range.primary.index.into(), range.secondary.index.into()))
    }

    /// The place line of a window, for the marked list.
    fn window_place(item: &ResultItem) -> String {
        match item.window() {
            Some(hwnd) => {
                crate::search::start_com();
                window_info::read(hwnd as HWND, None).place_line()
            }
            None => String::new(),
        }
    }

    /// Reads the selected window's details once, when the cursor gets there.
    fn refresh_details(&mut self) {
        let Some(hwnd) = self.selected_item().and_then(ResultItem::window) else {
            return;
        };
        if self
            .details
            .as_ref()
            .is_some_and(|(known, _)| *known == hwnd)
        {
            return;
        }
        crate::search::start_com();
        let pinner = self
            .pinner
            .get_or_insert_with(|| match Pinner::new() {
                Ok(pinner) => Some(pinner),
                Err(err) => {
                    log::info!("palette: pinning windows to all desktops is not available: {err}");
                    None
                }
            })
            .as_ref();
        let details = window_info::read(hwnd as HWND, pinner);
        self.details = Some((hwnd, details));
    }

    /// The inspector's last line: where the row comes from.
    fn source_line(&self, item: &ResultItem, snapshot: &UiSnapshot) -> String {
        if self.trying || item.provider.is_empty() {
            return String::new();
        }
        match item.provider.as_str() {
            "windows" => {
                let program = self
                    .details
                    .as_ref()
                    .filter(|(hwnd, _)| Some(*hwnd) == item.window())
                    .map(|(_, details)| details.process.clone())
                    .filter(|process| !process.is_empty())
                    .unwrap_or_else(|| {
                        item.subtitle
                            .split(" \u{b7} ")
                            .next()
                            .unwrap_or_default()
                            .to_string()
                    });
                format!("Open window \u{b7} {program}")
            }
            "calc" => "Built-in calculator".to_string(),
            "commands" => match &item.origin {
                Some(plugin) => snapshot
                    .plugins
                    .iter()
                    .find(|info| info.id == *plugin)
                    .map(|info| format!("From {} {}", info.name, info.version))
                    .unwrap_or_else(|| "From a plugin".to_string()),
                None => format!("WinCraft {}", snapshot.version),
            },
            "apps" => "App from the Start menu".to_string(),
            "files" => "File search".to_string(),
            "paths" => "Folders and files".to_string(),
            "bookmarks" => "Your bookmark".to_string(),
            "terminal" => "Terminal".to_string(),
            "web" => "Ask AI and web search".to_string(),
            other => self
                .router
                .provider_name(other)
                .map(|name| format!("From {name}"))
                .unwrap_or_default(),
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
        if self.prompt.is_some() {
            self.thumbnail = None;
            return self.show_prompt(ui, snapshot, to_host, appear, interactive);
        }
        let ctx = ui.ctx().clone();
        self.refresh(snapshot, &ctx);
        self.sync_tips(snapshot);
        self.refresh_details();
        let tokens = Tokens::get(&ctx);
        let mut outcome = Outcome::Stay;
        let mut run: Option<(Run, Via)> = None;
        let mut actions = self.current_actions();
        let listed = inspector::listed(&actions);

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
            if pressed(egui::Key::Escape) {
                let left_actions =
                    self.action_cursor.take().is_some() | std::mem::take(&mut self.overlay);
                if left_actions {
                    // Back to the list first.
                } else if self.marks.clear() {
                    actions = self.current_actions();
                } else if !self.router.escape(self.prefix.as_deref()) {
                    outcome = Outcome::Hide;
                }
            }
            let ctrl = |key| {
                key_modifiers(&ctx, key).is_some_and(|modifiers| {
                    modifiers.command && !modifiers.shift && !modifiers.alt
                })
            };
            if ctrl(egui::Key::Comma) {
                outcome = Outcome::OpenSettings(Page::General);
            }
            if ctrl(egui::Key::P) {
                ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::P));
                outcome = Outcome::OpenSettings(Page::Plugins);
            }
            if let Some(at) = self.action_cursor {
                if pressed(egui::Key::ArrowDown) && !listed.is_empty() {
                    self.action_cursor = Some((at + 1) % listed.len());
                }
                if pressed(egui::Key::ArrowUp) && !listed.is_empty() {
                    self.action_cursor = Some((at + listed.len() - 1) % listed.len());
                }
                if key_modifiers(&ctx, egui::Key::ArrowLeft).is_some() {
                    ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
                    });
                    self.action_cursor = None;
                    if self.narrow {
                        self.overlay = false;
                    }
                }
            } else if !self.results.items.is_empty() {
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
            match key_modifiers(&ctx, egui::Key::ArrowRight) {
                Some(modifiers) if modifiers.shift && !modifiers.command => {
                    if let Some(action) = actions.iter().find(|a| a.key == Key::ShiftRight) {
                        ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight)
                        });
                        run = Some((action.run.clone(), Via::Other));
                    }
                }
                Some(modifiers)
                    if modifiers.is_none()
                        && self.action_cursor.is_none()
                        && !listed.is_empty()
                        && self.text_cursor_at_end(&ctx) =>
                {
                    ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                    });
                    self.action_cursor = Some(0);
                    self.overlay = self.narrow;
                }
                _ => {}
            }
            if let Some(modifiers) = key_modifiers(&ctx, egui::Key::Enter) {
                let chosen = match self.action_cursor {
                    Some(at) => listed.get(at).map(|index| &actions[*index]),
                    None => {
                        let key = if modifiers.command {
                            Key::CtrlEnter
                        } else if modifiers.shift {
                            Key::ShiftEnter
                        } else {
                            Key::Enter
                        };
                        actions.iter().find(|action| action.key == key)
                    }
                };
                if let Some(action) = chosen {
                    let via = if self.marks.is_empty()
                        && matches!(action.key, Key::Enter | Key::ShiftEnter | Key::CtrlEnter)
                    {
                        Via::Pick
                    } else {
                        Via::Other
                    };
                    run = Some((action.run.clone(), via));
                }
            }
            // These act on the row; the key is taken here so the text box
            // does not also act on it.
            let chords = [
                (egui::Key::B, Key::CtrlB),
                (egui::Key::Delete, Key::CtrlDelete),
                (egui::Key::K, Key::CtrlK),
                (egui::Key::W, Key::CtrlW),
                (egui::Key::M, Key::CtrlM),
                (egui::Key::Space, Key::CtrlSpace),
                (egui::Key::C, Key::CtrlC),
            ];
            for (key, slot) in chords {
                if !ctrl(key) {
                    continue;
                }
                if slot == Key::CtrlC && !self.nothing_selected_in_box(&ctx) {
                    continue;
                }
                if let Some(action) = actions.iter().find(|action| action.key == slot) {
                    ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, key));
                    run = run.or(Some((action.run.clone(), Via::Other)));
                }
            }
            if ctrl(egui::Key::A) {
                let marked = self.marks.mark_all(&self.results.items, Self::window_place);
                if marked > 0 {
                    ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::COMMAND, egui::Key::A)
                    });
                    actions = self.current_actions();
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
                if let Some(action) = self.complete() {
                    run = run.or(Some((Run::Action(action), Via::Other)));
                }
            }
            // Checked before the text box sees the key: it is the Backspace
            // pressed on an already empty box that takes the prefix away.
            if pressed(egui::Key::Backspace) && self.query.is_empty() && self.prefix.is_some() {
                self.set_prefix(None);
            }
        }

        let strip = self.current_strip(to_host);
        let panel_width = self.panel_width();
        let (panel, painter, line) = paint_panel(ui, &tokens, appear, panel_width, self.glass);

        // Search bar: 56 high, 16 px sides, the search icon, the scope chip
        // when one is on, the query, and the Ask AI pill at the right.
        let search = Rect::from_min_size(panel.min, vec2(panel_width, SEARCH_HEIGHT));
        let rule = search_rule(ui.ctx(), &tokens, line);
        painter.hline(search.x_range(), search.bottom() - rule.width / 2.0, rule);
        let icon = Rect::from_center_size(
            pos2(search.left() + 16.0 + 8.0, search.center().y),
            vec2(16.0, 16.0),
        );
        icons::paint(&painter, icon, Icon::Search, tokens.text_disabled);
        let mut field_left = icon.right() + 12.0;
        let ask_prefix = self.router.ask_prefix();
        let asking = ask_prefix.is_some() && self.prefix == ask_prefix;
        let pill = ask_prefix.as_ref().map(|_| {
            let size = ask_pill_size(&painter);
            Rect::from_min_size(
                pos2(
                    search.right() - 16.0 - size.x,
                    search.center().y - size.y / 2.0,
                ),
                size,
            )
        });
        if let Some(pill) = pill {
            if ask_pill(ui, pill, asking, interactive) && !asking {
                self.prefix = ask_prefix.clone();
                self.selected = 0;
                self.cursor_to_end = true;
            }
        }
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
        let field_right = pill.map_or(search.right() - 16.0, |pill| pill.left() - 12.0);
        let field = Rect::from_min_max(
            pos2(field_left, search.top()),
            pos2(field_right, search.bottom() - line.width),
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
        self.query_id = Some(response.id);
        // Asked for on every frame, not once: the window only receives focus a
        // frame or two after it is shown, and egui drops a focus request made
        // before that, which left the first keystrokes going nowhere.
        if interactive && !response.has_focus() {
            response.request_focus();
        }
        if response.changed() {
            self.selected = 0;
            self.follow_selection = true;
            self.action_cursor = None;
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

        // Footer: 40 high, the gear, the selected row's keys, the version.
        let footer = Rect::from_min_max(
            pos2(panel.left(), panel.bottom() - FOOTER_HEIGHT),
            panel.max,
        );
        let escape = if !self.marks.is_empty() {
            "Clear marks"
        } else if self.action_cursor.is_some() || self.overlay {
            "Back"
        } else {
            self.results.escape_label.unwrap_or("Close")
        };
        let clear_scope = self.prefix.is_some() && self.query.is_empty();
        let hints = actions::footer_hints(&actions, clear_scope, escape);
        if paint_footer(
            ui,
            footer,
            &tokens,
            line,
            hints,
            &snapshot.version,
            interactive,
        ) {
            outcome = Outcome::OpenSettings(Page::General);
        }

        // The list on the left, the inspector at the right; on a narrow
        // monitor the inspector opens over the list.
        let body_top = search.bottom();
        let body_bottom = footer.top();
        let inspector_rect = Rect::from_min_max(
            pos2(panel.right() - inspector::WIDTH, body_top),
            pos2(panel.right(), body_bottom),
        );
        let list_right = if self.narrow {
            panel.right()
        } else {
            inspector_rect.left()
        };

        // The tip takes the bottom of the list area; the window never changes
        // size and the list scrolls in what is left.
        let mut list_bottom = body_bottom - 4.0;
        if let Some(strip) = &strip {
            let rect = Rect::from_min_max(
                pos2(panel.left() + 8.0, list_bottom - STRIP_HEIGHT),
                pos2(list_right - 8.0, list_bottom),
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
            pos2(panel.left() + 8.0, body_top + 4.0),
            pos2(list_right - 8.0, list_bottom),
        );
        let waiting = self.prefix.is_some() && self.query.trim().is_empty();
        let mut clicked_row: Option<(usize, bool)> = None;
        let verb = actions
            .first()
            .filter(|action| action.key == Key::Enter)
            .map(|action| actions::enter_verb(&action.label));
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
                        if self.trying {
                            no_match_line(ui, &tokens);
                        }
                        let icons = self.icons.get_or_insert_with(|| IconCache::new(&ctx));
                        let needle = self.results.needle.as_str();
                        let column = !self.marks.is_empty();
                        let mut last_group: Option<&str> = None;
                        for (position, item) in self.results.items.iter().enumerate() {
                            let best = position == 0 && self.results.best_match && !self.trying;
                            let group = if best {
                                "Best match"
                            } else {
                                item.group.as_str()
                            };
                            if last_group != Some(group) {
                                last_group = Some(group);
                                group_header(ui, group);
                            }
                            let selected = position == self.selected;
                            let look = RowLook {
                                needle,
                                selected,
                                interactive,
                                column,
                                marked: item.window().is_some_and(|hwnd| self.marks.contains(hwnd)),
                                best,
                                verb: if selected && self.marks.is_empty() {
                                    verb.as_deref()
                                } else {
                                    None
                                },
                            };
                            let row = result_row(ui, item, &look, icons);
                            if selected && std::mem::take(&mut self.follow_selection) {
                                ui.scroll_to_rect(row.rect, None);
                            }
                            if row.clicked {
                                clicked_row = Some((position, row.shift));
                            }
                        }
                    });
            },
        );
        if let Some((position, shift)) = clicked_row {
            let item = &self.results.items[position];
            if shift && item.window().is_some() {
                let place = Self::window_place(item);
                let item = item.clone();
                self.marks.toggle(&item, || place);
                actions = self.current_actions();
            } else {
                self.selected = position;
                self.action_cursor = None;
                run = item
                    .enter
                    .as_ref()
                    .map(|choice| (Run::Action(choice.action.clone()), Via::Pick));
            }
        }

        // The inspector, for the row under the cursor.
        let show_inspector = !self.narrow || self.overlay;
        let mut preview = None;
        if show_inspector {
            let rect = if self.narrow {
                let rect = Rect::from_min_max(
                    pos2(panel.right() - inspector::WIDTH - 8.0, body_top + 8.0),
                    pos2(panel.right() - 8.0, body_bottom - 8.0),
                );
                if tokens.shadows {
                    painter.add(
                        theme::window_shadow(&tokens)
                            .as_shape(rect, CornerRadius::same(tokens.radius_window)),
                    );
                }
                painter.rect(
                    rect,
                    tokens.radius_window,
                    tokens.elevated_bg,
                    theme::line(&ctx, tokens.rim),
                    StrokeKind::Inside,
                );
                rect
            } else {
                painter.vline(
                    inspector_rect.left() + line.width / 2.0,
                    inspector_rect.y_range(),
                    line,
                );
                inspector_rect
            };
            let item = self.selected_item().cloned();
            let pick = match (&item, self.on_best_match()) {
                (Some(item), true) => item.usage_key.as_ref().and_then(|key| {
                    self.router
                        .pick_info(key, self.prefix.as_deref(), &self.query)
                        .map(|info| (self.query.trim().to_string(), info))
                }),
                _ => None,
            };
            let source = item
                .as_ref()
                .map(|item| self.source_line(item, snapshot))
                .unwrap_or_default();
            let details = self
                .details
                .as_ref()
                .filter(|(hwnd, _)| item.as_ref().and_then(ResultItem::window) == Some(*hwnd))
                .map(|(_, details)| details);
            let inspected = Inspected {
                item: item.as_ref(),
                marks: &self.marks,
                actions: &actions,
                cursor: self.action_cursor,
                details,
                pick,
                best: self.on_best_match(),
                source,
                interactive,
            };
            let clicked = ui
                .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    inspector::show(ui, rect, &inspected)
                })
                .inner;
            if let Some(index) = clicked.action {
                run = Some((actions[index].run.clone(), Via::Other));
            }
            if let (Some(on), Some(hwnd)) =
                (clicked.pin, item.as_ref().and_then(ResultItem::window))
            {
                to_host.send(HostRequest::RunAction(Action::AllDesktops { hwnd, on }));
                if let Some((_, details)) = self.details.as_mut() {
                    details.pinned = Some(on);
                }
            }
            if let (Some(rect), Some(hwnd)) =
                (clicked.preview, item.as_ref().and_then(ResultItem::window))
            {
                preview = Some((hwnd, rect));
            }
        }
        self.place_thumbnail(&ctx, &painter, &tokens, preview, appear);

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

        if let Some((_, Via::Pick)) = &run {
            if let Some(key) = self.selected_item().and_then(|item| item.usage_key.clone()) {
                self.router.record_pick(
                    &key,
                    self.prefix.as_deref(),
                    &self.query,
                    &snapshot.search,
                );
            }
        }
        self.router.save_usage_soon();
        if let Some((run, _)) = run {
            let next = self.run(run, snapshot, to_host);
            if !matches!(next, Outcome::Stay) {
                outcome = next;
            }
        }
        outcome
    }

    /// The live picture of the selected window, drawn by DWM on top of the
    /// palette's own window in the inspector's preview box.
    fn place_thumbnail(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        tokens: &Tokens,
        preview: Option<(isize, Rect)>,
        appear: f32,
    ) {
        let Some((hwnd, rect)) = preview.filter(|_| self.window != 0) else {
            self.thumbnail = None;
            return;
        };
        if self
            .thumbnail
            .as_ref()
            .is_none_or(|(known, _)| *known != hwnd)
        {
            self.thumbnail = Thumbnail::register(self.window as HWND, hwnd as HWND)
                .map(|thumbnail| (hwnd, thumbnail));
        }
        let Some((_, thumbnail)) = self.thumbnail.as_mut() else {
            inspector::no_picture(painter, rect, tokens);
            return;
        };
        let Some(size) = thumbnail.source_size() else {
            thumbnail.place(None);
            inspector::no_picture(painter, rect, tokens);
            return;
        };
        let source = dwm_thumbs::picture_source(dwm_thumbs::visible_frame(hwnd as HWND), size);
        let fitted = dwm_thumbs::fit(source.size(), rect.size());
        let dest = Rect::from_center_size(rect.center(), fitted);
        let placement = dwm_thumbs::crop(dest, rect, source).map(|(shown, cropped)| Placement {
            dest: dwm_thumbs::physical(shown, ctx.pixels_per_point()),
            source: dwm_thumbs::physical(cropped, 1.0),
            opacity: (appear.clamp(0.0, 1.0) * 255.0).round() as u8,
        });
        thumbnail.place(placement);
    }

    /// Runs what a key, a click or the inspector chose.
    fn run(&mut self, run: Run, snapshot: &UiSnapshot, to_host: &Arc<HostChannel>) -> Outcome {
        match run {
            Run::Action(action) => self.perform(action, snapshot, to_host),
            Run::Complete(text) => {
                self.set_query(text);
                Outcome::Stay
            }
            Run::Forget => {
                if let Some(key) = self.selected_item().and_then(|item| item.usage_key.clone()) {
                    self.router
                        .forget_pick(&key, self.prefix.as_deref(), &self.query);
                    self.searched = None;
                    self.action_cursor = None;
                }
                Outcome::Stay
            }
            Run::Mark => {
                if let Some(item) = self.selected_item().cloned() {
                    let place = Self::window_place(&item);
                    self.marks.toggle(&item, || place);
                }
                Outcome::Stay
            }
        }
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
            Action::ChangeShortcut { id, label } => Outcome::OpenShortcut { id, label },
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

        let panel_width = self.panel_width();
        let (panel, painter, line) = paint_panel(ui, &tokens, appear, panel_width, self.glass);
        let search = Rect::from_min_size(panel.min, vec2(panel_width, SEARCH_HEIGHT));
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
        let mut hints: Vec<Hint> = Vec::new();
        if let Some(choice) = &item.enter {
            hints.push(Hint {
                key: Key::Enter.caps(),
                label: choice.label.clone(),
                accent: false,
            });
        }
        hints.push(Hint {
            key: "Esc",
            label: "Cancel".to_string(),
            accent: false,
        });
        if paint_footer(
            ui,
            footer,
            &tokens,
            line,
            hints,
            &snapshot.version,
            interactive,
        ) {
            outcome = Outcome::OpenSettings(Page::General);
        }

        let list = Rect::from_min_max(
            pos2(panel.left() + 8.0, search.bottom() + 4.0),
            pos2(panel.right() - 8.0, footer.top() - 4.0),
        );
        let icons = self.icons.get_or_insert_with(|| IconCache::new(&ctx));
        let look = RowLook {
            needle: "",
            selected: true,
            interactive,
            column: false,
            marked: false,
            best: false,
            verb: None,
        };
        let clicked = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(list)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(4.0);
                    result_row(ui, &item, &look, icons).clicked
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
    width: f32,
    glass: bool,
) -> (Rect, egui::Painter, egui::Stroke) {
    let ctx = ui.ctx().clone();
    ui.multiply_opacity(appear);
    let window = ui.max_rect();
    // With the glass the panel is the window: no margin, and no room to rise.
    let (margin, rise) = if glass {
        (0.0, 0.0)
    } else {
        (MARGIN, (1.0 - appear) * RISE)
    };
    let panel = Rect::from_min_size(
        window.min + vec2(margin, margin + rise),
        vec2(width, PANEL_HEIGHT),
    );
    let radius = CornerRadius::same(tokens.radius_window);
    let painter = ui.painter().clone();
    if tokens.shadows && !glass {
        painter.add(theme::window_shadow(tokens).as_shape(panel, radius));
    }
    let fill = if glass {
        theme::with_alpha(tokens.elevated_bg, tokens.glass_alpha)
    } else {
        tokens.elevated_bg
    };
    painter.rect(
        panel,
        radius,
        fill,
        theme::line(&ctx, tokens.rim),
        StrokeKind::Inside,
    );
    if tokens.top_highlight.a() > 0 {
        let inset = theme::line(&ctx, tokens.rim).width;
        let r = f32::from(tokens.radius_window);
        painter.hline(
            panel.left() + r..=panel.right() - r,
            panel.top() + inset + 0.5,
            theme::stroke(&ctx, 1.0, tokens.top_highlight),
        );
    }
    (panel, painter, theme::line(&ctx, tokens.border))
}

/// The scope chip, such as "w Windows": 24 high, 1 px accent border,
/// radius 4, the prefix in mono and the scope's name in accent, both 12 px.
/// Returns the chip's rectangle so the query can start after it. A
/// prompt's chip has a name and no prefix.
fn prefix_chip(
    ctx: &egui::Context,
    painter: &egui::Painter,
    left_centre: egui::Pos2,
    prefix: &str,
    name: &str,
    tokens: &Tokens,
) -> Rect {
    let mark = painter.layout_no_wrap(prefix.to_string(), theme::mono(12.0), tokens.accent);
    let label = painter.layout_no_wrap(name.to_string(), theme::regular(12.0), tokens.accent);
    let height = 24.0;
    let gap = if prefix.is_empty() { 0.0 } else { 6.0 };
    let width = 8.0 + mark.size().x + gap + label.size().x + 8.0;
    let rect = Rect::from_min_size(
        pos2(left_centre.x, left_centre.y - height / 2.0),
        vec2(width, height),
    );
    painter.rect(
        rect,
        4,
        Color32::TRANSPARENT,
        theme::line(ctx, tokens.accent),
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
        tokens.accent,
    );
    rect
}

const ASK_LABEL: &str = "Ask AI";

fn ask_pill_size(painter: &egui::Painter) -> egui::Vec2 {
    let label = painter.layout_no_wrap(ASK_LABEL.to_string(), theme::regular(13.0), Color32::WHITE);
    vec2(12.0 + 14.0 + 6.0 + label.size().x + 8.0 + 20.0 + 6.0, 28.0)
}

/// The search bar's "Ask AI" pill: 28 high, round, a sparkles icon in the
/// accent colour, the label and a "?" keycap. Returns true when clicked.
fn ask_pill(ui: &mut egui::Ui, rect: Rect, active: bool, interactive: bool) -> bool {
    let tokens = Tokens::get(ui.ctx());
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui
        .interact(rect, ui.id().with("ask-ai-pill"), sense)
        .on_hover_text("Ask AI in your browser \u{b7} type ? first");
    let painter = ui.painter();
    let fill = if response.hovered() {
        tokens.hover_bg
    } else {
        Color32::TRANSPARENT
    };
    let edge = if active { tokens.accent } else { tokens.border };
    painter.rect(
        rect,
        14,
        fill,
        theme::line(ui.ctx(), edge),
        StrokeKind::Inside,
    );
    let sparkles = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.center().y - 7.0),
        vec2(14.0, 14.0),
    );
    icons::paint(painter, sparkles, Icon::Sparkles, tokens.accent);
    let label = painter.layout_no_wrap(
        ASK_LABEL.to_string(),
        theme::regular(13.0),
        tokens.text_secondary,
    );
    let label_left = sparkles.right() + 6.0;
    let label_width = label.size().x;
    painter.galley(
        pos2(label_left, rect.center().y - label.size().y / 2.0),
        label,
        tokens.text_secondary,
    );
    let cap = Rect::from_min_size(
        pos2(label_left + label_width + 8.0, rect.center().y - 10.0),
        vec2(20.0, 20.0),
    );
    painter.rect(
        cap,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    painter.text(
        cap.center(),
        Align2::CENTER_CENTER,
        "?",
        theme::regular(12.0),
        tokens.text_secondary,
    );
    response.clicked()
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

/// The footer: 40 high on the elevated fill with a rule above it, the
/// Settings gear at the far left and a divider, the key hints, and the
/// version at the right. Returns true when the gear was clicked.
fn paint_footer(
    ui: &mut egui::Ui,
    footer: Rect,
    tokens: &Tokens,
    line: egui::Stroke,
    hints: Vec<Hint>,
    version: &str,
    interactive: bool,
) -> bool {
    let painter = ui.painter().clone();
    let radius = tokens.radius_window;
    painter.rect_filled(
        footer.shrink(line.width),
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius,
            se: radius,
        },
        tokens.elevated_bg,
    );
    painter.hline(footer.x_range(), footer.top() + line.width / 2.0, line);
    let gear = Rect::from_min_size(
        pos2(footer.left() + 8.0, footer.center().y - 14.0),
        vec2(28.0, 28.0),
    );
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui
        .interact(gear, ui.id().with("palette-settings"), sense)
        .on_hover_text("Settings \u{b7} Ctrl+,");
    if response.hovered() {
        painter.rect_filled(gear, tokens.radius_row, tokens.hover_bg);
    }
    icons::paint(
        &painter,
        Rect::from_center_size(gear.center(), vec2(16.0, 16.0)),
        Icon::Settings,
        tokens.text_secondary,
    );
    let divider_x = gear.right() + 8.0;
    painter.vline(
        divider_x,
        footer.center().y - 8.0..=footer.center().y + 8.0,
        line,
    );

    let version_text = format!("v{version}");
    let version_galley =
        painter.layout_no_wrap(version_text, theme::mono(12.0), tokens.text_disabled);
    let name_galley = painter.layout_no_wrap(
        "WinCraft".to_string(),
        theme::semibold(13.0),
        tokens.text_disabled,
    );
    let version_left = footer.right() - 16.0 - version_galley.size().x;
    let name_left = version_left - 6.0 - name_galley.size().x;
    painter.galley(
        pos2(name_left, footer.center().y - name_galley.size().y / 2.0),
        name_galley,
        tokens.text_disabled,
    );
    painter.galley(
        pos2(
            version_left,
            footer.center().y - version_galley.size().y / 2.0,
        ),
        version_galley,
        tokens.text_disabled,
    );

    let hints_left = divider_x + 12.0;
    let room = (name_left - 16.0 - hints_left).max(0.0);
    let hints = actions::fit_hints(hints, room);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(hints_left, footer.top()),
                pos2(name_left - 16.0, footer.bottom()),
            ))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| footer_hints(ui, tokens, &hints),
    );
    response.clicked()
}

/// Group header inside the list: 28 high, 12 px, sitting at the bottom
/// with 4 px under it, 12 px in from the left.
fn group_header(ui: &mut egui::Ui, name: &str) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), GROUP_HEADER), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        name,
        theme::regular(12.0),
        tokens.text_disabled,
    );
}

/// "No apps, windows or commands match." over the "Try instead" rows.
fn no_match_line(ui: &mut egui::Ui, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), 12.0 + 18.0 + 16.0),
        Sense::hover(),
    );
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 12.0),
        Align2::LEFT_TOP,
        "No apps, windows or commands match.",
        theme::regular(13.0),
        tokens.text_disabled,
    );
}

/// A label with only the main run of the match in the accent colour, as the
/// design handout asks; the semibold weight is kept for headings, so the
/// match is not shown in bold.
fn label_job(
    label: &str,
    query: &str,
    font: FontId,
    colour: Color32,
    line_height: f32,
    tokens: &Tokens,
) -> LayoutJob {
    let matched = fuzzy::main_run(query, label).unwrap_or(0..0);
    let plain = text::format(font.clone(), colour, Some(line_height));
    let hit = text::format(font, tokens.accent, Some(line_height));
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

/// A small outlined pill after a title.
#[derive(Debug, PartialEq)]
pub(crate) enum Badge {
    /// "open", with a success dot.
    Open,
    /// "often picked", with a clock.
    OftenPicked,
    /// A text such as "0.20.20 available", with an accent dot.
    Update(String),
}

/// The badges a row carries: "open" on a window beside other rows, "often
/// picked" on the best match, and the command's own text, such as "0.20.20
/// available" on Update WinCraft.
pub(crate) fn row_badges(item: &ResultItem, best: bool) -> Vec<Badge> {
    let mut badges = Vec::new();
    if item.open && item.group == SUGGESTED_GROUP {
        badges.push(Badge::Open);
    }
    if best {
        badges.push(Badge::OftenPicked);
    }
    if let Some(text) = &item.badge {
        badges.push(Badge::Update(text.clone()));
    }
    badges
}

/// Draws one badge with its left edge at `at.x` and its top at `at.y`: 18
/// high, 1 px border, 12 px (the design's 11 px raised to the 12 px floor).
pub(crate) fn badge(ui: &egui::Ui, at: egui::Pos2, kind: &Badge, tokens: &Tokens) -> Rect {
    let painter = ui.painter();
    let (text, lead) = match kind {
        Badge::Open => ("open", 6.0),
        Badge::OftenPicked => ("often picked", 12.0),
        Badge::Update(text) => (text.as_str(), 6.0),
    };
    let ink = match kind {
        Badge::Update(_) => tokens.accent,
        _ => tokens.text_secondary,
    };
    let line = match kind {
        Badge::Update(_) => tokens.accent,
        _ => tokens.border,
    };
    let galley = painter.layout_no_wrap(text.to_string(), theme::regular(12.0), ink);
    let rect = Rect::from_min_size(at, vec2(8.0 + lead + 4.0 + galley.size().x + 8.0, 18.0));
    painter.rect(
        rect,
        9,
        Color32::TRANSPARENT,
        theme::line(ui.ctx(), line),
        StrokeKind::Inside,
    );
    let mark = pos2(rect.left() + 8.0 + lead / 2.0, rect.center().y);
    match kind {
        Badge::Open => {
            painter.circle_filled(mark, 3.0, tokens.success);
        }
        Badge::Update(_) => {
            painter.circle_filled(mark, 3.0, tokens.accent);
        }
        Badge::OftenPicked => icons::paint(
            painter,
            Rect::from_center_size(mark, vec2(12.0, 12.0)),
            Icon::Clock,
            tokens.text_secondary,
        ),
    }
    painter.galley(
        pos2(
            rect.left() + 8.0 + lead + 4.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    rect
}

/// How one row is drawn.
struct RowLook<'a> {
    needle: &'a str,
    selected: bool,
    interactive: bool,
    /// Windows are marked, so the checkbox column is there.
    column: bool,
    marked: bool,
    best: bool,
    /// The Enter pill's word on the selected row, in place of its keycaps.
    verb: Option<&'a str>,
}

struct RowResponse {
    clicked: bool,
    /// Shift was held for the click: it marks the window instead.
    shift: bool,
    rect: Rect,
}

/// A 44 px row: the checkbox column while windows are marked, the 26 px
/// icon slot, the title 15 (20 for an answer) with its badges and an
/// optional 12 px subtitle, and at the right the keycaps, or the Enter pill
/// on the selected row. A compact row is one 24 px line of monospace output.
/// The cursor row is drawn from state, never from hover; hover only adds the
/// 5 % tint. A command whose plugin is off stays listed at 45 % with an
/// italic "plugin off", so its hotkey does not seem to vanish.
fn result_row(
    ui: &mut egui::Ui,
    entry: &ResultItem,
    look: &RowLook,
    icons: &mut IconCache,
) -> RowResponse {
    let tokens = Tokens::get(ui.ctx());
    let sense = if look.interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let height = if entry.compact {
        COMPACT_HEIGHT
    } else {
        tokens.row_height.max(ROW_MIN_HEIGHT)
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    let shift = response.clicked() && ui.input(|input| input.modifiers.shift);
    let answer = RowResponse {
        clicked: response.clicked(),
        shift,
        rect,
    };
    if !ui.is_rect_visible(rect) {
        return answer;
    }
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
    if look.selected {
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
    let mut left = inner.left();
    if look.column && !entry.compact {
        if entry.window().is_some() {
            let check = Rect::from_min_size(
                pos2(left, rect.center().y - CHECK_SIZE / 2.0),
                vec2(CHECK_SIZE, CHECK_SIZE),
            );
            let edge = if look.marked {
                tokens.accent
            } else {
                tokens.border
            };
            painter.rect(
                check,
                3,
                Color32::TRANSPARENT,
                theme::line(ui.ctx(), edge),
                StrokeKind::Inside,
            );
            if look.marked {
                icons::paint(&painter, check.shrink(2.0), Icon::Check, tokens.accent);
            }
        }
        left += CHECK_SIZE + 10.0;
    }
    let (slot, text_left) = if entry.compact {
        let slot = Rect::from_min_size(pos2(left, rect.center().y - 8.0), vec2(16.0, 16.0));
        (slot, slot.right() + 12.0)
    } else {
        let slot = Rect::from_min_size(
            pos2(left, rect.center().y - ICON_SLOT / 2.0),
            vec2(ICON_SLOT, ICON_SLOT),
        );
        (slot, slot.right() + 12.0)
    };
    paint_icon(
        ui.ctx(),
        &painter,
        slot,
        entry,
        look.selected,
        icons,
        &tokens,
    );

    let mut text_right = inner.right();
    if let Some(verb) = look.verb.filter(|_| !entry.compact) {
        let pill = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            enter_pill(ui, verb)
        });
        text_right = pill.inner.left() - 12.0;
    } else if let Some(keys) = crate::core::shortcuts::parse(&entry.keys) {
        let chips = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            keycap::caps(ui, &keys.steps(), keycap::Caps::palette()).rect
        });
        text_right = chips.inner.left() - 12.0;
    } else if !entry.hint.is_empty() {
        let chips = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            keycap::binding(ui, &keycap::spaced(&entry.hint), false).rect
        });
        text_right = chips.inner.left() - 12.0;
    }

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
    let badges = if entry.compact {
        Vec::new()
    } else {
        row_badges(entry, look.best)
    };
    let badges_width: f32 = badges
        .iter()
        .map(|kind| match kind {
            Badge::Open => 52.0,
            Badge::OftenPicked => 104.0,
            Badge::Update(text) => {
                let width = painter
                    .layout_no_wrap(text.clone(), theme::regular(12.0), tokens.accent)
                    .size()
                    .x;
                8.0 + 6.0 + 4.0 + width + 8.0 + 6.0
            }
        })
        .sum();
    let off_width = off.as_ref().map_or(0.0, |galley| galley.size().x + 10.0);
    let title_width = text_right - text_left - off_width - badges_width;

    let colour = match entry.tone {
        Tone::Normal => tokens.text_primary,
        Tone::Warning => tokens.warning,
        Tone::Danger => tokens.danger,
    };
    let (font, line_height) = if entry.compact {
        (theme::mono(13.0), TITLE_LINE)
    } else if entry.answer {
        (theme::regular(20.0), ANSWER_LINE)
    } else {
        (theme::regular(15.0), TITLE_LINE)
    };
    let mut title = label_job(
        &entry.title,
        look.needle,
        font,
        colour,
        line_height,
        &tokens,
    );
    title.wrap = one_line(title_width);
    let title = painter.layout_job(title);
    let top = if entry.subtitle.is_empty() || entry.compact {
        rect.center().y - line_height / 2.0
    } else {
        rect.center().y - (line_height + SUBTITLE_LINE) / 2.0
    };
    let title_drawn = title.size().x;
    painter.galley(pos2(text_left, top), title, colour);
    let mut after = text_left + title_drawn + 10.0;
    if let Some(off) = off {
        let y = top + (line_height - off.size().y) / 2.0;
        let width = off.size().x;
        painter.galley(pos2(after, y), off, tokens.text_disabled);
        after += width + 10.0;
    }
    for kind in &badges {
        let at = pos2(after, top + (line_height - 18.0) / 2.0);
        after = badge(ui, at, kind, &tokens).right() + 6.0;
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
            pos2(text_left, top + line_height),
            subtitle,
            tokens.text_secondary,
        );
    }
    answer
}

/// The selected row's Enter pill: the verb in accent 12 px and a small ↵
/// keycap, in a 24 high outline of the accent colour.
fn enter_pill(ui: &mut egui::Ui, verb: &str) -> Rect {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let word = painter.layout_no_wrap(verb.to_string(), theme::regular(12.0), tokens.accent);
    let arrow = painter.layout_no_wrap("\u{21B5}".to_string(), theme::mono(12.0), tokens.accent);
    let cap_width = arrow.size().x + 8.0;
    let size = vec2(8.0 + word.size().x + 6.0 + cap_width + 4.0, 24.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        4,
        Color32::TRANSPARENT,
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    let word_width = word.size().x;
    painter.galley(
        pos2(rect.left() + 8.0, rect.center().y - word.size().y / 2.0),
        word,
        tokens.accent,
    );
    let cap = Rect::from_min_size(
        pos2(rect.left() + 8.0 + word_width + 6.0, rect.center().y - 8.0),
        vec2(cap_width, 16.0),
    );
    painter.rect(
        cap,
        3,
        Color32::TRANSPARENT,
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    painter.galley(cap.center() - arrow.size() / 2.0, arrow, tokens.accent);
    rect
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

/// The 26 px icon slot: a program's or file's own icon at 24 px, a Lucide
/// glyph at 18 px (accent on the selected row), or a monogram tile when the
/// row has no picture. A shell icon appears a frame or two after its row,
/// once the icon thread has read it; until then the slot stays empty rather
/// than flashing a stand-in.
fn paint_icon(
    ctx: &egui::Context,
    painter: &egui::Painter,
    slot: Rect,
    entry: &ResultItem,
    selected: bool,
    icons: &mut IconCache,
    tokens: &Tokens,
) {
    match &entry.icon {
        IconRef::None if entry.compact => {}
        IconRef::None => {
            painter.rect(
                slot,
                6,
                Color32::TRANSPARENT,
                theme::line(ctx, tokens.border),
                StrokeKind::Inside,
            );
            let letter: String = entry
                .title
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            painter.text(
                slot.center(),
                Align2::CENTER_CENTER,
                letter,
                theme::semibold(13.0),
                tokens.text_secondary,
            );
        }
        IconRef::Glyph(glyph) => {
            let drawn = match glyph {
                Glyph::Command => Icon::ChevronRight,
                Glyph::Search => Icon::Search,
                Glyph::Calculator => Icon::Calculator,
                Glyph::Globe => Icon::Globe,
                Glyph::Terminal => Icon::Terminal,
                Glyph::Sparkles => Icon::Sparkles,
            };
            if tokens.icon_tiles {
                let glyph_rect = Rect::from_center_size(slot.center(), vec2(18.0, 18.0));
                icons::paint_on_tile(
                    painter,
                    slot,
                    glyph_rect,
                    drawn,
                    f32::from(tokens.radius_row),
                );
            } else {
                let colour = if selected {
                    tokens.accent
                } else {
                    tokens.text_secondary
                };
                let size = if entry.compact { 16.0 } else { 18.0 };
                icons::paint(
                    painter,
                    Rect::from_center_size(slot.center(), vec2(size, size)),
                    drawn,
                    colour,
                );
            }
        }
        IconRef::Path(_) | IconRef::Window { .. } => {
            if let Some(texture) = icons.get(ctx, &entry.icon) {
                let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let size = if entry.compact { 16.0 } else { 24.0 };
                painter.image(
                    texture.id(),
                    Rect::from_center_size(slot.center(), vec2(size, size)),
                    whole,
                    egui::Color32::WHITE,
                );
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

/// Such as `↵ Switch  Shift+→ Next monitor  Esc Close`, 12 px with the keys
/// as 20 px chips. The arrows are drawn in the monospace family: they exist
/// only in egui's monospace fallback font.
fn footer_hints(ui: &mut egui::Ui, tokens: &Tokens, hints: &[Hint]) {
    for (index, hint) in hints.iter().enumerate() {
        if index > 0 {
            ui.add_space(16.0);
        }
        let font = if hint.key.is_ascii() {
            theme::regular(12.0)
        } else {
            theme::mono(12.0)
        };
        keycap::hint(ui, hint.key, font);
        ui.add_space(6.0);
        let colour = if hint.accent {
            tokens.accent
        } else {
            tokens.text_secondary
        };
        text::single(
            ui,
            text::job(&hint.label, theme::regular(12.0), colour, None),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_with_a_badge_text_carries_the_update_badge() {
        let plain = ResultItem::default();
        assert!(row_badges(&plain, false).is_empty());
        let item = ResultItem {
            badge: Some("0.20.20 available".to_string()),
            ..Default::default()
        };
        assert_eq!(
            row_badges(&item, false),
            [Badge::Update("0.20.20 available".to_string())]
        );
    }

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

    fn hint_keys(item: &ResultItem, tab_action: Option<Choice>) -> Vec<String> {
        let situation = Situation {
            tab_action,
            ..Default::default()
        };
        let actions = actions::row_actions(item, &situation);
        actions::fit_hints(actions::footer_hints(&actions, false, "Close"), 690.0)
            .iter()
            .map(|hint| hint.key.to_string())
            .collect()
    }

    fn browse() -> Option<Choice> {
        Some(Choice {
            label: "Browse".to_string(),
            action: Action::SetPrefix(String::new()),
        })
    }

    #[test]
    fn a_row_that_can_be_bookmarked_advertises_ctrl_b() {
        let keys = hint_keys(&row_with_keys("Files"), None);
        assert!(keys.contains(&"Ctrl+B".to_string()), "{keys:?}");
        assert_eq!(keys.last().map(String::as_str), Some("Esc"));
        // A row without the slot shows no such key.
        let plain = ResultItem {
            enter: Some(Choice {
                label: "Open".to_string(),
                action: Action::SetPrefix(String::new()),
            }),
            ..Default::default()
        };
        assert!(!hint_keys(&plain, None).contains(&"Ctrl+B".to_string()));
    }

    #[test]
    fn the_footer_leaves_out_the_least_needed_keys_instead_of_overflowing() {
        let mut folder = row_with_keys("Folders");
        folder.delete = folder.bookmark.clone().map(|mut choice| {
            choice.label = "Remove".to_string();
            choice
        });
        let situation = Situation {
            tab_action: browse(),
            ..Default::default()
        };
        let actions = actions::row_actions(&folder, &situation);
        let hints = actions::fit_hints(actions::footer_hints(&actions, false, "Close"), 400.0);
        let width: f32 = hints.iter().map(actions::hint_width).sum();
        assert!(width <= 400.0, "{width}");
        assert!(hints.len() <= actions::MOST_HINTS);
        assert_eq!(hints.first().map(|hint| hint.key), Some("\u{21B5}"));
        assert_eq!(hints.last().map(|hint| hint.key), Some("Esc"));
    }

    #[test]
    fn rows_with_their_own_keys_fill_the_footer_with_them() {
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
        assert_eq!(
            hint_keys(&item, browse()),
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
            MARGIN + PANEL_WIDTH - inspector::WIDTH - 8.0 - 8.0 - 12.0,
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

    /// Window rows that never touch a real window: the handles are made up
    /// and nothing in these tests reaches the host's window actions.
    struct Wins;

    impl crate::search::SearchProvider for Wins {
        fn id(&self) -> &'static str {
            "windows"
        }
        fn name(&self) -> &'static str {
            "Windows"
        }
        fn description(&self) -> &'static str {
            "test windows"
        }
        fn scope(&self) -> Option<&'static str> {
            Some("w")
        }
        fn blended(&self) -> bool {
            true
        }
        fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
            [(-7001, "Notes"), (-7002, "Inbox")]
                .into_iter()
                .map(|(hwnd, title)| ResultItem {
                    group: "Windows".to_string(),
                    title: title.to_string(),
                    icon: IconRef::Window {
                        hwnd,
                        exe: std::path::PathBuf::new(),
                    },
                    enter: Some(Choice {
                        label: "Switch to".to_string(),
                        action: Action::Activate(hwnd),
                    }),
                    ..Default::default()
                })
                .collect()
        }
    }

    fn windows_palette() -> Palette {
        let mut palette = Palette::new(Router::new(
            vec![(None, Box::new(Wins))],
            &Default::default(),
        ));
        palette.opened();
        palette
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }
    }

    #[test]
    fn a_scope_word_becomes_the_chip_and_backspace_on_an_empty_box_removes_it() {
        let mut palette = windows_palette();
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        let ctx = drawing_context();
        palette.type_in("w no");
        assert_eq!(palette.prefix.as_deref(), Some("w"));
        assert_eq!(palette.query, "no");
        palette.query.clear();
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::Backspace)],
        );
        assert_eq!(palette.prefix, None, "the chip is gone");
    }

    #[test]
    fn esc_clears_the_marks_before_it_closes_the_palette() {
        let mut palette = windows_palette();
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        let ctx = drawing_context();
        palette.type_in("w ");
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![ctrl_key(egui::Key::Space)],
        );
        assert_eq!(palette.marks.windows(), [-7001]);
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![ctrl_key(egui::Key::A)],
        );
        assert_eq!(
            palette.marks.windows(),
            [-7001, -7002],
            "Ctrl+A marks every window shown"
        );
        let outcome = frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::Escape)],
        );
        assert!(matches!(outcome, Outcome::Stay));
        assert!(palette.marks.is_empty());
        let outcome = frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::Escape)],
        );
        assert!(matches!(outcome, Outcome::Hide));
        assert!(bridge.host_rx.try_recv().is_err(), "no window was acted on");
    }

    #[test]
    fn nothing_matching_shows_the_try_instead_rows() {
        let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        palette.opened();
        palette.type_in("zqx");
        frame(
            &mut palette,
            &drawing_context(),
            &snapshot,
            &bridge,
            Vec::new(),
        );
        assert!(palette.trying);
        let titles: Vec<&str> = palette
            .results
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect();
        assert_eq!(
            titles,
            ["Ask AI about \u{201c}zqx\u{201d}", "Search the web"]
        );
    }

    #[test]
    fn ctrl_comma_and_ctrl_p_open_settings() {
        let mut palette = windows_palette();
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        let ctx = drawing_context();
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        let outcome = frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![ctrl_key(egui::Key::Comma)],
        );
        assert!(matches!(outcome, Outcome::OpenSettings(Page::General)));
        let outcome = frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![ctrl_key(egui::Key::P)],
        );
        assert!(matches!(outcome, Outcome::OpenSettings(Page::Plugins)));
    }

    #[test]
    fn the_right_arrow_walks_into_the_actions_and_enter_runs_the_one_there() {
        let folder = scratch_folder("actions").display().to_string();
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        let ctx = drawing_context();
        let mut palette = places_palette(&[&folder], &new_store());
        palette.query.clear();
        palette.searched = None;
        palette.type_in("x");
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::ArrowRight)],
        );
        assert_eq!(palette.action_cursor, Some(0));
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::ArrowDown)],
        );
        assert_eq!(palette.action_cursor, Some(1), "on Ctrl+B's bookmark");
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::ArrowUp)],
        );
        assert_eq!(palette.action_cursor, Some(0));
        let outcome = frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::Enter)],
        );
        assert!(matches!(outcome, Outcome::Hide));
        // The folder tip's state may go to the host first.
        assert!(std::iter::from_fn(|| bridge.host_rx.try_recv().ok())
            .any(|request| matches!(request, HostRequest::RunAction(Action::Open(_)))));
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn esc_in_the_actions_goes_back_to_the_list_first() {
        let mut palette = windows_palette();
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot::default();
        let ctx = drawing_context();
        palette.set_narrow(true);
        palette.type_in("w ");
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::ArrowRight)],
        );
        assert!(
            palette.overlay,
            "on a narrow monitor the inspector opens over the list"
        );
        let outcome = frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![key(egui::Key::Escape)],
        );
        assert!(matches!(outcome, Outcome::Stay));
        assert!(!palette.overlay);
        assert_eq!(palette.action_cursor, None);
    }

    #[test]
    fn a_narrow_palette_is_the_list_alone() {
        let mut palette = windows_palette();
        assert_eq!(palette.panel_width(), PANEL_WIDTH);
        palette.set_narrow(true);
        assert_eq!(palette.panel_width(), PANEL_WIDTH - inspector::WIDTH);
        assert_eq!(
            palette.window_width(),
            PANEL_WIDTH - inspector::WIDTH + 2.0 * MARGIN
        );
    }

    #[test]
    fn with_the_glass_the_window_is_the_panel() {
        let mut palette = windows_palette();
        assert_eq!(palette.window_height(), HEIGHT);
        palette.set_glass(true);
        assert!(palette.glass());
        assert_eq!(palette.margin(), 0.0);
        assert_eq!(palette.window_width(), PANEL_WIDTH);
        assert_eq!(palette.window_height(), PANEL_HEIGHT);
        let bridge = crate::core::ui_bridge::create();
        let ctx = drawing_context();
        frame(
            &mut palette,
            &ctx,
            &UiSnapshot::default(),
            &bridge,
            Vec::new(),
        );
        palette.set_glass(false);
        assert_eq!(palette.window_width(), WIDTH);
    }

    #[test]
    fn the_enter_pill_word_comes_from_the_first_action() {
        let mut palette = windows_palette();
        palette.type_in("w ");
        let bridge = crate::core::ui_bridge::create();
        frame(
            &mut palette,
            &drawing_context(),
            &UiSnapshot::default(),
            &bridge,
            Vec::new(),
        );
        let actions = palette.current_actions();
        assert_eq!(actions::enter_verb(&actions[0].label), "Switch");
    }
}
