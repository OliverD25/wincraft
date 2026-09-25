//! The palette's search engine: providers answer a query, the router picks
//! which providers a query goes to and merges what they return.
//!
//! The request and result types derive serde so that a provider living in
//! another process could speak the same shapes as JSON later without the
//! palette changing.

pub mod actions;
pub mod icons;
pub mod providers;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use windows_sys::core::GUID;
use windows_sys::Win32::System::Com::{CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

use crate::core::com::ComPtr;
use crate::core::config::SearchConfig;
use crate::core::ui_bridge::{CommandId, PaletteEntry, PluginInfo};
use crate::ui::fuzzy;

thread_local! {
    static COM_STARTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A COM object for the search, which runs on the UI thread. winit usually
/// has COM running there already; if not, this starts it. It is never shut
/// down: the thread lives as long as WinCraft.
pub(crate) fn create_com(clsid: &GUID, iid: &GUID) -> Option<ComPtr> {
    COM_STARTED.with(|done| {
        if !done.replace(true) {
            unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        }
    });
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
    /// A one-line monospace row, 24 px high, for command output.
    #[serde(default)]
    pub compact: bool,
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
}

/// The providers a query can reach, and the prefix of each.
pub struct Router {
    slots: Vec<Slot>,
    /// See `Context::folder`.
    folder: Option<String>,
}

/// What the palette draws for one query.
#[derive(Default)]
pub struct Results {
    pub items: Vec<ResultItem>,
    pub needle: String,
    pub follows_end: bool,
    pub escape_label: Option<&'static str>,
}

const BLENDED_LIMIT: usize = 8;
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
            slots.push(Slot {
                provider,
                plugin,
                prefix,
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
        }
    }

    /// Splits a known prefix off the start of what was typed, preferring the
    /// longest when one prefix begins another.
    pub fn split<'q>(&self, raw: &'q str) -> Option<(String, &'q str)> {
        self.slots
            .iter()
            .filter_map(|slot| slot.prefix.as_deref())
            .chain(std::iter::once(HELP_PREFIX))
            .filter(|prefix| raw.starts_with(prefix))
            .max_by_key(|prefix| prefix.len())
            .map(|prefix| (prefix.to_string(), &raw[prefix.len()..]))
    }

    fn slot_for(&self, prefix: &str) -> Option<&Slot> {
        self.slots
            .iter()
            .find(|slot| slot.prefix.as_deref() == Some(prefix))
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
        self.slots
            .iter_mut()
            .find(|slot| slot.prefix.as_deref() == Some(prefix))
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

    pub fn available(&mut self, provider: &str, command: &str) -> bool {
        self.slots
            .iter_mut()
            .find(|slot| slot.provider.id() == provider)
            .is_some_and(|slot| slot.provider.available(command))
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
        let index = self
            .slots
            .iter()
            .position(|slot| slot.prefix.as_deref() == Some(prefix));
        let index = match index {
            Some(index) if !(help && text.is_empty()) => index,
            _ => return self.help(text, context),
        };
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
        Results {
            items: rank(items, needle.is_empty() || follows_end),
            needle,
            follows_end,
            escape_label,
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
        Results {
            items: rank(items, text.is_empty()),
            needle: text.to_string(),
            ..Default::default()
        }
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
            let prefix = slot.prefix.as_deref()?;
            if !plugin_is_on(slot.plugin.as_deref(), context.plugins) {
                return None;
            }
            Some(row(
                slot.provider.name(),
                &slot.provider.describe(context),
                prefix,
            ))
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

fn timed(provider: &mut dyn SearchProvider, query: &Query, context: &Context) -> Vec<ResultItem> {
    let started = Instant::now();
    let found = provider.query(query, context);
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
mod tests {
    use super::*;

    fn item(group: &str, title: &str, score: i32) -> ResultItem {
        ResultItem {
            group: group.to_string(),
            title: title.to_string(),
            score,
            ..Default::default()
        }
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|item| item.title.as_str()).collect()
    }

    /// Answers every query with one row naming itself and the text it got.
    struct Echo {
        id: &'static str,
        prefix: Option<&'static str>,
        blended: bool,
    }

    impl SearchProvider for Echo {
        fn id(&self) -> &'static str {
            self.id
        }
        fn name(&self) -> &'static str {
            self.id
        }
        fn description(&self) -> &'static str {
            "echo"
        }
        fn default_prefix(&self) -> Option<&'static str> {
            self.prefix
        }
        fn blended(&self) -> bool {
            self.blended
        }
        fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
            vec![item(self.id, &format!("{}:{}", self.id, query.text), 1)]
        }
    }

    fn echo(
        id: &'static str,
        prefix: Option<&'static str>,
        blended: bool,
    ) -> Box<dyn SearchProvider> {
        Box::new(Echo {
            id,
            prefix,
            blended,
        })
    }

    fn router(prefixes: &[(&str, &str)]) -> Router {
        let prefixes = prefixes
            .iter()
            .map(|(id, prefix)| (id.to_string(), prefix.to_string()))
            .collect();
        Router::new(
            vec![
                (None, echo("commands", None, true)),
                (None, echo("windows", Some("<"), true)),
                (None, echo("paths", Some("/"), false)),
                (None, echo("web", Some("?"), false)),
                (
                    Some("bookmarks".to_string()),
                    echo("marks", Some("*"), false),
                ),
            ],
            &prefixes,
        )
    }

    fn context() -> Context<'static> {
        test_context()
    }

    #[test]
    fn a_prefix_is_split_off_the_front() {
        let router = router(&[]);
        assert_eq!(router.split("<chrome"), Some(("<".to_string(), "chrome")));
        assert_eq!(router.split("/"), Some(("/".to_string(), "")));
        assert_eq!(router.split("chrome"), None);
        assert_eq!(router.split(" <chrome"), None);
    }

    #[test]
    fn the_longest_prefix_wins() {
        let router = router(&[("paths", "//")]);
        assert_eq!(router.split("//x"), Some(("//".to_string(), "x")));
    }

    #[test]
    fn config_can_move_or_switch_off_a_prefix() {
        let router = router(&[("windows", "w:"), ("paths", "")]);
        assert_eq!(router.split("w:x"), Some(("w:".to_string(), "x")));
        assert_eq!(router.split("<x"), None);
        assert_eq!(router.split("/x"), None);
    }

    #[test]
    fn a_taken_prefix_is_not_given_twice() {
        let router = router(&[("paths", "<")]);
        let mut router = router;
        let found = router.search(Some("<"), "x", &context());
        assert_eq!(titles(&found.items), ["windows:x"]);
    }

    #[test]
    fn no_prefix_asks_only_the_blended_providers() {
        let mut router = router(&[]);
        let found = router.search(None, "x", &context());
        let mut got = titles(&found.items);
        got.sort();
        assert_eq!(got, ["commands:x", "windows:x"]);
    }

    #[test]
    fn a_prefix_asks_only_its_provider() {
        let mut router = router(&[]);
        let found = router.search(Some("/"), "c:\\", &context());
        assert_eq!(titles(&found.items), ["paths:c:\\"]);
    }

    #[test]
    fn the_question_mark_alone_lists_the_prefixes_and_with_text_searches_the_web() {
        let mut router = router(&[]);
        let help = router.search(Some("?"), "", &context());
        assert_eq!(
            titles(&help.items),
            ["Commands, apps and windows", "windows", "paths", "web"]
        );
        assert_eq!(help.items[1].hint, "<");
        assert_eq!(
            help.items[1].enter.as_ref().map(|choice| &choice.action),
            Some(&Action::SetPrefix("<".to_string()))
        );
        let web = router.search(Some("?"), "rust egui", &context());
        assert_eq!(titles(&web.items), ["web:rust egui"]);
    }

    #[test]
    fn the_question_mark_still_lists_prefixes_when_nothing_owns_it() {
        let mut router = router(&[("web", "")]);
        assert_eq!(router.split("?pa"), Some(("?".to_string(), "pa")));
        let found = router.search(Some("?"), "pa", &context());
        assert_eq!(titles(&found.items).first(), Some(&"paths"));
    }

    #[test]
    fn a_plugin_provider_answers_only_while_its_plugin_is_on() {
        let mut router = router(&[]);
        let found = router.search(Some("*"), "x", &context());
        assert!(found.items.is_empty());
    }

    #[test]
    fn nothing_typed_keeps_the_providers_order() {
        let items = vec![item("B", "b1", 0), item("A", "a1", 0), item("B", "b2", 0)];
        assert_eq!(titles(&rank(items, true)), ["b1", "a1", "b2"]);
    }

    #[test]
    fn groups_follow_their_best_match_and_rows_their_own_score() {
        let items = vec![
            item("Apps", "low app", 5),
            item("WinCraft", "good command", 20),
            item("Apps", "best app", 30),
            item("WinCraft", "weak command", 1),
        ];
        assert_eq!(
            titles(&rank(items, false)),
            ["best app", "low app", "good command", "weak command"]
        );
    }

    #[test]
    fn an_exact_title_goes_first_whatever_its_group() {
        let items = vec![
            item(
                "Apps",
                "Notepad++",
                fuzzy::score("notepad", "Notepad++").unwrap(),
            ),
            item(
                "Windows",
                "Notepad",
                fuzzy::score("notepad", "Notepad").unwrap(),
            ),
        ];
        assert_eq!(titles(&rank(items, false)), ["Notepad", "Notepad++"]);
    }

    #[test]
    fn equal_scores_keep_the_order_the_provider_gave() {
        let items = vec![item("Files", "z", 3), item("Files", "a", 3)];
        assert_eq!(titles(&rank(items, false)), ["z", "a"]);
    }

    #[test]
    fn a_result_survives_a_trip_through_json() {
        let shortcut = PathBuf::from(r"C:\Start Menu\Notepad.lnk");
        let mut original = item("Apps", "Notepad", 12);
        original.icon = IconRef::Path(shortcut.clone());
        original.enter = Some(Choice {
            label: "Open".to_string(),
            action: Action::Open(shortcut.clone()),
        });
        original.shift_enter = Some(Choice {
            label: "Show in folder".to_string(),
            action: Action::Reveal(shortcut),
        });
        let text = serde_json::to_string(&original).unwrap();
        let back: ResultItem = serde_json::from_str(&text).unwrap();
        assert_eq!(back, original);
    }
}
