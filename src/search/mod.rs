//! The palette's search engine: providers answer a query, the router picks
//! which providers a query goes to and merges what they return.
//!
//! The request and result types derive serde so that a provider living in
//! another process could speak the same shapes as JSON later without the
//! palette changing.

pub mod actions;
pub mod ask_ai;
pub mod bookmarks;
pub mod icons;
pub mod providers;
pub mod usage;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::{CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

use crate::core::clock;
use crate::core::com::ComPtr;
use crate::core::config::SearchConfig;
use crate::core::ui_bridge::{CommandId, PaletteEntry, PluginInfo};
use crate::ui::fuzzy;
use usage::Usage;

thread_local! {
    static COM_STARTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The search runs on the UI thread. winit usually has COM running there
/// already; if not, this starts it. It is never shut down: the thread lives
/// as long as WinCraft.
pub(crate) fn start_com() {
    COM_STARTED.with(|done| {
        if !done.replace(true) {
            unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        }
    });
}

/// A COM object for the search, with COM started first.
pub(crate) fn create_com(clsid: &GUID, iid: &GUID) -> Option<ComPtr> {
    start_com();
    ComPtr::create(clsid, iid, CLSCTX_ALL).ok()
}

/// Typed alone, lists every prefix. A provider may still own it for queries
/// with text after it, as web search does by default.
pub const HELP_PREFIX: &str = "?";

/// What the user typed, with the prefix already taken off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub text: String,
    /// The prefix the user typed, or None when the query is blended.
    #[serde(default)]
    pub prefix: Option<String>,
    /// The most rows the palette will show from one provider. A provider may
    /// return fewer; the commands provider returns all of its few matches.
    pub limit: usize,
}

/// What choosing a row does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Action {
    /// A WinCraft command. Only the host knows what the id means, so it never
    /// crosses a process boundary.
    #[serde(skip)]
    Command(CommandId),
    /// Opens a plugin's settings page, for a command whose plugin is off.
    OpenPlugin(String),
    /// Opens a file, folder or shortcut the way Explorer would.
    Open(PathBuf),
    /// Opens the folder that holds this file, with the file selected.
    Reveal(PathBuf),
    /// Brings a window forward, restoring it if it is minimized.
    Activate(isize),
    Copy(String),
    /// Opens an http or https address in the default browser.
    OpenUrl(String),
    /// Switches the palette to another prefix; empty goes back to none.
    SetPrefix(String),
    /// Handed back to the provider that made the row, on the UI thread, for
    /// work only it can do, such as starting a command it then watches.
    Provider {
        provider: String,
        command: String,
    },
    /// Opens the hotkey dialog on this command.
    ChangeShortcut {
        id: String,
        label: String,
    },
    /// Puts the text on the clipboard and pastes it into the window that
    /// was in front before the palette opened.
    Paste(String),
    Minimize(Vec<isize>),
    /// Asks the windows to close, as their own close button does, so an
    /// app can still ask to save first.
    Close(Vec<isize>),
    /// Moves the windows to the next monitor to the right, or round to the
    /// first, keeping their place on the screen.
    NextMonitor(Vec<isize>),
    /// Shows a window on every virtual desktop, or on its own one again.
    AllDesktops {
        hwnd: isize,
        on: bool,
    },
    /// Opens the Arrange strip with this window picked.
    Arrange(isize),
}

/// What the palette does after a provider handled an action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Stay,
    /// Stays open with the text after the prefix emptied.
    ClearQuery,
    /// Switches to another provider's prefix with this text after it, as
    /// Tab on an Explorer window does to browse its folder under `/`.
    Switch {
        provider: &'static str,
        text: String,
    },
    Hide,
    /// Turns the palette into a one-line box that asks this.
    Ask(Ask),
}

/// A question a provider puts to the person, which the palette shows as a
/// one-line box. The answer comes back to the provider's `answered`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ask {
    /// The provider that gets the answer.
    pub provider: &'static str,
    /// The chip before the box, such as "Bookmark"; also the Enter key's label.
    pub chip: String,
    pub placeholder: String,
    /// Put in the box and selected, so typing replaces it.
    pub text: String,
    /// The row under the box says what Enter does with the text typed, such
    /// as "Save bookmark as" …
    pub action: String,
    /// … and its second line.
    pub subtitle: String,
    /// What the provider needs to know when the answer arrives.
    pub payload: String,
}

