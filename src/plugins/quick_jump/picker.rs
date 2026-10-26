//! "Pin a window…": the open windows to choose from for one slot, with a
//! search box. Enter pins the chosen window (it moves if it is pinned
//! elsewhere), Esc closes.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{pos2, vec2, Align2, Color32, Key, Rect, Sense, StrokeKind, TextEdit, Ui, UiBuilder};

use crate::core::theme::{self, Tokens};
use crate::core::traits::{PluginWindow, WindowFrame};
use crate::core::ui_bridge::HostRequest;
use crate::search::icons::IconCache;
use crate::search::IconRef;
use crate::ui::fuzzy;
use crate::ui::settings;
use crate::ui::widgets::focus;

use super::list::clip_text;
use super::matching::Live;
use super::pictures::Pictures;
use super::pins::digit_of_slot;
use super::shared::{Handle, Request, View};

pub const WINDOW_ID: &str = "picker";
const TITLE: &str = "QuickJump \u{b7} Pin a window";
const WIDTH: f32 = 620.0;
const ROW: f32 = 56.0;
const ROWS: usize = 8;
const FOOTER: f32 = 40.0;
const THUMB: egui::Vec2 = egui::Vec2::new(64.0, 36.0);

/// The open windows the search finds, best first; all of them, top of the
/// z-order first, for an empty search.
pub fn found<'a>(windows: &'a [Live], query: &str) -> Vec<&'a Live> {
    let query = query.trim();
    if query.is_empty() {
        return windows.iter().take(ROWS).collect();
    }
    let mut scored: Vec<(i32, &Live)> = windows
        .iter()
        .filter_map(|window| {
            let text = format!("{} {}", window.shown, window.app_name());
            fuzzy::score(query, &text).map(|score| (score, window))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .map(|(_, window)| window)
        .take(ROWS)
        .collect()
}

#[derive(Default)]
struct PickerUi {
    query: String,
    selected: usize,
    window: isize,
    focused: bool,
    pictures: Pictures,
    icons: Option<IconCache>,
    slot: Option<usize>,
}

pub struct Picker {
    shared: Handle,
    state: Arc<Mutex<PickerUi>>,
}

impl Picker {
    pub fn new(shared: Handle) -> Self {
        Self {
            shared,
            state: Arc::new(Mutex::new(PickerUi::default())),
        }
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-quick-jump-picker")
}

/// What a frame of the picker asks for.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Pin(isize),
    Close,
}

impl PluginWindow for Picker {
    fn id(&self) -> &'static str {
        WINDOW_ID
    }

    fn open(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.picker = shared.picker.or(Some(0));
        }
    }

    fn close(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.picker = None;
        }
        if let Ok(mut state) = self.state.lock() {
            state.pictures.clear();
            state.window = 0;
        }
    }

    fn is_open(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|shared| shared.picker.is_some())
    }

    fn show(&mut self, ctx: &egui::Context, frame: &WindowFrame) {
        let height = 120.0 + ROWS as f32 * ROW + FOOTER;
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([WIDTH, height])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_minimize_button(false);
        if let Some(icon) = &frame.icon {
            builder = builder.with_icon(Arc::clone(icon));
        }
        let shared = Arc::clone(&self.shared);
        let state = Arc::clone(&self.state);
        let palette = frame.palette_hwnd;
        let to_host = Arc::clone(&frame.to_host);
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let (Ok(mut shared), Ok(mut state)) = (shared.lock(), state.lock()) else {
                return;
            };
            let Some(slot) = shared.picker else {
                return;
            };
            if state.slot != Some(slot) {
                state.slot = Some(slot);
                state.query.clear();
                state.selected = 0;
                state.focused = false;
                shared.send(Request::Refresh);
            }
            let tokens = Tokens::get(ui.ctx());
            if state.window == 0 {
                state.window = settings::find_window(TITLE, palette);
            }
            if state.window != 0 {
                settings::sync_title_bar(state.window, tokens.dark);
                if !std::mem::replace(&mut state.focused, true) {
                    to_host.send(HostRequest::FocusWindow(state.window));
                }
            }
            if state.icons.is_none() {
                state.icons = Some(IconCache::new(ui.ctx()));
            }
            let window = state.window;
            state.pictures.begin(window);
            let view = shared.view.clone();
            let mut outcomes = Vec::new();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(tokens.elevated_bg))
                .show(ui, |ui| {
                    contents(ui, &view, slot, &mut state, &mut outcomes)
                });
            state.pictures.end();
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                outcomes.push(Outcome::Close);
            }
            for outcome in outcomes {
                match outcome {
                    Outcome::Pin(hwnd) => {
                        shared.send(Request::Pin { slot, hwnd });
                        shared.select = Some(slot);
                        shared.picker = None;
                    }
                    Outcome::Close => shared.picker = None,
                }
            }
            if shared.picker.is_none() {
                state.slot = None;
                state.pictures.clear();
                state.window = 0;
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

fn contents(ui: &mut Ui, view: &View, slot: usize, state: &mut PickerUi, out: &mut Vec<Outcome>) {
    let tokens = Tokens::get(ui.ctx());
    let area = ui.max_rect();
    let painter = ui.painter().clone();
    let left = area.left() + 24.0;
    let right = area.right() - 24.0;
    let mut y = area.top() + 18.0;
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "QuickJump",
        theme::semibold(12.0),
        tokens.text_disabled,
    );
    y += 18.0;
    let digit = digit_of_slot(slot);
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        format!("Pin a window to slot {digit}"),
        theme::semibold(18.0),
        tokens.text_primary,
    );
    y += 32.0;

    let search = Rect::from_min_size(pos2(left, y), vec2(right - left, 36.0));
    let id = ui.id().with("quick-jump-search");
    let edit = TextEdit::singleline(&mut state.query)
        .id(id)
        .font(theme::regular(14.0))
        .text_color(tokens.text_primary)
        .hint_text(
            egui::RichText::new("Search open windows")
                .font(theme::regular(14.0))
                .color(tokens.text_disabled),
        )
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::symmetric(10, 0))
        .vertical_align(egui::Align::Center)
        .desired_width(search.width() - 20.0);
    let response = ui.put(search, edit);
    if !response.has_focus() {
        response.request_focus();
    }
    if response.changed() {
        state.selected = 0;
    }
    painter.rect_stroke(
        search,
        tokens.radius_row,
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    y = search.bottom() + 12.0;

    let rows = found(&view.windows, &state.query);
    let (up, down, enter, escape) = ui.ctx().input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
            i.key_pressed(Key::Escape),
        )
    });
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    if down && state.selected + 1 < rows.len() {
        state.selected += 1;
    }
    state.selected = state.selected.min(rows.len().saturating_sub(1));
    let pixels_per_point = ui.ctx().pixels_per_point();
    for (index, window) in rows.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(area.left() + 8.0, y), vec2(area.width() - 16.0, ROW));
        let response = ui.interact(rect, id.with(window.hwnd), Sense::click());
        if response.hovered() && ui.ctx().input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            state.selected = index;
        }
        if response.clicked() {
            out.push(Outcome::Pin(window.hwnd));
        }
        if index == state.selected {
            painter.rect_filled(rect, tokens.radius_row, tokens.selection_bg);
        }
        let thumb = Rect::from_min_size(pos2(left, rect.center().y - THUMB.y / 2.0), THUMB);
        painter.rect_filled(thumb, 4, tokens.panel_bg);
        state.pictures.show(window.hwnd, thumb, pixels_per_point);
        let reference = IconRef::Window {
            hwnd: window.hwnd,
            exe: PathBuf::from(&window.exe),
        };
        if let Some(texture) = state
            .icons
            .as_mut()
            .and_then(|cache| cache.get(ui.ctx(), &reference))
        {
            painter.image(
                texture.id(),
                Rect::from_min_size(thumb.max - vec2(12.0, 12.0), vec2(18.0, 18.0)),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let text_left = thumb.right() + 18.0;
        let pinned = view
            .slots
            .iter()
            .find(|view_slot| view_slot.hwnd == Some(window.hwnd));
        let tag_width = if pinned.is_some() { 72.0 } else { 0.0 };
        let room = right - text_left - tag_width;
        painter.text(
            pos2(text_left, rect.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            clip_text(&painter, &window.shown, theme::regular(14.0), room),
            theme::regular(14.0),
            tokens.text_primary,
        );
        let place = window.place();
        let second = if place.is_empty() {
            window.app_name()
        } else {
            format!("{} \u{b7} {place}", window.app_name())
        };
        painter.text(
            pos2(text_left, rect.center().y + 1.0),
            Align2::LEFT_TOP,
            clip_text(&painter, &second, theme::regular(12.0), room),
            theme::regular(12.0),
            tokens.text_secondary,
        );
        if let Some(pinned) = pinned {
            let tag = Rect::from_min_size(
                pos2(right - tag_width + 8.0, rect.center().y - 10.0),
                vec2(tag_width - 8.0, 20.0),
            );
            painter.rect_stroke(
                tag,
                10,
                theme::line(ui.ctx(), tokens.border),
                StrokeKind::Inside,
            );
            painter.text(
                tag.center(),
                Align2::CENTER_CENTER,
                format!("in slot {}", pinned.digit),
                theme::regular(12.0),
                tokens.text_secondary,
            );
        }
        y += ROW;
    }
    if rows.is_empty() {
        painter.text(
            pos2(left, y + 12.0),
            Align2::LEFT_TOP,
            "No open window matches",
            theme::regular(14.0),
            tokens.text_disabled,
        );
    }
    if enter {
        if let Some(window) = rows.get(state.selected) {
            out.push(Outcome::Pin(window.hwnd));
        }
    }
    if escape {
        out.push(Outcome::Close);
    }

    let footer = Rect::from_min_max(pos2(area.left(), area.bottom() - FOOTER), area.max);
    painter.hline(
        footer.x_range(),
        footer.top(),
        theme::line(ui.ctx(), tokens.border),
    );
    ui.scope_builder(
        UiBuilder::new().max_rect(footer.shrink2(vec2(24.0, 0.0))),
        |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(
                    egui::RichText::new(format!("\u{21b5} Pin to {digit}"))
                        .font(theme::regular(12.0))
                        .color(tokens.accent),
                );
                ui.label(
                    egui::RichText::new("\u{b7} \u{2191}\u{2193} Move \u{b7} Esc Cancel")
                        .font(theme::regular(12.0))
                        .color(tokens.text_secondary),
                );
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::super::matching::tests::window;
    use super::*;
    use crate::core::config::{ThemeChoice, ThemeName};

    fn windows() -> Vec<Live> {
        vec![
            window(1, "Chrome", "Mail - Google Chrome"),
            window(2, "Claude", "Claude"),
            window(3, "Code", "main.rs - Code"),
        ]
    }

    #[test]
    fn the_search_finds_windows_by_title_or_app() {
        let windows = windows();
        assert_eq!(found(&windows, "").len(), 3);
        let hwnds: Vec<isize> = found(&windows, "claude").iter().map(|w| w.hwnd).collect();
        assert_eq!(hwnds, [2]);
        assert!(found(&windows, "zzzz").is_empty());
    }

    fn draw(state: &mut PickerUi, events: Vec<egui::Event>) -> Vec<Outcome> {
        let view = View {
            windows: windows(),
            ..View::default()
        };
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut out = Vec::new();
        for (frame, name) in ThemeName::ALL.into_iter().enumerate() {
            theme::apply(&ctx, name, ThemeChoice::Light);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(WIDTH, 640.0))),
                events: if frame == 0 {
                    events.clone()
                } else {
                    Vec::new()
                },
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| contents(ui, &view, 4, state, &mut out));
            output.textures_delta.clear();
        }
        out
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn enter_pins_the_chosen_window_and_esc_closes() {
        let mut state = PickerUi::default();
        let out = draw(&mut state, vec![key(Key::ArrowDown), key(Key::Enter)]);
        assert_eq!(out.first(), Some(&Outcome::Pin(2)));
        let mut state = PickerUi::default();
        assert!(draw(&mut state, vec![key(Key::Escape)]).contains(&Outcome::Close));
    }
}
