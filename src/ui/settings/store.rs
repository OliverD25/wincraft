use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use crate::core::ui_bridge::{HostChannel, HostRequest, UiSnapshot};
use crate::store::{github, index};
use crate::ui::settings::SettingsState;

pub enum StoreMessage {
    Index(Result<(index::PluginIndex, String), String>),
    Readme(String, Result<(String, String), String>),
}

pub struct Store {
    pub index: Option<index::PluginIndex>,
    pub status: String,
    pub open_plugin: Option<String>,
    pub readme: Option<String>,
    pub readme_status: String,
    loading: bool,
    tx: Sender<StoreMessage>,
    rx: Receiver<StoreMessage>,
}

impl Default for Store {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            index: None,
            status: "Not loaded yet.".to_string(),
            open_plugin: None,
            readme: None,
            readme_status: String::new(),
            loading: false,
            tx,
            rx,
        }
    }
}

impl Store {
    /// The fetch runs on a throwaway thread and wakes the UI when it lands, so
    /// a slow or dead network never freezes the window.
    pub fn refresh(&mut self, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.status = "Loading\u{2026}".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = index::fetch_index().map(|fetched| (fetched.value, fetched.note));
            let _ = tx.send(StoreMessage::Index(result));
            ctx.request_repaint_of(super::viewport_id());
        });
    }

    fn load_readme(&mut self, ctx: &egui::Context, id: String, path: String) {
        self.readme = None;
        self.readme_status = "Loading\u{2026}".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = index::fetch_readme(&path).map(|fetched| (fetched.value, fetched.note));
            let _ = tx.send(StoreMessage::Readme(id, result));
            ctx.request_repaint_of(super::viewport_id());
        });
    }

    fn drain(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                StoreMessage::Index(Ok((index, note))) => {
                    self.status = note;
                    self.index = Some(index);
                    self.loading = false;
                }
                StoreMessage::Index(Err(err)) => {
                    self.status = err;
                    self.loading = false;
                }
                StoreMessage::Readme(id, Ok((text, note))) => {
                    if self.open_plugin.as_deref() == Some(id.as_str()) {
                        self.readme = Some(text);
                        self.readme_status = note;
                    }
                }
                StoreMessage::Readme(id, Err(err)) => {
                    if self.open_plugin.as_deref() == Some(id.as_str()) {
                        self.readme_status = err;
                    }
                }
            }
        }
    }
}

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    state.store.drain();
    let ctx = ui.ctx().clone();
    if state.store.index.is_none() && state.store.status == "Not loaded yet." {
        state.store.refresh(&ctx);
    }

    ui.horizontal(|ui| {
        ui.heading("Plugin store");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                state.store.refresh(&ctx);
            }
        });
    });
    ui.label(egui::RichText::new(&state.store.status).weak().small());
    ui.add_space(12.0);

    if let Some(id) = state.store.open_plugin.clone() {
        detail(ui, &id, snapshot, state, to_host);
        return;
    }

    let Some(catalogue) = state.store.index.clone() else {
        return;
    };
    for entry in &catalogue.plugins {
        let installed = snapshot.plugins.iter().find(|plugin| plugin.id == entry.id);
        let frame = egui::Frame::NONE
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui.link(egui::RichText::new(&entry.name).strong()).clicked() {
                            state.store.open_plugin = Some(entry.id.clone());
                            let path = entry.readme.clone();
                            let id = entry.id.clone();
                            state.store.load_readme(&ctx, id, path);
                        }
                        ui.label(
                            egui::RichText::new(format!("{} \u{00B7} {}", entry.version, entry.author))
                                .weak()
                                .small(),
                        );
                    });
                    ui.label(egui::RichText::new(&entry.description).small());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    badge(ui, entry, installed, &catalogue.wincraft_version, to_host);
                });
            });
        });
        ui.add_space(8.0);
    }
}

fn badge(
    ui: &mut egui::Ui,
    entry: &index::IndexEntry,
    installed: Option<&crate::core::ui_bridge::PluginInfo>,
    needs_version: &str,
    to_host: &Arc<HostChannel>,
) {
    match installed {
        Some(plugin) if plugin.enabled => {
            ui.label(egui::RichText::new("Enabled").small());
        }
        Some(plugin) => {
            if ui.button("Enable").clicked() {
                to_host.send(HostRequest::SetModuleEnabled {
                    id: plugin.id.clone(),
                    enabled: true,
                });
            }
            ui.label(egui::RichText::new("Installed, disabled").small().weak());
        }
        None => {
            if ui.button("Get update").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(github::releases_url()));
            }
            ui.label(
                egui::RichText::new(format!("Needs WinCraft {needs_version}"))
                    .small()
                    .weak(),
            );
            let _ = entry;
        }
    }
}

fn detail(
    ui: &mut egui::Ui,
    id: &str,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
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

    ui.horizontal(|ui| {
        if ui.button("\u{2190} All plugins").clicked() {
            state.store.open_plugin = None;
            state.store.readme = None;
        }
        if ui.button("Open on GitHub").clicked() {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(github::readme_url(&entry.readme)));
        }
    });
    ui.add_space(8.0);
    ui.heading(&entry.name);
    ui.label(
        egui::RichText::new(format!("{} \u{00B7} {}", entry.version, entry.author))
            .weak()
            .small(),
    );
    ui.horizontal(|ui| {
        let installed = snapshot.plugins.iter().find(|plugin| plugin.id == entry.id);
        badge(ui, &entry, installed, "", to_host);
    });
    ui.add_space(12.0);

    if !state.store.readme_status.is_empty() {
        ui.label(
            egui::RichText::new(&state.store.readme_status)
                .weak()
                .small(),
        );
    }
    let readme = state.store.readme.clone();
    if let Some(text) = readme {
        state.readme.show(ui, &text);
    }
}