/// The title's colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Normal,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    /// The footer's word for it: "Run", "Open", "Copy".
    pub label: String,
    pub action: Action,
}

/// What Tab does: replaces the text after the prefix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    pub label: String,
    pub text: String,
}

impl Completion {
    /// Puts a row's title in the box without a footer hint, the everyday
    /// Tab that needs no explaining.
    pub fn quiet(text: &str) -> Self {
        Self {
            label: String::new(),
            text: text.to_string(),
        }
    }
}

/// A picture the palette draws itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Glyph {
    Command,
    Search,
    Calculator,
    Globe,
    Terminal,
    Sparkles,
}

/// Where a row's 16 px picture comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum IconRef {
    #[default]
    None,
    Glyph(Glyph),
    /// The shell's icon for this file, folder, drive or shortcut.
    Path(PathBuf),
    /// The window's own icon, or its program's when it has none.
    Window {
        hwnd: isize,
        exe: PathBuf,
    },
}

/// One row of the palette. The action slots are the keys that act on it:
/// Enter, Shift+Enter, Ctrl+Enter and Tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResultItem {
    /// Rows are listed under one header per group, as the palette always did
    /// with plugin names.
    pub group: String,
    pub title: String,
    /// The smaller second line: a path, a window's program, a monitor's state.
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub icon: IconRef,
    /// Keys shown as keycaps at the right, such as "Win+Alt+F1".
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub score: i32,
    /// Drawn dimmed with "plugin off" after the title.
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub enter: Option<Choice>,
    #[serde(default)]
    pub shift_enter: Option<Choice>,
    #[serde(default)]
    pub ctrl_enter: Option<Choice>,
    /// None means Tab does nothing. A completion with an empty label works
    /// but is not advertised in the footer.
    #[serde(default)]
    pub tab: Option<Completion>,
    /// Tab as an action instead of a completion, used when the provider can
    /// say `available` for it; otherwise `tab` applies.
    #[serde(default)]
    pub tab_action: Option<Choice>,
    #[serde(default)]
    pub tone: Tone,
    /// Ctrl+B: bookmark what this row opens.
    #[serde(default)]
    pub bookmark: Option<Choice>,
    /// Ctrl+Delete: forget this row, such as a bookmark.
    #[serde(default)]
    pub delete: Option<Choice>,
    /// Ctrl+K: change this command's shortcut.
    #[serde(default)]
    pub shortcut: Option<Choice>,
    /// The command's shortcut, drawn as keycaps at the right in place of
    /// `hint`; empty for none.
    #[serde(default)]
    pub keys: String,
    /// A one-line monospace row, 24 px high, for command output.
    #[serde(default)]
    pub compact: bool,
    /// Set by a provider whose rows should rank higher once picked: a key
    /// that stays the same for the same thing, such as an app's shortcut
    /// path. The router puts the provider's id in front of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_key: Option<String>,
    /// An answer worked out from what was typed, like the calculator's: it
    /// leads the list and its title is drawn large.
    #[serde(default)]
    pub answer: bool,
    /// Ctrl+C while nothing is selected in the box, such as copying a sum
    /// with its answer.
    #[serde(default)]
    pub copy: Option<Choice>,
    /// An open window, marked "open" where other rows sit beside it.
    #[serde(default)]
    pub open: bool,
    /// A short text drawn as a pill after the title, such as "0.20.20
    /// available".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// The plugin a command belongs to, for the inspector's source line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// The id of the provider that made the row; the router fills it in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
}

impl ResultItem {
    /// The window this row stands for, when it is an open window.
    pub fn window(&self) -> Option<isize> {
        match (&self.icon, self.enter.as_ref().map(|choice| &choice.action)) {
            (IconRef::Window { hwnd, .. }, Some(Action::Activate(target))) if hwnd == target => {
                Some(*hwnd)
            }
            _ => None,
        }
    }
}

/// What every provider may read besides the query.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub commands: &'a [PaletteEntry],
    pub plugins: &'a [PluginInfo],
    pub search: &'a SearchConfig,
    /// The folder the user last browsed with a provider during this opening
    /// of the palette, such as a folder drilled into under `/`.
    pub folder: Option<&'a str>,
}

