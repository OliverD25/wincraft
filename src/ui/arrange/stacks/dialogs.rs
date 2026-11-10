//! The name box for a new group or a new name, and the question before
//! closing a group's windows. Each sits on top of the strip and has the
//! keyboard: Enter does what its main button says, Esc cancels.

use super::*;
use crate::ui::widgets::button::{self, Kind as ButtonKind};

const WIDTH: f32 = 360.0;
const MAX_CHARS: usize = 120;

/// "A, B and C".
pub fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The close question's title and text.
pub fn close_words(app_label: &str, windows: &[(isize, String)]) -> (String, String) {
    let names: Vec<String> = windows.iter().map(|(_, label)| label.clone()).collect();
    (
        format!("Close {} windows in {app_label}?", windows.len()),
        format!(
            "WinCraft will close {}. The group stays, and the windows join it again when they open.",
            and_list(&names)
        ),
    )
}

/// What the dialog's buttons or keys decided this frame.
enum Outcome {
    Stay,
    Cancel,
    Confirm,
}

/// Draws the dialog and does what it decided.
pub fn show(ctx: &egui::Context, shared: &mut Shared, panel: Rect) {
    let Some(mut dialog) = shared.stacks.dialog.take() else {
        return;
    };
    let tokens = Tokens::get(ctx);
    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let mut outcome = Outcome::Stay;
    let at = pos2(panel.center().x - WIDTH / 2.0, panel.top() + 48.0);
    let area = egui::Area::new(egui::Id::new("deck-group-dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(tokens.window_bg)
                .stroke(theme::line(ctx, tokens.border))
                .corner_radius(8)
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.set_width(WIDTH - 32.0);
                    ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                    let (title, main) = match &mut dialog {
                        Dialog::Name {
                            labels,
                            text,
                            focused,
                            ..
                        } => {
                            let title = format!("Group {} windows", labels.len());
                            name_box(ui, &tokens, &title, text, focused);
                            (None, "Make the group".to_string())
                        }
                        Dialog::Rename { text, focused, .. } => {
                            name_box(ui, &tokens, "Rename the group", text, focused);
                            (None, "Rename".to_string())
                        }
                        Dialog::Close {
                            app_label, windows, ..
                        } => {
                            let (title, words) = close_words(app_label, windows);
                            (
                                Some((title, words)),
                                format!("Close {} windows", windows.len()),
                            )
                        }
                    };
                    if let Some((title, words)) = title {
                        ui.label(
                            egui::RichText::new(title)
                                .font(theme::semibold(14.0))
                                .color(tokens.text_primary),
                        );
                        text::wrapped(
                            ui,
                            text::job(&words, theme::regular(13.0), tokens.text_secondary, None),
                        );
                    }
                    ui.horizontal(|ui| {
                        if button::button(ui, "Cancel", ButtonKind::Secondary).clicked() {
                            outcome = Outcome::Cancel;
                        }
                        let kind = if matches!(dialog, Dialog::Close { .. }) {
                            ButtonKind::Danger
                        } else {
                            ButtonKind::Primary
                        };
                        if button::button(ui, &main, kind).clicked() {
                            outcome = Outcome::Confirm;
                        }
                    });
                });
        });
    shared.stacks.covers.push(area.response.rect);
    if escape {
        outcome = Outcome::Cancel;
    } else if enter && matches!(outcome, Outcome::Stay) {
        outcome = Outcome::Confirm;
    }
    match outcome {
        Outcome::Stay => shared.stacks.dialog = Some(dialog),
        Outcome::Cancel => {
            log::info!("WindowDeck: the group box was cancelled");
        }
        Outcome::Confirm => confirm(shared, dialog),
    }
    ctx.request_repaint();
}

/// A title and a one-line text box that takes the keyboard.
fn name_box(ui: &mut Ui, tokens: &Tokens, title: &str, text: &mut String, focused: &mut bool) {
    ui.label(
        egui::RichText::new(title)
            .font(theme::semibold(14.0))
            .color(tokens.text_primary),
    );
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .id_salt("deck-group-name")
            .font(theme::regular(13.0))
            .char_limit(MAX_CHARS)
            .desired_width(f32::INFINITY),
    );
    // Asked for on every frame until it arrives: the window may take a
    // frame or two to hand the keyboard over.
    if response.has_focus() {
        if !*focused {
            // The first frame with the keyboard: the whole name is picked,
            // so typing replaces it.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let end = egui::text::CCursor::new(text.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        end,
                    )));
                state.store(ui.ctx(), response.id);
            }
        }
        *focused = true;
    } else if !*focused {
        response.request_focus();
    }
}

fn confirm(shared: &mut Shared, dialog: Dialog) {
    let Some(mut defs) = shared.snapshot.as_ref().and_then(|s| s.deck_groups.clone()) else {
        return;
    };
    match dialog {
        Dialog::Name {
            app, labels, text, ..
        } => match deck_groups::make(&mut defs, &app, &text, &labels) {
            Some(name) => {
                log::info!(
                    "WindowDeck: group \"{name}\" made of {} windows",
                    labels.len()
                );
                shared.stacks.picked.clear();
                save_groups(shared, defs);
            }
            None => {
                shared.stacks.dialog = Some(Dialog::Name {
                    app,
                    labels,
                    text,
                    focused: true,
                })
            }
        },
        Dialog::Rename { app, old, text, .. } => {
            if let Some(new) = deck_groups::rename(&mut defs, &app, &old, &text) {
                if let Some((_, name)) = shared
                    .stacks
                    .open
                    .as_mut()
                    .filter(|(a, n)| *a == app && *n == old)
                {
                    *name = new;
                }
                save_groups(shared, defs);
            } else {
                shared.stacks.dialog = Some(Dialog::Rename {
                    app,
                    old,
                    text,
                    focused: true,
                });
            }
        }
        Dialog::Close { windows, name, .. } => {
            log::info!(
                "WindowDeck closes the {} windows of group \"{name}\" after asking",
                windows.len()
            );
            let plugin = shared
                .snapshot
                .as_ref()
                .map(|s| s.plugin.clone())
                .unwrap_or_default();
            for (hwnd, _) in windows {
                shared.ask_close(hwnd);
                shared.to_host.send(HostRequest::Arrange {
                    plugin: plugin.clone(),
                    action: ArrangeAction::Close(hwnd),
                });
            }
        }
    }
}

/// Keeps the new groups on screen at once and asks the look plugin to save
/// them.
pub fn save_groups(shared: &mut Shared, defs: Vec<DeckGroup>) {
    let Some(snapshot) = shared.snapshot.as_mut() else {
        return;
    };
    snapshot.deck_groups = Some(defs.clone());
    shared.stacks.refold(snapshot);
    let plugin = snapshot.look_plugin.clone();
    shared.to_host.send(HostRequest::Arrange {
        plugin,
        action: ArrangeAction::SetGroups(defs),
    });
}
