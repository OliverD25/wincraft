use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use crate::core::clock;
use crate::core::theme::Tokens;
use crate::core::ui_bridge::{HostChannel, HostRequest, UiSnapshot};
use crate::store::github;
use crate::store::index::{self, IndexEntry, PluginIndex, Source};
use crate::ui::settings::catalog::{self, Card, Sort, StoreAction, StoreState, Tone, View};
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::badge;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::icons::Icon;
use crate::ui::widgets::text;
use crate::ui::widgets::tile::Part;

/// What the status line at the bottom of the page reports.
#[derive(Clone, Debug)]
enum Status {
    Loading,
    Loaded { count: usize, at: String },
    Cached { date: String },
    BuiltIn { count: usize },
    Failed(String),
}

enum Message {
    Index(Result<(PluginIndex, Source), String>),
    Readme(String, Result<String, String>),
}

pub struct Store {
    /// The search, category and sort the tile grid is showing.
    view: View,
    index: Option<PluginIndex>,
    status: Option<Status>,
    open_plugin: Option<String>,
    readme: Option<String>,
    readme_note: String,
    tx: Sender<Message>,
    rx: Receiver<Message>,
}

impl Default for Store {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            view: View::sorted_by(Sort::Name),
            index: None,
            status: None,
            open_plugin: None,
            readme: None,
            readme_note: String::new(),
            tx,
            rx,
        }
    }
}

impl Store {
    /// The fetch runs on a throwaway thread and wakes the settings window when
    /// it lands, so a slow or dead network never freezes the page.
    fn refresh(&mut self, ctx: &egui::Context) {
        if matches!(self.status, Some(Status::Loading)) {
            return;
        }
        self.status = Some(Status::Loading);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = index::fetch_index().map(|fetched| (fetched.value, fetched.source));
            let _ = tx.send(Message::Index(result));
            ctx.request_repaint_of(super::viewport_id());
        });
    }

    fn load_readme(&mut self, ctx: &egui::Context, id: String, path: String) {
        self.readme = None;
        self.readme_note = "Loading\u{2026}".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = index::fetch_readme(&path).map(|fetched| fetched.value);
            let _ = tx.send(Message::Readme(id, result));
            ctx.request_repaint_of(super::viewport_id());
        });
    }

    fn drain(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Index(Ok((catalogue, source))) => {
                    let count = catalogue.plugins.len();
                    self.status = Some(match source {
                        Source::Live => Status::Loaded {
                            count,
                            at: clock::now_hours_minutes(),
                        },
                        Source::Cache(written) => Status::Cached {
                            date: clock::day_month(written),
                        },
                        Source::BuiltIn => Status::BuiltIn { count },
                    });
                    self.index = Some(catalogue);
                }
                Message::Index(Err(err)) => self.status = Some(Status::Failed(err)),
                Message::Readme(id, result) => {
                    if self.open_plugin.as_deref() == Some(id.as_str()) {
                        match result {
                            Ok(text) => {
                                self.readme = Some(text);
                                self.readme_note.clear();
                            }
                            Err(err) => self.readme_note = err,
                        }
                    }
                }
            }
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        "plugin"
    } else {
        "plugins"
    }
}

/// Status line pinned to the bottom of the page: a 6 px dot, then the text.
fn status_line(ui: &mut egui::Ui, status: &Status) {
    let tokens = Tokens::get(ui.ctx());
    let (colour, filled, message, text_colour) = match status {
        Status::Loading => (
            tokens.text_disabled,
            false,
            "Loading\u{2026}".to_string(),
            tokens.text_disabled,
        ),
        Status::Loaded { count, at } => (
            tokens.success,
            true,
            format!("{count} {} listed \u{00B7} updated {at}", plural(*count)),
            tokens.text_secondary,
        ),
        Status::Cached { date } => (
            tokens.warning,
            true,
            format!("Offline, showing the cached copy from {date}"),
            tokens.text_secondary,
        ),
        Status::BuiltIn { count } => (
            tokens.warning,
            true,
            format!(
                "Offline, showing the {count} {} built into this copy",
                plural(*count)
            ),
            tokens.text_secondary,
        ),
        Status::Failed(err) => (
            tokens.warning,
            true,
            format!("Could not load the index \u{2014} {err}"),
            tokens.text_secondary,
        ),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        text::status_dot(ui, colour, filled);
        text::caption(ui, &message, text_colour);
    });
}