pub trait SearchProvider: Send {
    /// Short lowercase id, also the key in config.json's `search.prefixes`.
    fn id(&self) -> &'static str;

    /// Shown in the prefix chip and in the `?` list.
    fn name(&self) -> &'static str;

    /// One line for the `?` list.
    fn description(&self) -> &'static str;

    /// The `?` list's line when it depends on the settings, such as the
    /// shell the terminal provider runs commands in.
    fn describe(&self, context: &Context) -> String {
        let _ = context;
        self.description().to_string()
    }

    /// The prefix that sends a query here when config.json names none.
    fn default_prefix(&self) -> Option<&'static str> {
        None
    }

    /// A short word that also sends a query here when a space follows it,
    /// such as "w" for windows: "w chrome".
    fn scope(&self) -> Option<&'static str> {
        None
    }

    /// Rows for "Recent and suggested" under an empty box, such as the open
    /// windows. The router keeps the few picked most.
    fn suggestions(&mut self, context: &Context) -> Vec<ResultItem> {
        let _ = context;
        Vec::new()
    }

    /// Whether this provider also answers queries typed without a prefix.
    fn blended(&self) -> bool {
        false
    }

    /// The empty search box's grey text while this provider's prefix is on.
    fn placeholder(&self) -> &'static str {
        "Type to search\u{2026}"
    }

    /// The part of the text whose letters are underlined in the titles.
    fn needle<'a>(&self, text: &'a str) -> &'a str {
        text
    }

    /// Called each time the palette opens, for a provider that keeps a
    /// snapshot of something that changes, like the open windows.
    fn opened(&mut self) {}

    /// Something is still being read on another thread; while this is true
    /// the palette wakes up now and then to call `has_news`.
    fn waiting(&self) -> bool {
        false
    }

    /// Something read on another thread arrived since the last call, so the
    /// palette should ask this provider again.
    fn has_news(&mut self) -> bool {
        false
    }

    /// Answers one query. It runs on the UI thread on every keystroke, so it
    /// must answer from memory or from one cheap system call.
    fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem>;

    /// Runs an `Action::Provider` this provider put on a row.
    fn act(&mut self, command: &str, context: &Context) -> Reply {
        let _ = (command, context);
        Reply::Stay
    }

    /// The answer to a question this provider asked with `Reply::Ask`. Returns
    /// another question when it needs more, such as a second Enter to confirm.
    fn answered(&mut self, payload: &str, text: &str, context: &Context) -> Option<Ask> {
        let _ = (payload, text, context);
        None
    }

    /// Whether an `Action::Provider` on the selected row can do anything,
    /// asked only for that row so a slow check runs once, not per keystroke.
    fn available(&mut self, command: &str) -> bool {
        let _ = command;
        true
    }

    /// The folder the last answer was browsing, shared with other providers
    /// through `Context::folder`.
    fn folder(&self) -> Option<String> {
        None
    }

    /// Esc was pressed while this provider's prefix is on. Returning true
    /// keeps the palette open, as when Esc stops a running command.
    fn escape(&mut self) -> bool {
        false
    }

    /// The footer's word for Esc when it does not close the palette.
    fn escape_label(&self) -> Option<&'static str> {
        None
    }

    /// A key other than Enter was pressed, which cancels a pending "press
    /// Enter again".
    fn interrupted(&mut self) {}

    /// Everything Ctrl+Shift+C copies, such as a command's whole output.
    fn copy_all(&self) -> Option<String> {
        None
    }

    /// New rows arrive at the bottom, so the palette keeps the last one in
    /// view as long as the cursor is already there.
    fn follows_end(&self) -> bool {
        false
    }
}

struct Slot {
    provider: Box<dyn SearchProvider>,
    /// The plugin that added this provider; it answers only while that
    /// plugin is on.
    plugin: Option<String>,
    prefix: Option<String>,
    scope: Option<String>,
}

impl Slot {
    /// Whether `prefix`, as split off the box, sends queries here.
    fn answers(&self, prefix: &str) -> bool {
        self.prefix.as_deref() == Some(prefix) || self.scope.as_deref() == Some(prefix)
    }
}

/// The providers a query can reach, and the prefix of each.
pub struct Router {
    slots: Vec<Slot>,
    /// See `Context::folder`.
    folder: Option<String>,
    usage: Usage,
}

