//! The Layout page: every window LayoutKeeper has saved, grouped by app,
//! with the desktop each one comes back to.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::log_tail::{self, Entry};
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    ArrangeAction, HostChannel, HostRequest, LayoutAction, LayoutApp, LayoutEntry, LayoutSnapshot,
    UiSnapshot,
};
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::badge::badge;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::input::{self, Face};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::{choice, text};

/// How often the page asks for fresh data while it is on screen.
const REFRESH: Duration = Duration::from_secs(2);
/// How often the activity list reads the log again while the page is open.
const LOG_REFRESH: Duration = Duration::from_secs(5);
const LOG_LINES: usize = 200;
const LOG_TARGET: &str = "layout_keeper";
const PLUGIN: &str = "layout_keeper";
const SAME_TITLE_HINT: &str = "Restores cannot tell these windows apart. Give one a name.";

/// The "Recent activity" list: LayoutKeeper's last log lines, read on a
/// thread of their own so the page never waits for the disk.
#[derive(Default)]
pub struct LogView {
    entries: Vec<Entry>,
    filter: String,
    loaded: Option<Instant>,
    pending: Option<Receiver<Vec<Entry>>>,
}

impl LogView {
    /// Takes what a finished read found, and starts a new read when the
    /// list is old or the person asked for one.
    fn update(&mut self, ctx: &egui::Context, path: &std::path::Path, force: bool) {
        if let Some(receiver) = &self.pending {
            if let Ok(entries) = receiver.try_recv() {
                self.entries = entries;
                self.loaded = Some(Instant::now());
                self.pending = None;
            }
        }
        let old = self.loaded.is_none_or(|at| at.elapsed() >= LOG_REFRESH);
        if self.pending.is_some() || !(old || force) {
            return;
        }
        let (sender, receiver) = channel();
        let (path, ctx) = (path.to_path_buf(), ctx.clone());
        std::thread::spawn(move || {
            let entries = log_tail::tail(&path, LOG_TARGET, LOG_LINES);
            let _ = sender.send(entries);
            ctx.request_repaint_of(crate::ui::settings::viewport_id());
        });
        self.pending = Some(receiver);
    }

    /// The entries that contain the filter text, in any case.
    fn shown(&self) -> Vec<&Entry> {
        let wanted = self.filter.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|entry| {
                wanted.is_empty()
                    || [&entry.when, &entry.level, &entry.text]
                        .iter()
                        .any(|part| part.to_lowercase().contains(&wanted))
            })
            .collect()
    }
}

/// What the page asked for while it was drawn.
#[derive(Default)]
struct Requests {
    actions: Vec<LayoutAction>,
    /// Open windows to ask a name for (WindowNamer's prompt).
    renames: Vec<isize>,
    refresh_log: bool,
    open_log: bool,
}

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let due = state
        .layout_asked
        .is_none_or(|asked| asked.elapsed() >= REFRESH);
    if due {
        to_host.send(HostRequest::LayoutRefresh);
        state.layout_asked = Some(Instant::now());
    }
    ui.ctx().request_repaint_after(REFRESH);

    let mut requests = Requests::default();
    let SettingsState { layout, log, .. } = state;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            body(ui, layout.as_ref(), &mut requests);
            activity(ui, log, &mut requests);
        });
    log.update(ui.ctx(), &snapshot.log_path, requests.refresh_log);
    if requests.open_log {
        to_host.send(HostRequest::OpenPath(snapshot.log_path.clone()));
    }
    for action in requests.actions {
        to_host.send(HostRequest::Layout {
            plugin: PLUGIN.to_string(),
            action,
        });
    }
    for hwnd in requests.renames {
        to_host.send(HostRequest::Arrange {
            plugin: PLUGIN.to_string(),
            action: ArrangeAction::AskName(hwnd),
        });
    }
}

fn body(ui: &mut egui::Ui, layout: Option<&LayoutSnapshot>, requests: &mut Requests) {
    let tokens = Tokens::get(ui.ctx());
    let description = "Where each saved window comes back to after a restart. A new desktop \
        is saved at once, and an open window moves there now.";
    page::page_header(ui, "Layout", None, description, |ui| {
        let Some(layout) = layout else {
            return;
        };
        ui.spacing_mut().item_spacing.x = 8.0;
        if button::button(ui, "Restore now", Kind::Primary).clicked() {
            requests.actions.push(LayoutAction::RestoreNow);
        }
        if button::button(ui, "Save now", Kind::Secondary).clicked() {
            requests.actions.push(LayoutAction::SaveNow);
        }
        let saved = match &layout.last_saved {
            Some(time) => format!("Last saved {time}"),
            None => "Not saved yet this session".to_string(),
        };
        text::secondary(ui, &saved, tokens.text_secondary);
    });
    ui.add_space(16.0);

    let Some(layout) = layout else {
        empty_state(
            ui,
            "No saved layout to show",
            "LayoutKeeper is off, or has not answered yet. Switch it on in Plugins.",
        );
        return;
    };
    if layout.apps.is_empty() {
        empty_state(
            ui,
            "Nothing is saved yet",
            "The layout is saved every few seconds once app windows are open.",
        );
        return;
    }
    for app in &layout.apps {
        app_section(ui, layout, app, requests);
    }
}

