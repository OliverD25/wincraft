use std::sync::mpsc::{channel, Receiver, Sender};

use crate::core::traits::FieldKind;
use crate::core::ui_bridge::UiSnapshot;
use crate::store::github;
use crate::ui::settings::SettingsState;

pub struct Update {
    pub status: String,
    checking: bool,
    tx: Sender<Result<String, String>>,
    rx: Receiver<Result<String, String>>,
}

impl Default for Update {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            status: String::new(),
            checking: false,
            tx,
            rx,
        }
    }
}

impl Update {
    fn check(&mut self, ctx: &egui::Context, current: String) {
        if self.checking {
            return;
        }
        self.checking = true;
        self.status = "Checking\u{2026}".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = github::latest_release_tag().map(|tag| {
                if github::is_newer(&tag, &current) {
                    format!("{tag} is available.")
                } else {
                    format!("You are up to date ({tag} is the latest release).")
                }
            });
            let _ = tx.send(result);
            ctx.request_repaint_of(super::viewport_id());
        });
    }

    fn drain(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.status = match result {
                Ok(text) => text,
                Err(err) => format!("Could not check: {err}"),
            };
            self.checking = false;
        }
    }
}

pub fn show(ui: &mut egui::Ui, snapshot: &UiSnapshot, state: &mut SettingsState) {
    state.update.drain();

    ui.heading("About WinCraft");
    ui.add_space(12.0);
    ui.label(format!("Version {}", snapshot.version));
    ui.label(format!("Built for Windows 11 \u{00B7} {} plugins", snapshot.plugins.len()));
    ui.add_space(16.0);

    ui.horizontal(|ui| {
        if ui.button("Repository").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(github::repo_url()));
        }
        if ui.button("Report an issue").clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(github::issues_url()));
        }
        if ui.button("Releases").clicked() {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(github::releases_url()));
        }
    });
    ui.add_space(16.0);

    ui.horizontal(|ui| {
        if ui.button("Check for updates").clicked() {
            let ctx = ui.ctx().clone();
            state.update.check(&ctx, snapshot.version.clone());
        }
        if !state.update.status.is_empty() {
            ui.label(egui::RichText::new(&state.update.status).weak());
        }
    });

    ui.add_space(24.0);
    ui.separator();
    ui.add_space(12.0);
    ui.label(egui::RichText::new("For plugin authors").strong());
    ui.label(
        egui::RichText::new(format!(
            "This build can draw these setting types: {}.",
            FieldKind::ALL
                .iter()
                .map(FieldKind::name)
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .small()
        .weak(),
    );
    ui.label(
        egui::RichText::new(
            "Declare them in settings_fields() and WinCraft draws and saves them for you.",
        )
        .small()
        .weak(),
    );
}