/// What the palette draws for one query.
#[derive(Default)]
pub struct Results {
    pub items: Vec<ResultItem>,
    pub needle: String,
    pub follows_end: bool,
    pub escape_label: Option<&'static str>,
    /// The first row is the one picked most after what is typed now.
    pub best_match: bool,
}

/// What the inspector says about the best match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickInfo {
    pub this_month: u32,
    /// Unix seconds.
    pub last: i64,
}

const BLENDED_LIMIT: usize = 8;
/// How many rows "Recent and suggested" shows.
const SUGGESTED: usize = 5;
pub const SUGGESTED_GROUP: &str = "Recent and suggested";
const PREFIXED_LIMIT: usize = 500;

impl Router {
    /// `providers` pairs each provider with the plugin that added it, or None
    /// for a built-in one. `prefixes` is config.json's `search.prefixes`: a
    /// provider missing from it keeps its default prefix, an empty string
    /// switches its prefix off.
    pub fn new(
        providers: Vec<(Option<String>, Box<dyn SearchProvider>)>,
        prefixes: &BTreeMap<String, String>,
    ) -> Self {
        let mut slots: Vec<Slot> = Vec::new();
        for (plugin, provider) in providers {
            let wanted = match prefixes.get(provider.id()) {
                Some(text) => Some(text.trim()).filter(|text| !text.is_empty()),
                None => provider.default_prefix(),
            };
            let prefix = wanted.and_then(|prefix| {
                if slots
                    .iter()
                    .any(|slot| slot.prefix.as_deref() == Some(prefix))
                {
                    log::warn!(
                        "search prefix {prefix:?} is taken; {} gets none",
                        provider.id()
                    );
                    None
                } else {
                    Some(prefix.to_string())
                }
            });
            let scope = provider.scope().and_then(|scope| {
                let taken = slots.iter().any(|slot| {
                    slot.scope.as_deref() == Some(scope) || slot.prefix.as_deref() == Some(scope)
                });
                if taken {
                    log::warn!(
                        "search scope {scope:?} is taken; {} gets none",
                        provider.id()
                    );
                }
                (!taken).then(|| scope.to_string())
            });
            slots.push(Slot {
                provider,
                plugin,
                prefix,
                scope,
            });
        }
        for id in prefixes.keys() {
            if !slots.iter().any(|slot| slot.provider.id() == id) {
                log::warn!("search.prefixes names {id:?}, which is not a search provider");
            }
        }
        Self {
            slots,
            folder: None,
            usage: Usage::default(),
        }
    }

    /// Reads the picks saved in earlier sessions. Without it the router
    /// ranks from this session's picks only and saves nothing.
    pub fn load_usage(&mut self, path: PathBuf) {
        self.usage = Usage::load(path);
        self.usage.rename_keys(
            providers::commands::WINDOW_DECK_SPLIT,
            &providers::commands::WINDOW_DECK_RENAMES,
        );
        log::info!(
            "palette usage: {} rows picked before, {} typed texts remembered",
            self.usage.len(),
            self.usage.typed_len()
        );
    }

    /// A row was run with Enter after `text` was typed, behind `prefix`;
    /// nothing is recorded while ranking by use is off.
    pub fn record_pick(
        &mut self,
        key: &str,
        prefix: Option<&str>,
        text: &str,
        search: &SearchConfig,
    ) {
        if !search.rank_by_use {
            return;
        }
        let now = usage::now();
        self.usage.record(key, now);
        if let Some(typed) = typed_text(prefix, text) {
            self.usage
                .record_typed(&typed, key, now, clock::month_key(now));
        }
        self.usage.save_soon();
    }

    /// How often `key` was picked after this text, for the inspector.
    pub fn pick_info(&self, key: &str, prefix: Option<&str>, text: &str) -> Option<PickInfo> {
        let typed = typed_text(prefix, text)?;
        let now = usage::now();
        let (this_month, last) = self.usage.picked(&typed, key, clock::month_key(now))?;
        Some(PickInfo { this_month, last })
    }

    /// "Forget this pick": the row is no longer the best match for this
    /// text, or for the shorter texts it was remembered under.
    pub fn forget_pick(&mut self, key: &str, prefix: Option<&str>, text: &str) {
        if let Some(typed) = typed_text(prefix, text) {
            self.usage.forget_typed(&typed, key);
            self.usage.save_soon();
            log::info!("palette: forgot a pick for what was typed");
        }
    }