const STATUS_HEIGHT: f32 = 12.0 + 17.0;

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let ctx = ui.ctx().clone();
    state.store.drain();
    if state.store.status.is_none() {
        state.store.refresh(&ctx);
    }

    let list_height = (ui.available_height() - STATUS_HEIGHT).max(0.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(list_height)
        .show(ui, |ui| match state.store.open_plugin.clone() {
            Some(id) => detail(ui, &id, snapshot, state, to_host),
            None => list(ui, snapshot, state, to_host),
        });

    ui.add_space(12.0);
    if let Some(status) = state.store.status.clone() {
        status_line(ui, &status);
    }
}

/// The button a store tile or the detail page offers for one plugin.
fn state_button(
    ui: &mut egui::Ui,
    state: &StoreState,
    plugin_id: &str,
    to_host: &Arc<HostChannel>,
    small: bool,
) {
    if let Some(spec) = state.button() {
        let clicked = if small {
            button::small(ui, spec.label, spec.kind)
        } else {
            button::button(ui, spec.label, spec.kind)
        }
        .clicked();
        if clicked {
            match spec.action {
                StoreAction::Enable => to_host.send(HostRequest::SetPluginEnabled {
                    id: plugin_id.to_string(),
                    enabled: true,
                }),
                StoreAction::OpenReleases => ui
                    .ctx()
                    .open_url(egui::OpenUrl::new_tab(github::releases_url())),
            }
        }
    }
}

fn state_badge(ui: &mut egui::Ui, state: &StoreState) {
    let tokens = Tokens::get(ui.ctx());
    if let Some(spec) = state.badge() {
        let (stroke, text_colour) = match spec.tone {
            Tone::Good => (tokens.success, tokens.success),
            Tone::Quiet => (tokens.text_disabled, tokens.text_secondary),
            Tone::Warn => (tokens.warning, tokens.warning),
        };
        badge::tall(ui, &spec.label, stroke, text_colour);
    }
}

fn state_of(entry_id: &str, snapshot: &UiSnapshot, catalogue: &PluginIndex) -> StoreState {
    let installed = snapshot
        .plugins
        .iter()
        .find(|plugin| plugin.id == entry_id)
        .map(|plugin| plugin.enabled);
    StoreState::of(installed, &catalogue.wincraft_version, &snapshot.version)
}

fn list(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let ctx = ui.ctx().clone();
    let subtitle = match &state.store.index {
        Some(catalogue) if !catalogue.plugins.is_empty() => format!(
            "{} {} from the community index.",
            catalog::count_in_words(catalogue.plugins.len()),
            plural(catalogue.plugins.len())
        ),
        _ => "Plugins from the community index.".to_string(),
    };
    let mut refresh = false;
    catalog::page_title(ui, "Plugin Store", &subtitle, |ui| {
        refresh = button::with_icon(ui, "Refresh", Kind::Secondary, Some(Icon::Refresh)).clicked();
    });
    if refresh {
        state.store.refresh(&ctx);
    }
    ui.add_space(20.0);

    let Some(catalogue) = state.store.index.clone() else {
        if let Some(Status::Failed(err)) = &state.store.status {
            empty_state(ui, "The plugin index could not be loaded", err);
        }
        return;
    };
    let cards: Vec<Card> = catalogue
        .plugins
        .iter()
        .map(|entry| {
            let installed = snapshot.plugins.iter().find(|plugin| plugin.id == entry.id);
            Card::from_entry(entry, installed)
        })
        .collect();
    catalog::toolbar(ui, &mut state.store.view, &cards, "Search the store", true);
    let order = catalog::visible(&cards, &state.store.view);
    if order.is_empty() {
        empty_state(ui, "No plugin matches", "Try other words, or choose All.");
        return;
    }

    let picked = catalog::tiles(ui, &cards, &order, false, |ui, card, part| {
        if part == Part::Footer {
            let tile_state = state_of(&card.id, snapshot, &catalogue);
            // Right to left: the button ends up on the right of the badge.
            state_button(ui, &tile_state, &card.id, to_host, true);
            state_badge(ui, &tile_state);
        }
    });
    if let Some(picked) = picked {
        // Space opens a tile here as Enter does; there is no switch to flip.
        if let Some(entry) = catalogue.plugins.get(picked.card) {
            open(state, &ctx, entry);
        }
    }
}

