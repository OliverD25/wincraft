use std::sync::mpsc::{channel, Receiver, Sender};

use crate::core::theme::{self, Tokens};
use crate::core::traits::FieldKind;
use crate::core::ui_bridge::UiSnapshot;
use crate::store::github;
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::text;

#[derive(Clone, Debug, PartialEq)]
enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(String),
    NoRelease,
    Failed,
}

pub struct Update {
    state: UpdateState,
    tx: Sender<UpdateState>,
    rx: Receiver<UpdateState>,
}

impl Default for Update {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            state: UpdateState::Idle,
            tx,
            rx,
        }
    }
}

impl Update {
    fn check(&mut self, ctx: &egui::Context, current: String) {
        if self.state == UpdateState::Checking {
            return;
        }
        self.state = UpdateState::Checking;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let state = match github::latest_release_tag() {
                Ok(tag) if github::is_newer(&tag, &current) => UpdateState::Available(tag),
                Ok(_) => UpdateState::UpToDate,
                // GitHub answers 404 until the first release is published, which
                // is a different fact from being offline.
                Err(err) if err.contains("404") => UpdateState::NoRelease,
                Err(_) => UpdateState::Failed,
            };
            let _ = tx.send(state);
            ctx.request_repaint_of(super::viewport_id());
        });
    }

    fn drain(&mut self) {
        while let Ok(state) = self.rx.try_recv() {
            self.state = state;
        }
    }
}

pub fn show(ui: &mut egui::Ui, snapshot: &UiSnapshot, state: &mut SettingsState) {
    let tokens = Tokens::get(ui.ctx());
    state.update.drain();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page::page_header(
                ui,
                "About",
                None,
                "The source code is public on GitHub.",
                |_| {},
            );
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.y = 0.0;

            let version = format!(
                "{} \u{00B7} built {}",
                snapshot.version,
                env!("WINCRAFT_BUILD_DATE")
            );
            settings_row(ui, "version", RowText::new("Version"), false, |ui| {
                text::mono(ui, &version, 12.0, tokens.text_secondary);
            });

            settings_row(ui, "links", RowText::new("Links"), false, |ui| {
                if button::button(ui, "Repository", Kind::Secondary).clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(github::repo_url()));
                }
                if button::button(ui, "Report an issue", Kind::Secondary).clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(github::issues_url()));
                }
            });

            let current = snapshot.version.clone();
            settings_row(
                ui,
                "updates",
                RowText::new("Updates").desc(
                    "Checks GitHub releases once, when you ask.",
                    tokens.text_secondary,
                ),
                false,
                |ui| {
                    update_status(ui, &state.update.state);
                    if button::button(ui, "Check for updates", Kind::Primary).clicked() {
                        let ctx = ui.ctx().clone();
                        state.update.check(&ctx, current);
                    }
                },
            );

            ui.add_space(20.0);
            text::group_header(ui, "For plugin authors");
            ui.add_space(6.0);
            authors_note(ui);
        });
}

/// The result sits in the status text beside the button; the button itself
/// never changes, so it is always where the user left it.
fn update_status(ui: &mut egui::Ui, state: &UpdateState) {
    let tokens = Tokens::get(ui.ctx());
    match state {
        UpdateState::Idle => {}
        UpdateState::Checking => {
            text::secondary(ui, "Checking\u{2026}", tokens.text_disabled);
        }
        UpdateState::UpToDate => {
            text::secondary(ui, "Up to date", tokens.success);
        }
        UpdateState::Available(tag) => {
            if text::link(ui, &format!("{tag} available \u{2014} Open release")).clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(github::release_url(tag)));
            }
        }
        UpdateState::NoRelease => {
            text::secondary(ui, "No release published yet", tokens.text_secondary);
        }
        UpdateState::Failed => {
            text::secondary(ui, "Could not check \u{2014} offline", tokens.warning);
        }
    }
}

fn authors_note(ui: &mut egui::Ui) {
    let tokens = Tokens::get(ui.ctx());
    let kinds = FieldKind::ALL
        .iter()
        .map(FieldKind::name)
        .collect::<Vec<_>>()
        .join(" \u{00B7} ");
    let body = || text::format(theme::lora(13.0), tokens.text_secondary, Some(19.0));
    let mut job = egui::text::LayoutJob::default();
    job.append("Declare fields of kind ", 0.0, body());
    job.append(
        &kinds,
        0.0,
        text::format(theme::mono(12.0), tokens.text_primary, Some(19.0)),
    );
    job.append(
        "; the settings page draws and saves them for you. See ",
        0.0,
        body(),
    );
    job.append(
        "src/plugins/<id>/README.md",
        0.0,
        text::format(theme::lora(13.0), tokens.accent, Some(19.0)),
    );
    job.append(".", 0.0, body());
    let width = ui.available_width().min(560.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        text::wrapped(ui, job);
    });
}