    /// Forgets every pick, in memory and in the file, at once.
    pub fn clear_usage(&mut self) {
        self.usage.clear();
        self.usage.flush();
        log::info!("palette usage cleared");
    }

    /// Saves recorded picks, at most once every 2 s.
    pub fn save_usage_soon(&mut self) {
        self.usage.save_soon();
    }

    pub fn flush_usage(&mut self) {
        self.usage.flush();
    }

    /// Splits a known prefix off the start of what was typed, preferring the
    /// longest when one prefix begins another.
    /// A scope word counts only with the space after it, which is taken
    /// off too: "w chrome" is the windows scope and "chrome".
    pub fn split<'q>(&self, raw: &'q str) -> Option<(String, &'q str)> {
        let scoped = self
            .slots
            .iter()
            .filter_map(|slot| slot.scope.as_deref())
            .find(|scope| {
                raw.strip_prefix(scope)
                    .is_some_and(|rest| rest.starts_with(' '))
            })
            .map(|scope| (scope.to_string(), &raw[scope.len() + 1..]));
        scoped.or_else(|| {
            self.slots
                .iter()
                .filter_map(|slot| slot.prefix.as_deref())
                .chain(std::iter::once(HELP_PREFIX))
                .filter(|prefix| raw.starts_with(prefix))
                .max_by_key(|prefix| prefix.len())
                .map(|prefix| (prefix.to_string(), &raw[prefix.len()..]))
        })
    }

    fn slot_for(&self, prefix: &str) -> Option<&Slot> {
        self.slots.iter().find(|slot| slot.answers(prefix))
    }

    /// The prefix of the provider Ask AI belongs to, for the search bar's
    /// pill.
    pub fn ask_prefix(&self) -> Option<String> {
        self.prefix_of("web")
    }

    /// The name in the chip next to a prefix, and the grey text for the empty
    /// search box after it. `?` with nothing after it is the prefix list,
    /// even when a provider owns `?` for the text that follows.
    pub fn chip(&self, prefix: &str, text: &str) -> (&'static str, &'static str) {
        match self.slot_for(prefix) {
            Some(slot) if !(prefix == HELP_PREFIX && text.trim().is_empty()) => {
                (slot.provider.name(), slot.provider.placeholder())
            }
            Some(slot) => ("Prefixes", slot.provider.placeholder()),
            None => ("Prefixes", "Filter the prefixes\u{2026}"),
        }
    }

    pub fn opened(&mut self) {
        self.folder = None;
        for slot in &mut self.slots {
            slot.provider.opened();
        }
    }

    fn slot_mut(&mut self, prefix: Option<&str>) -> Option<&mut Slot> {
        let prefix = prefix?;
        self.slots.iter_mut().find(|slot| slot.answers(prefix))
    }

    pub fn act(&mut self, provider: &str, command: &str, context: &Context) -> Reply {
        let context = Context {
            folder: self.folder.as_deref(),
            ..*context
        };
        match self
            .slots
            .iter_mut()
            .find(|slot| slot.provider.id() == provider)
        {
            Some(slot) => slot.provider.act(command, &context),
            None => Reply::Stay,
        }
    }

    /// Hands an answer to the provider that asked.
    pub fn answered(
        &mut self,
        provider: &str,
        payload: &str,
        text: &str,
        context: &Context,
    ) -> Option<Ask> {
        let context = Context {
            folder: self.folder.as_deref(),
            ..*context
        };
        self.slots
            .iter_mut()
            .find(|slot| slot.provider.id() == provider)
            .and_then(|slot| slot.provider.answered(payload, text, &context))
    }

    pub fn available(&mut self, provider: &str, command: &str) -> bool {
        self.slots
            .iter_mut()
            .find(|slot| slot.provider.id() == provider)
            .is_some_and(|slot| slot.provider.available(command))
    }

    /// The name a provider shows in its chip, for the inspector's source
    /// line.
    pub fn provider_name(&self, provider: &str) -> Option<&'static str> {
        self.slots
            .iter()
            .find(|slot| slot.provider.id() == provider)
            .map(|slot| slot.provider.name())
    }

    /// The prefix a provider answers to, if it has one.
    pub fn prefix_of(&self, provider: &str) -> Option<String> {
        self.slots
            .iter()
            .find(|slot| slot.provider.id() == provider)
            .and_then(|slot| slot.prefix.clone())
    }

    pub fn escape(&mut self, prefix: Option<&str>) -> bool {
        self.slot_mut(prefix)
            .is_some_and(|slot| slot.provider.escape())
    }

    pub fn copy_all(&mut self, prefix: Option<&str>) -> Option<String> {
        self.slot_mut(prefix)
            .and_then(|slot| slot.provider.copy_all())
    }

    pub fn interrupted(&mut self) {
        for slot in &mut self.slots {
            slot.provider.interrupted();
        }
    }

    /// Whether any provider got background results since the last poll, and
    /// whether any is still waiting for some.
    pub fn poll(&mut self) -> (bool, bool) {
        let mut news = false;
        let mut waiting = false;
        for slot in &mut self.slots {
            news |= slot.provider.has_news();
            waiting |= slot.provider.waiting();
        }
        (news, waiting)
    }

    pub fn search(&mut self, prefix: Option<&str>, text: &str, context: &Context) -> Results {
        let text = text.trim_start();
        let Some(prefix) = prefix else {
            return self.blended(text.trim_end(), context);
        };
        let help = prefix == HELP_PREFIX;
        let index = self.slots.iter().position(|slot| slot.answers(prefix));
        let index = match index {
            Some(index) if !(help && text.is_empty()) => index,
            _ => return self.help(text, context),
        };
        let settings = context.search;
        let context = Context {
            folder: self.folder.as_deref(),
            ..*context
        };
        let slot = &mut self.slots[index];
        if !plugin_is_on(slot.plugin.as_deref(), context.plugins) {
            return Results::default();
        }
        let query = Query {
            text: text.to_string(),
            prefix: Some(prefix.to_string()),
            limit: PREFIXED_LIMIT,
        };
        let needle = slot.provider.needle(text).trim().to_string();
        let items = timed(slot.provider.as_mut(), &query, &context);
        let follows_end = slot.provider.follows_end();
        let escape_label = slot.provider.escape_label();
        if let Some(folder) = slot.provider.folder() {
            self.folder = Some(folder);
        }
        let keep_order = needle.is_empty() || follows_end;
        let mut items = self.rank_by_use(items, keep_order, settings);
        let mut best_match = false;
        if settings.rank_by_use && !follows_end {
            (items, best_match) =
                self.lift_remembered(items, typed_text(Some(prefix), text).as_deref());
        }
        Results {
            items: answers_first(items),
            needle,
            follows_end,
            escape_label,
            best_match,
        }
    }

    /// `rank`, with the usage boost added first when something is typed, or
    /// the picked rows moved up within their group when nothing is.
    fn rank_by_use(
        &self,
        mut items: Vec<ResultItem>,
        keep_order: bool,
        search: &SearchConfig,
    ) -> Vec<ResultItem> {
        if !search.rank_by_use {
            return rank(items, keep_order);
        }
        let now = usage::now();
        if keep_order {
            self.usage.order_rows(&mut items, now);
            items
        } else {
            self.usage.boost_rows(&mut items, now);
            rank(items, false)
        }
    }

    fn blended(&mut self, text: &str, context: &Context) -> Results {
        let query = Query {
            text: text.to_string(),
            prefix: None,
            limit: BLENDED_LIMIT,
        };
        let mut items = Vec::new();
        for slot in &mut self.slots {
            if !slot.provider.blended() || !plugin_is_on(slot.plugin.as_deref(), context.plugins) {
                continue;
            }
            items.extend(timed(slot.provider.as_mut(), &query, context));
        }
        let items = self.rank_by_use(items, text.is_empty(), context.search);
        let mut items = lift_commands(items, text);
        let mut best_match = false;
        if context.search.rank_by_use {
            (items, best_match) = self.lift_remembered(items, typed_text(None, text).as_deref());
        }
        if text.is_empty() {
            let mut suggested = self.suggested(context);
            suggested.extend(items);
            items = suggested;
        }
        let items = answers_first(items);
        Results {
            best_match: best_match && !items.first().is_some_and(|item| item.answer),
            items,
            needle: text.to_string(),
            ..Default::default()
        }
    }

    /// "Recent and suggested": the rows the providers suggest, the most
    /// picked first, up to five. An app is suggested only once it was
    /// picked; a window always may be, in the order Alt+Tab shows them.
    fn suggested(&mut self, context: &Context) -> Vec<ResultItem> {
        let now = usage::now();
        let mut rows: Vec<(f64, ResultItem)> = Vec::new();
        for slot in &mut self.slots {
            if !slot.provider.blended() || !plugin_is_on(slot.plugin.as_deref(), context.plugins) {
                continue;
            }
            let id = slot.provider.id();
            for mut item in slot.provider.suggestions(context) {
                if let Some(key) = item.usage_key.take() {
                    item.usage_key = Some(format!("{id}:{key}"));
                }
                item.provider = id.to_string();
                let score = match (&item.usage_key, context.search.rank_by_use) {
                    (Some(key), true) => self.usage.score(key, now),
                    _ => 0.0,
                };
                if score > 0.0 || item.open {
                    item.group = SUGGESTED_GROUP.to_string();
                    rows.push((score, item));
                }
            }
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        rows.into_iter()
            .take(SUGGESTED)
            .map(|(_, item)| item)
            .collect()
    }

    /// The row picked before after this text, if it is among the results,
    /// goes first with the rest of its group. True when one did.
    fn lift_remembered(
        &self,
        items: Vec<ResultItem>,
        typed: Option<&str>,
    ) -> (Vec<ResultItem>, bool) {
        let Some(typed) = typed else {
            return (items, false);
        };
        for key in self.usage.remembered(typed, usage::now()) {
            if let Some(index) = items
                .iter()
                .position(|item| item.usage_key.as_deref() == Some(key.as_str()))
            {
                return (put_first(items, index), true);
            }
        }
        (items, false)
    }

    /// One row per prefix, and one for going back to none. Choosing a row
    /// switches the palette to it.
    fn help(&self, filter: &str, context: &Context) -> Results {
        let filter = filter.trim();
        let row = |name: &str, description: &str, prefix: &str| ResultItem {
            group: "Prefixes".to_string(),
            title: name.to_string(),
            subtitle: description.to_string(),
            icon: IconRef::Glyph(Glyph::Search),
            hint: prefix.to_string(),
            enter: Some(Choice {
                label: "Use".to_string(),
                action: Action::SetPrefix(prefix.to_string()),
            }),
            ..Default::default()
        };
        let mut rows = vec![row(
            "Commands, apps and windows",
            "No prefix: WinCraft commands, Start menu apps and open windows together",
            "",
        )];
        rows.extend(self.slots.iter().filter_map(|slot| {
            let prefix = slot.prefix.as_deref().or(slot.scope.as_deref())?;
            if !plugin_is_on(slot.plugin.as_deref(), context.plugins) {
                return None;
            }
            let mut item = row(
                slot.provider.name(),
                &slot.provider.describe(context),
                prefix,
            );
            if let (Some(_), Some(scope)) = (&slot.prefix, &slot.scope) {
                item.subtitle = format!("{} Or type {scope} and a space.", item.subtitle);
            }
            Some(item)
        }));
        let items = rows
            .into_iter()
            .filter_map(|mut item| {
                if filter.is_empty() {
                    return Some(item);
                }
                item.score = fuzzy::score(filter, &item.title)?;
                Some(item)
            })
            .collect();
        Results {
            items: rank(items, filter.is_empty()),
            needle: filter.to_string(),
            ..Default::default()
        }
    }
}