fn open(state: &mut SettingsState, ctx: &egui::Context, entry: &IndexEntry) {
    state.store.open_plugin = Some(entry.id.clone());
    state
        .store
        .load_readme(ctx, entry.id.clone(), entry.readme.clone());
}

fn detail(
    ui: &mut egui::Ui,
    id: &str,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let entry = state
        .store
        .index
        .as_ref()
        .and_then(|catalogue| catalogue.plugins.iter().find(|entry| entry.id == id))
        .cloned();
    let Some(entry) = entry else {
        state.store.open_plugin = None;
        return;
    };

    if text::link(ui, "\u{2039} All plugins").clicked() {
        state.store.open_plugin = None;
        state.store.readme = None;
        return;
    }
    ui.add_space(8.0);

    let meta = format!("{} \u{00B7} {}", entry.version, entry.author);
    let mut open_github = false;
    page::page_header(ui, &entry.name, Some(&meta), &entry.description, |ui| {
        open_github = button::with_icon(
            ui,
            "Open on GitHub",
            Kind::Primary,
            Some(Icon::ExternalLink),
        )
        .clicked();
    });
    if open_github {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab(github::readme_url(&entry.readme)));
    }

    ui.add_space(12.0);
    let tile_state = state
        .store
        .index
        .as_ref()
        .map(|catalogue| state_of(&entry.id, snapshot, catalogue));
    if let Some(tile_state) = tile_state {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            state_badge(ui, &tile_state);
            state_button(ui, &tile_state, &entry.id, to_host, false);
        });
    }

    ui.add_space(20.0);
    if !state.store.readme_note.is_empty() {
        text::secondary(ui, &state.store.readme_note, tokens.text_disabled);
    }
    if let Some(readme) = state.store.readme.clone() {
        state.readme.boxed(ui, &readme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeChoice;
    use crate::core::theme;
    use crate::core::ui_bridge::PluginInfo;

    fn entry(id: &str, category: &str) -> IndexEntry {
        IndexEntry {
            id: id.to_string(),
            name: id.to_uppercase(),
            version: "1.0.0".to_string(),
            author: "community".to_string(),
            description: "Does one useful thing.".to_string(),
            readme: format!("src/plugins/{id}/README.md"),
            category: category.to_string(),
            thumbnail: None,
        }
    }

    fn installed(id: &str, enabled: bool) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            name: id.to_uppercase(),
            version: "1.0.0".to_string(),
            author: "community".to_string(),
            description: String::new(),
            readme: String::new(),
            category: String::new(),
            enabled,
            config_path: Default::default(),
            hotkeys: Vec::new(),
            fields: Vec::new(),
            status: None,
            page_action: None,
        }
    }

    #[test]
    fn the_store_draws_a_tile_for_every_state_and_sends_nothing_by_itself() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(
            &ctx,
            crate::core::config::ThemeName::Classic,
            ThemeChoice::Dark,
        );
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot {
            version: "0.20.2".to_string(),
            plugins: vec![installed("on", true), installed("off", false)],
            ..UiSnapshot::default()
        };
        let mut state = SettingsState::default();
        state.store.index = Some(PluginIndex {
            wincraft_version: "0.21.0".to_string(),
            plugins: vec![
                entry("on", "Windows"),
                entry("off", "Input"),
                entry("new", "Keyboard"),
            ],
        });
        state.store.status = Some(Status::Loaded {
            count: 3,
            at: "09:30".to_string(),
        });
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                list(ui, &snapshot, &mut state, &bridge.to_host);
            });
            output.textures_delta.clear();
        }
        assert!(bridge.host_rx.try_recv().is_err());
        assert_eq!(state.store.open_plugin, None);
    }

    #[test]
    fn a_plugin_the_index_lists_but_this_build_lacks_needs_a_newer_wincraft() {
        let snapshot = UiSnapshot {
            version: "0.20.2".to_string(),
            plugins: vec![installed("on", true)],
            ..UiSnapshot::default()
        };
        let index = PluginIndex {
            wincraft_version: "0.21.0".to_string(),
            plugins: Vec::new(),
        };
        assert_eq!(state_of("on", &snapshot, &index), StoreState::Enabled);
        assert_eq!(
            state_of("new", &snapshot, &index),
            StoreState::NeedsUpdate("0.21.0".to_string())
        );
    }
}
