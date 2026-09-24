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

use crate::core::ui_bridge::{CommandId, PaletteEntry, PluginInfo};

/// What the user typed, with the prefix already taken off.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub text: String,
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
    /// Brings a window forward, restoring it if it is minimized.
    Activate(isize),
}

/// A picture the palette draws itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Glyph {
    Command,
}

/// Where a row's 16 px picture comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum IconRef {
    #[default]
    None,
    Glyph(Glyph),
    /// The shell's icon for this file, folder or shortcut.
    Path(PathBuf),
    /// The window's own icon, or its program's when it has none.
    Window {
        hwnd: isize,
        exe: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    /// The footer's word for it: "Run", "Open", "Copy".
    pub label: String,
    pub action: Action,
}

/// One row of the palette.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// What Enter does.
    pub enter: Option<Choice>,
}

/// What every provider may read besides the query.
pub struct Context<'a> {
    pub commands: &'a [PaletteEntry],
    pub plugins: &'a [PluginInfo],
}

pub trait SearchProvider: Send {
    /// Short lowercase id, also the key in config.json's `search.prefixes`.
    fn id(&self) -> &'static str;

    /// Called each time the palette opens, for a provider that keeps a
    /// snapshot of something that changes, like the open windows.
    fn opened(&mut self) {}

    /// Answers one query. It runs on the UI thread on every keystroke, so it
    /// must answer from memory or from one cheap system call.
    fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem>;
}

struct Slot {
    provider: Box<dyn SearchProvider>,
    /// The plugin that added this provider; it answers only while that
    /// plugin is on.
    plugin: Option<String>,
}

/// The providers a query can reach.
pub struct Router {
    slots: Vec<Slot>,
}

/// What the palette draws for one query.
#[derive(Default)]
pub struct Results {
    pub items: Vec<ResultItem>,
    /// The part of the query whose letters are underlined in the titles.
    pub needle: String,
}

const BLENDED_LIMIT: usize = 8;

impl Router {
    /// `providers` pairs each provider with the plugin that added it, or None
    /// for a built-in one.
    pub fn new(providers: Vec<(Option<String>, Box<dyn SearchProvider>)>) -> Self {
        let slots = providers
            .into_iter()
            .map(|(plugin, provider)| Slot { provider, plugin })
            .collect();
        Self { slots }
    }

    pub fn opened(&mut self) {
        for slot in &mut self.slots {
            slot.provider.opened();
        }
    }

    pub fn search(&mut self, text: &str, context: &Context) -> Results {
        let text = text.trim();
        let query = Query {
            text: text.to_string(),
            prefix: None,
            limit: BLENDED_LIMIT,
        };
        let mut items = Vec::new();
        for slot in &mut self.slots {
            if !plugin_is_on(slot.plugin.as_deref(), context.plugins) {
                continue;
            }
            items.extend(timed(slot.provider.as_mut(), &query, context));
        }
        Results {
            items: rank(items, text.is_empty()),
            needle: text.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item(group: &str, title: &str, score: i32) -> ResultItem {
        ResultItem {
            group: group.to_string(),
            title: title.to_string(),
            subtitle: String::new(),
            icon: IconRef::None,
            hint: String::new(),
            score,
            disabled: false,
            enter: None,
        }
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|item| item.title.as_str()).collect()
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
            action: Action::Open(shortcut),
        });
        let text = serde_json::to_string(&original).unwrap();
        let back: ResultItem = serde_json::from_str(&text).unwrap();
        assert_eq!(back, original);
    }
}