/// What the memory of typed texts files a pick under: the prefix and the
/// text together. None while nothing but a prefix is typed.
fn typed_text(prefix: Option<&str>, text: &str) -> Option<String> {
    let text = usage::normal_text(text);
    if text.is_empty() {
        return None;
    }
    Some(format!("{}{text}", prefix.unwrap_or_default()))
}

/// Answers worked out from the text, like the calculator's, lead the list.
fn answers_first(items: Vec<ResultItem>) -> Vec<ResultItem> {
    if !items.iter().any(|item| item.answer) {
        return items;
    }
    let (mut first, rest): (Vec<_>, Vec<_>) = items.into_iter().partition(|item| item.answer);
    first.extend(rest);
    first
}

/// Moves the row at `index` to the top and its group right after it, then
/// the other groups as they were, so the group header stays in one piece.
fn put_first(items: Vec<ResultItem>, index: usize) -> Vec<ResultItem> {
    let group = items[index].group.clone();
    let mut first = Vec::with_capacity(items.len());
    let mut rest = Vec::new();
    for (position, item) in items.into_iter().enumerate() {
        if position == index {
            first.insert(0, item);
        } else if item.group == group {
            first.push(item);
        } else {
            rest.push(item);
        }
    }
    first.extend(rest);
    first
}

