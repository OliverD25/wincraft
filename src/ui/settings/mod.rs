mod about;
mod general;
mod plugins;
mod store;

use std::sync::{Arc, Mutex};

use crate::core::traits::Hotkey;
use crate::core::ui_bridge::{HostChannel, Page, UiSnapshot};
use crate::ui::widgets::hotkey_capture;

/// The settings window is a viewport of its own, so a background thread that
/// finishes work has to wake that viewport by id. Waking only the root leaves
/// the settings window showing "Loading..." until the user moves the mouse.
pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-settings")
}

pub struct Readme(egui_commonmark::CommonMarkCache);

impl Default for Readme {
    fn default() -> Self {
        Self(egui_commonmark::CommonMarkCache::default())
    }
}

impl Readme {
    pub fn show(&mut self, ui: &mut egui::Ui, text: &str) {
        egui_commonmark::CommonMarkViewer::new().show(ui, &mut self.0, text);
    }
}

#[derive(Default)]
pub struct SettingsState {
    pub page: Page,
    pub open_plugin: Option<String>,
    pub capture_owner: Option<String>,
    pub pending: Option<(String, Hotkey, String)>,
    pub readme: Readme,
    pub store: store::Store,
    pub update: about::Update,
}

impl SettingsState {
    pub fn begin_capture(&mut self, owner: String) {
        hotkey_capture::stop();
        self.pending = None;
        self.capture_owner = Some(owner);
        hotkey_capture::start();
    }

    pub fn end_capture(&mut self) {
        hotkey_capture::stop();
        self.capture_owner = None;
        self.pending = None;
    }
}

/// The deferred viewport callback must be Send + Sync + 'static, so everything
/// it draws lives behind one lock instead of being borrowed from the app.
pub struct Settings {
    shared: Arc<Mutex<Shared>>,
}

struct Shared {
    state: SettingsState,
    snapshot: UiSnapshot,
    to_host: Arc<HostChannel>,
    closed: bool,
}

impl Settings {
    pub fn new(to_host: Arc<HostChannel>, snapshot: UiSnapshot) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                state: SettingsState::default(),
                snapshot,
                to_host,
                closed: false,
            })),
        }
    }

    pub fn set_snapshot(&self, snapshot: UiSnapshot) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.snapshot = snapshot;
        }
    }

    pub fn open_at(&self, page: Page) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.page = page;
            shared.state.open_plugin = None;
            shared.closed = false;
        }
    }

    pub fn was_closed(&self) -> bool {
        self.shared.lock().map(|shared| shared.closed).unwrap_or(false)
    }

    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.end_capture();
            shared.closed = false;
        }
    }

    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let mut builder = egui::ViewportBuilder::default()
            .with_title("WinCraft Settings")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0]);
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }

        ctx.show_viewport_deferred(
            viewport_id(),
            builder,
            move |ui, _class| {
                let Ok(mut shared) = shared.lock() else {
                    return;
                };
                let Shared {
                    state,
                    snapshot,
                    to_host,
                    closed,
                } = &mut *shared;

                egui::Panel::left("wincraft-nav")
                    .resizable(false)
                    .exact_size(200.0)
                    .show(ui, |ui| {
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new("WinCraft").heading().strong());
                        ui.label(
                            egui::RichText::new(format!("version {}", snapshot.version))
                                .weak()
                                .small(),
                        );
                        ui.add_space(20.0);
                        for (page, label) in PAGES {
                            if ui
                                .selectable_label(state.page == *page, *label)
                                .clicked()
                            {
                                state.page = *page;
                                state.end_capture();
                            }
                        }
                    });

                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| match state.page {
                            Page::General => general::show(ui, snapshot, state, to_host),
                            Page::Plugins => plugins::show(ui, snapshot, state, to_host),
                            Page::Store => store::show(ui, snapshot, state, to_host),
                            Page::About => about::show(ui, snapshot, state),
                        });
                });

                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    *closed = true;
                }
            },
        );
    }
}

const PAGES: &[(Page, &str)] = &[
    (Page::General, "General"),
    (Page::Plugins, "Plugins"),
    (Page::Store, "Plugin Store"),
    (Page::About, "About"),
];
