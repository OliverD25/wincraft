//! The Layout page: every window LayoutKeeper has saved, grouped by app,
//! with the desktop each one comes back to.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::theme::Tokens;
use crate::core::ui_bridge::{
    HostChannel, HostRequest, LayoutAction, LayoutApp, LayoutEntry, LayoutSnapshot,
};
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::badge::badge;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::{choice, text};

/// How often the page asks for fresh data while it is on screen.
const REFRESH: Duration = Duration::from_secs(2);
const PLUGIN: &str = "layout_keeper";

pub fn show(ui: &mut egui::Ui, state: &mut SettingsState, to_host: &Arc<HostChannel>) {
    let due = state
        .layout_asked
        .is_none_or(|asked| asked.elapsed() >= REFRESH);
    if due {
        to_host.send(HostRequest::LayoutRefresh);
        state.layout_asked = Some(Instant::now());
    }
    ui.ctx().request_repaint_after(REFRESH);

    let mut actions: Vec<LayoutAction> = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            body(ui, state.layout.as_ref(), &mut actions);
        });
    for action in actions {
        to_host.send(HostRequest::Layout {
            plugin: PLUGIN.to_string(),
            action,
        });
    }
}

fn body(ui: &mut egui::Ui, layout: Option<&LayoutSnapshot>, actions: &mut Vec<LayoutAction>) {
    let tokens = Tokens::get(ui.ctx());
    let description = "Where each saved window comes back to after a restart. A new desktop \
        is saved at once, and an open window moves there now.";
    page::page_header(ui, "Layout", None, description, |ui| {
        let Some(layout) = layout else {
            return;
        };
        ui.spacing_mut().item_spacing.x = 8.0;
        if button::button(ui, "Restore now", Kind::Primary).clicked() {
            actions.push(LayoutAction::RestoreNow);
        }
        if button::button(ui, "Save now", Kind::Secondary).clicked() {
            actions.push(LayoutAction::SaveNow);
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
        app_section(ui, layout, app, actions);
    }
}

fn app_section(
    ui: &mut egui::Ui,
    layout: &LayoutSnapshot,
    app: &LayoutApp,
    actions: &mut Vec<LayoutAction>,
) {
    let count = app.windows.len();
    let noun = if count == 1 { "window" } else { "windows" };
    page::section_header(ui, &format!("{} \u{b7} {count} {noun}", app.label));
    ui.spacing_mut().item_spacing.y = 0.0;
    for window in &app.windows {
        window_row(ui, layout, app, window, actions);
    }
}

fn window_row(
    ui: &mut egui::Ui,
    layout: &LayoutSnapshot,
    app: &LayoutApp,
    window: &LayoutEntry,
    actions: &mut Vec<LayoutAction>,
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
        if window.desktop_removed {
            text::secondary(ui, "desktop removed", tokens.warning);
        }
        let names: Vec<&str> = layout.desktops.iter().map(|d| d.name.as_str()).collect();
        let selected = layout
            .desktops
            .iter()
            .position(|desktop| desktop.id.eq_ignore_ascii_case(&window.desktop));
        if let Some(index) = choice::dropdown(ui, &id, &names, selected) {
            actions.push(LayoutAction::SetDesktop {
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
            actions.push(LayoutAction::Forget(window.entry.clone()));
        }
    });
}