/// Whether `text` starts a word of `label`: "upd" and "wincr" start words of
/// "Update WinCraft", "ate" does not.
fn starts_a_word(label: &str, text: &str) -> bool {
    let label: Vec<char> = label.chars().flat_map(char::to_lowercase).collect();
    let text: Vec<char> = text.trim().chars().flat_map(char::to_lowercase).collect();
    if text.is_empty() || text.len() > label.len() {
        return false;
    }
    (0..=label.len() - text.len()).any(|start| {
        (start == 0 || !label[start - 1].is_alphanumeric()) && label[start..].starts_with(&text)
    })
}

/// With text typed and no prefix: when a WinCraft command has the text at the
/// start of a word, the groups holding such commands come before the others,
/// each group still in the order it had.
fn lift_commands(items: Vec<ResultItem>, text: &str) -> Vec<ResultItem> {
    if text.trim().is_empty() {
        return items;
    }
    let mut lifted: Vec<String> = Vec::new();
    for item in &items {
        let is_command = item
            .usage_key
            .as_deref()
            .is_some_and(|key| key.starts_with(COMMANDS_KEY));
        if is_command && starts_a_word(&item.title, text) && !lifted.contains(&item.group) {
            lifted.push(item.group.clone());
        }
    }
    if lifted.is_empty() {
        return items;
    }
    let (mut first, rest): (Vec<_>, Vec<_>) = items
        .into_iter()
        .partition(|item| lifted.contains(&item.group));
    first.extend(rest);
    first
}

