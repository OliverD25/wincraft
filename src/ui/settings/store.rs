use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use crate::core::clock;
use crate::core::theme::Tokens;
use crate::core::ui_bridge::{HostChannel, HostRequest, PluginInfo, UiSnapshot};
use crate::store::github;
use crate::store::index::{self, IndexEntry, PluginIndex, Source};
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::badge::badge;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::text;

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
        text::secondary(ui, &message, text_colour);
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

fn store_badge(
    ui: &mut egui::Ui,
    installed: Option<&PluginInfo>,
    needs_version: &str,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    match installed {
        Some(plugin) if plugin.enabled => {
            badge(ui, "Enabled", tokens.success, tokens.success);
        }
        Some(plugin) => {
            badge(
                ui,
                "Installed, disabled",
                tokens.text_disabled,
                tokens.text_secondary,
            );
            if button::button(ui, "Enable", Kind::Primary).clicked() {
                to_host.send(HostRequest::SetPluginEnabled {
                    id: plugin.id.clone(),
                    enabled: true,
                });
            }
        }
        None => {
            let label = format!("Needs WinCraft {needs_version}");
            badge(ui, &label, tokens.warning, tokens.warning);
            if button::button(ui, "Get update", Kind::Secondary).clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(github::releases_url()));
            }
        }
    }
}

fn list(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let mut refresh = false;
    page::page_header(
        ui,
        "Plugin Store",
        None,
        "Plugins from the public index. Installing writes to the plugins folder only.",
        |ui| {
            refresh =
                button::with_icon(ui, "Refresh", Kind::Secondary, Some(Icon::Refresh)).clicked();
        },
    );
    if refresh {
        state.store.refresh(&ctx);
    }
    ui.add_space(16.0);
    ui.spacing_mut().item_spacing.y = 0.0;

    let Some(catalogue) = state.store.index.clone() else {
        if let Some(Status::Failed(err)) = &state.store.status {
            empty_state(ui, "The plugin index could not be loaded", err);
        }
        return;
    };
    for entry in &catalogue.plugins {
        let installed = snapshot.plugins.iter().find(|plugin| plugin.id == entry.id);
        let meta = format!("{} \u{00B7} {}", entry.version, entry.author);
        let text = RowText::new(&entry.name)
            .meta(&meta)
            .desc(&entry.description, tokens.text_secondary);
        let row = settings_row(ui, &entry.id, text, true, |ui| {
            store_badge(ui, installed, &catalogue.wincraft_version, to_host);
            ui.add_space(4.0);
            icons::show(ui, Icon::ChevronRight, 14.0, tokens.text_disabled);
        });
        if row.clicked() {
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
    ui.horizontal(|ui| {
        let installed = snapshot.plugins.iter().find(|plugin| plugin.id == entry.id);
        let needs = state
            .store
            .index
            .as_ref()
            .map(|catalogue| catalogue.wincraft_version.clone())
            .unwrap_or_default();
        store_badge(ui, installed, &needs, to_host);
    });

    ui.add_space(20.0);
    if !state.store.readme_note.is_empty() {
        text::secondary(ui, &state.store.readme_note, tokens.text_disabled);
    }
    if let Some(readme) = state.store.readme.clone() {
        state.readme.boxed(ui, &readme);
    }
}