fn app_section(
    ui: &mut egui::Ui,
    layout: &LayoutSnapshot,
    app: &LayoutApp,
    requests: &mut Requests,
) {
    let count = app.windows.len();
    let noun = if count == 1 { "window" } else { "windows" };
    page::section_header(ui, &format!("{} \u{b7} {count} {noun}", app.label));
    ui.spacing_mut().item_spacing.y = 0.0;
    for window in &app.windows {
        window_row(ui, layout, app, window, requests);
    }
}

fn window_row(
    ui: &mut egui::Ui,
    layout: &LayoutSnapshot,
    app: &LayoutApp,
    window: &LayoutEntry,
    requests: &mut Requests,
) {
    let tokens = Tokens::get(ui.ctx());
    let id = format!("{}/{}/{}", app.key, window.entry.exe, window.entry.index);
    let (state_text, state_colour) = match (&window.hwnd, &window.closed_since) {
        (Some(_), _) => ("open now".to_string(), tokens.success),
        (None, Some(day)) => (format!("closed since {day}"), tokens.text_secondary),
        (None, None) => ("closed".to_string(), tokens.text_secondary),
    };
    let row = RowText::new(&window.title).desc(&state_text, state_colour);
    settings_row(ui, &id, row, false, |ui| {
        if let Some(monitor) = &window.monitor {
            badge(ui, monitor, tokens.border, tokens.text_secondary);
        }
        if window.same_title {
            badge(ui, "Same title", tokens.warning, tokens.warning).on_hover_text(SAME_TITLE_HINT);
            if let (Some(hwnd), true) = (window.hwnd, layout.can_rename) {
                if button::button(ui, "Rename\u{2026}", Kind::Secondary).clicked() {
                    requests.renames.push(hwnd);
                }
            }
        }
        if window.desktop_removed {
            text::secondary(ui, "desktop removed", tokens.warning);
        }
        let names: Vec<&str> = layout.desktops.iter().map(|d| d.name.as_str()).collect();
        let selected = layout
            .desktops
            .iter()
            .position(|desktop| desktop.id.eq_ignore_ascii_case(&window.desktop));
        if let Some(index) = choice::dropdown(ui, &id, &names, selected) {
            requests.actions.push(LayoutAction::SetDesktop {
                entry: window.entry.clone(),
                desktop: layout.desktops[index].id.clone(),
                hwnd: window.hwnd,
            });
        }
        let closed = window.hwnd.is_none();
        let forget = ui.add_enabled_ui(closed, |ui| button::button(ui, "Forget", Kind::Danger));
        let clicked = forget
            .inner
            .on_disabled_hover_text("An open window is saved again at once")
            .clicked();
        if clicked {
            requests
                .actions
                .push(LayoutAction::Forget(window.entry.clone()));
        }
    });
}

/// The "Recent activity" section: filter, refresh, the log file, and the
/// lines, newest first.
fn activity(ui: &mut egui::Ui, log: &mut LogView, requests: &mut Requests) {
    let tokens = Tokens::get(ui.ctx());
    page::section_header(ui, "Recent activity");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        input::text(ui, "layout-log-filter", &mut log.filter, Face::Text, 240.0);
        if button::button(ui, "Refresh", Kind::Secondary).clicked() {
            requests.refresh_log = true;
        }
        if button::button(ui, "Open log file", Kind::Secondary).clicked() {
            requests.open_log = true;
        }
        if log.pending.is_some() {
            text::caption(ui, "Reading the log\u{2026}", tokens.text_disabled);
        }
    });
    ui.add_space(8.0);

    let shown = log.shown();
    if shown.is_empty() {
        let line = if log.loaded.is_none() {
            "Reading the log\u{2026}"
        } else if log.entries.is_empty() {
            "LayoutKeeper has not logged anything yet."
        } else {
            "No line matches the filter."
        };
        text::caption(ui, line, tokens.text_secondary);
        return;
    }
    egui::Frame::NONE
        .stroke(theme::line(ui.ctx(), tokens.border))
        .corner_radius(tokens.radius_row)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("layout-log")
                .max_height(320.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for entry in shown {
                        text::wrapped(ui, entry_job(entry, &tokens));
                    }
                });
        });
}

/// One line in the mono face: the time dim, the level in its colour.
fn entry_job(entry: &Entry, tokens: &Tokens) -> egui::text::LayoutJob {
    let level_colour = match entry.level.as_str() {
        "ERROR" => tokens.danger,
        "WARN" => tokens.warning,
        _ => tokens.text_secondary,
    };
    let mut job = text::job(
        &entry.when,
        theme::mono(12.0),
        tokens.text_disabled,
        Some(16.0),
    );
    job.append(
        &entry.level,
        8.0,
        text::format(theme::mono(12.0), level_colour, Some(16.0)),
    );
    job.append(
        &entry.text,
        8.0,
        text::format(theme::mono(12.0), tokens.text_primary, Some(16.0)),
    );
    job
}