/// Where the router puts the id of the commands provider in a usage key.
const COMMANDS_KEY: &str = "commands:";

fn timed(provider: &mut dyn SearchProvider, query: &Query, context: &Context) -> Vec<ResultItem> {
    let started = Instant::now();
    let mut found = provider.query(query, context);
    let id = provider.id();
    for item in &mut found {
        if let Some(key) = item.usage_key.take() {
            item.usage_key = Some(format!("{id}:{key}"));
        }
        item.provider = id.to_string();
    }
    log::debug!(
        "{} answered {:?} with {} rows in {:.2} ms",
        provider.id(),
        query.text,
        found.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    found
}

fn plugin_is_on(plugin: Option<&str>, plugins: &[PluginInfo]) -> bool {
    match plugin {
        None => true,
        Some(id) => plugins.iter().any(|info| info.id == id && info.enabled),
    }
}

/// With nothing typed the providers' own order stands, because the host
/// already groups its commands. Otherwise the best matches come first, still
/// grouped, with the groups in the order of their best match.
pub fn rank(items: Vec<ResultItem>, keep_order: bool) -> Vec<ResultItem> {
    if keep_order {
        return items;
    }
    let mut best: BTreeMap<&str, i32> = BTreeMap::new();
    for item in &items {
        let entry = best.entry(item.group.as_str()).or_insert(i32::MIN);
        *entry = (*entry).max(item.score);
    }
    let mut order: Vec<(i32, &str, i32, usize)> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let group = item.group.as_str();
            (best[group], group, item.score, index)
        })
        .collect();
    order.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(b.1))
            .then(b.2.cmp(&a.2))
            .then(a.3.cmp(&b.3))
    });
    let positions: Vec<usize> = order.into_iter().map(|(_, _, _, index)| index).collect();
    let mut taken: Vec<Option<ResultItem>> = items.into_iter().map(Some).collect();
    positions
        .into_iter()
        .filter_map(|index| taken[index].take())
        .collect()
}

/// An empty context with default settings, for providers' tests.
#[cfg(test)]
pub fn test_context() -> Context<'static> {
    Context {
        commands: &[],
        plugins: &[],
        search: Box::leak(Box::new(SearchConfig::default())),
        folder: None,
    }
}

#[cfg(test)]
mod tests;
