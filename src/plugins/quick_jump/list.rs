//! The Quick access window: the pinned windows as cards with a live
//! picture each (`grid`), and a details panel that says how QuickJump
//! finds a window. F2, or a click on the selected card, opens the details.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOW};

use egui::{
    pos2, vec2, Align2, Color32, Key, Painter, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::core::traits::{PluginWindow, WindowFrame};
use crate::core::ui_bridge::HostRequest;
use crate::core::window_place;
use crate::search::icons::IconCache;
use crate::search::IconRef;
use crate::ui::dwm_thumbs;
use crate::ui::settings;
use crate::ui::widgets::button::{self, Kind};

use crate::ui::widgets::{choice, focus, input, toggle};

use super::matching::{candidates, check_line, CheckTone, SlotView};
use super::pictures::Pictures;
use super::pins::{number_of_slot, Pin};
use super::shared::{Handle, Request, Shared, View};
use super::spot::{self, Place, Spot};

mod grid;

use grid::window_size;

pub const WINDOW_ID: &str = "list";
const TITLE: &str = "QuickJump \u{b7} Quick access";

const DETAILS: f32 = 360.0;
const NOT_OPEN: f32 = 0.45;
const REFRESH: Duration = Duration::from_secs(1);

/// The pin being edited in the details panel; it changes only on Done.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Draft {
    slot: usize,
    pin: Pin,
}

#[derive(Default)]
struct ListUi {
    selected: usize,
    details: Option<Draft>,
    window: isize,
    focused: bool,
    /// Hidden until it is in its place, then covered until it has drawn.
    reveal: Reveal,
    pictures: Pictures,
    /// One scale for every card's picture, so their sizes compare as on
    /// the screen; worked out at the start of each frame.
    picture_scale: Option<f32>,
    icons: Option<IconCache>,
    drag: Option<usize>,
    refreshed: Option<Instant>,
    closed: bool,
    size: Option<egui::Vec2>,
}

/// What a frame of Quick access asks for.
#[derive(Debug, PartialEq)]
enum Outcome {
    Send(Request),
    Close,
}

pub struct PinList {
    shared: Handle,
    state: Arc<Mutex<ListUi>>,
}

impl PinList {
    pub fn new(shared: Handle) -> Self {
        Self {
            shared,
            state: Arc::new(Mutex::new(ListUi::default())),
        }
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-quick-jump-list")
}

impl PluginWindow for PinList {
    fn id(&self) -> &'static str {
        WINDOW_ID
    }

    fn open(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.list_open = true;
        }
        if let Ok(mut state) = self.state.lock() {
            state.closed = false;
            state.focused = false;
            state.reveal = Reveal::Hidden;
            state.window = 0;
        }
    }

    fn close(&mut self) {
        if let (Ok(mut shared), Ok(state)) = (self.shared.lock(), self.state.lock()) {
            remember_spot(&mut shared, state.window);
        }
        if let Ok(mut shared) = self.shared.lock() {
            shared.list_open = false;
        }
        if let Ok(mut state) = self.state.lock() {
            state.pictures.clear();
            state.details = None;
            state.window = 0;
            state.reveal = Reveal::Hidden;
        }
    }

    fn is_open(&self) -> bool {
        self.shared.lock().is_ok_and(|shared| shared.list_open)
    }

    fn show(&mut self, ctx: &egui::Context, frame: &WindowFrame) {
        if let (Ok(shared), Ok(mut state)) = (self.shared.lock(), self.state.lock()) {
            reveal_step(ctx, &shared, &mut state, frame.palette_hwnd);
        }
        let size = match (self.shared.lock(), self.state.lock()) {
            (Ok(shared), Ok(state)) => {
                window_size(shared.view.slots.len(), state.details.is_some())
            }
            _ => window_size(1, false),
        };
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size(size)
            .with_resizable(false)
            .with_maximize_button(false)
            // Shown by `reveal_step` once it is in its place.
            .with_visible(false);
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
            let tokens = Tokens::get(ui.ctx());
            if state.window == 0 {
                state.window = settings::find_window(TITLE, palette);
            }
            if state.window != 0 {
                settings::sync_title_bar(state.window, tokens.dark);
                if let Reveal::Covered { .. } = state.reveal {
                    // This frame draws into the covered window; uncover after it.
                    state.reveal = Reveal::Drawn;
                    ui.ctx().request_repaint();
                } else if state.reveal == Reveal::Drawn {
                    dwm_thumbs::cloak(state.window as HWND, false);
                    state.reveal = Reveal::Shown;
                }
                if !std::mem::replace(&mut state.focused, true) {
                    to_host.send(HostRequest::FocusWindow(state.window));
                }
            }
            if let Some(slot) = shared.select.take() {
                state.selected = slot;
            }
            if state.icons.is_none() {
                state.icons = Some(IconCache::new(ui.ctx()));
            }
            if state.size != Some(size) {
                state.size = Some(size);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
            if state.refreshed.is_none_or(|at| at.elapsed() >= REFRESH) {
                state.refreshed = Some(Instant::now());
                shared.send(Request::Refresh);
            }
            ui.ctx().request_repaint_after(REFRESH);

            let window = state.window;
            state.pictures.begin(window);
            let view = shared.view.clone();
            let mut outcomes = Vec::new();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(tokens.window_bg))
                .show(ui, |ui| {
                    grid::contents(ui, &view, &mut state, &mut outcomes)
                });
            state.pictures.end();

            if ui.ctx().input(|i| i.viewport().close_requested()) {
                outcomes.push(Outcome::Close);
            }
            for outcome in outcomes {
                match outcome {
                    Outcome::Send(request) => {
                        if matches!(request, Request::Jump(_)) {
                            shared.jumped_from = window_place::rect(state.window as HWND);
                        }
                        shared.send(request)
                    }
                    Outcome::Close => state.closed = true,
                }
            }
            if state.closed {
                remember_spot(&mut shared, state.window);
                shared.list_open = false;
                state.closed = false;
                state.pictures.clear();
                state.details = None;
                state.window = 0;
                state.size = None;
                state.reveal = Reveal::Hidden;
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

/// How far a new Quick access window is on its way to the screen. Windows
/// paints a new window white before egui draws it, so it stays hidden while
/// it moves into place and covered (cloaked) until its first frame is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Reveal {
    #[default]
    Hidden,
    Covered {
        since: Instant,
    },
    Drawn,
    Shown,
}

/// Uncovers the window anyway after this long, so a missed frame never
/// leaves it invisible.
const COVERED_LONGEST: Duration = Duration::from_millis(400);

/// Runs on the main frame, so it works while the window itself is hidden.
fn reveal_step(ctx: &egui::Context, shared: &Shared, state: &mut ListUi, palette: isize) {
    if !shared.list_open {
        return;
    }
    match state.reveal {
        Reveal::Hidden => {
            if state.window == 0 {
                state.window = settings::find_window(TITLE, palette);
            }
            if state.window == 0 {
                ctx.request_repaint();
                return;
            }
            let hwnd = state.window as HWND;
            put_in_place(shared, state.window);
            dwm_thumbs::cloak(hwnd, true);
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            state.reveal = Reveal::Covered {
                since: Instant::now(),
            };
            ctx.request_repaint_of(viewport_id());
            ctx.request_repaint();
        }
        Reveal::Covered { since } if since.elapsed() >= COVERED_LONGEST => {
            dwm_thumbs::cloak(state.window as HWND, false);
            state.reveal = Reveal::Shown;
            log::warn!("QuickJump: Quick access was shown before it had drawn itself");
        }
        Reveal::Covered { .. } | Reveal::Drawn => ctx.request_repaint(),
        Reveal::Shown => {}
    }
}

/// Puts a freshly opened Quick access in the middle of the monitor under
/// the mouse, or where the user left it when the setting says so.
fn put_in_place(shared: &Shared, window: isize) {
    let hwnd = window as HWND;
    match spot::place(!shared.keep_spot, shared.spot) {
        Place::Centre => window_place::centre_on_cursor_monitor(hwnd),
        Place::At(x, y) => window_place::place_at(hwnd, (x, y)),
    }
}

/// Keeps where the window is now, so it can open there next time.
fn remember_spot(shared: &mut Shared, window: isize) {
    let Some((x, y)) = window_place::top_left(window as HWND) else {
        return;
    };
    let now = Spot { x, y };
    if shared.spot != Some(now) {
        shared.spot = Some(now);
        spot::save(&spot::path(), now);
    }
}

fn draft(view: &View, slot: usize) -> Option<Draft> {
    let pin = view.pins.get(slot).cloned()?;
    Some(Draft { slot, pin })
}

fn app_icon(
    ui: &Ui,
    painter: &Painter,
    state: &mut ListUi,
    view: &SlotView,
    rect: Rect,
    alpha: f32,
) {
    let reference = match view.hwnd {
        Some(hwnd) => IconRef::Window {
            hwnd,
            exe: PathBuf::from(&view.exe),
        },
        None => IconRef::Path(PathBuf::from(&view.exe)),
    };
    let texture = state
        .icons
        .as_mut()
        .and_then(|cache| cache.get(ui.ctx(), &reference));
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE.gamma_multiply(alpha),
        );
    }
}

fn name_card(painter: &Painter, area: Rect, name: &str, tokens: &Tokens, alpha: f32) {
    let text = clip_text(painter, name, theme::regular(12.0), area.width() - 8.0);
    painter.text(
        area.center(),
        Align2::CENTER_CENTER,
        text,
        theme::regular(12.0),
        theme::with_alpha(tokens.text_secondary, alpha),
    );
}

fn paint_keycap(painter: &Painter, rect: Rect, key: &str, tokens: &Tokens) {
    painter.rect(
        rect,
        4,
        tokens.panel_bg,
        Stroke::new(1.0, tokens.border),
        StrokeKind::Inside,
    );
    painter.hline(
        rect.x_range().shrink(2.0),
        rect.bottom() - 1.0,
        Stroke::new(2.0, tokens.border),
    );
    painter.text(
        rect.center() - vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        key,
        theme::semibold(12.0),
        tokens.text_secondary,
    );
}

/// The text cut with an ellipsis to fit `width`.
pub fn clip_text(painter: &Painter, text: &str, font: egui::FontId, width: f32) -> String {
    let fits = |candidate: &str| {
        painter
            .layout_no_wrap(candidate.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let candidate = format!(
            "{}\u{2026}",
            chars[..keep].iter().collect::<String>().trim_end()
        );
        if fits(&candidate) {
            return candidate;
        }
    }
    "\u{2026}".to_string()
}

fn details_panel(ui: &mut Ui, view: &View, area: Rect, state: &mut ListUi, out: &mut Vec<Outcome>) {
    let tokens = Tokens::get(ui.ctx());
    ui.painter().rect_filled(area, 0, tokens.panel_bg);
    ui.painter().vline(
        area.left(),
        area.y_range(),
        theme::line(ui.ctx(), tokens.border),
    );
    let Some(draft) = state.details.as_mut() else {
        return;
    };
    let slot = draft.slot;
    let view_slot = view.slots.get(slot).cloned().unwrap_or_default();
    let mut done = false;
    let mut remove = false;
    let inner = area.shrink(20.0);
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            paint_keycap(
                ui.painter(),
                rect,
                &number_of_slot(slot).to_string(),
                &tokens,
            );
            ui.label(
                egui::RichText::new(format!("Number {} details", number_of_slot(slot)))
                    .font(theme::semibold(18.0))
                    .color(tokens.text_primary),
            );
        });
        let place = [view_slot.app_name.as_str(), view_slot.place.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        caption(ui, &place, tokens.text_secondary);
        ui.add_space(12.0);

        field_label(ui, "Label (optional)", &tokens);
        let mut label = draft.pin.label.clone().unwrap_or_default();
        if input::text(
            ui,
            "quick-jump-label",
            &mut label,
            input::Face::Text,
            inner.width(),
        )
        .changed()
        {
            draft.pin.label = (!label.trim().is_empty()).then_some(label);
        }
        caption(
            ui,
            "Shown in the tip and Quick access instead of the window title.",
            tokens.text_secondary,
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("How QuickJump finds this window")
                .font(theme::semibold(13.0))
                .color(tokens.text_primary),
        );
        field_label(ui, "App", &tokens);
        let options: Vec<String> = view
            .apps
            .iter()
            .map(|app| {
                format!(
                    "{} \u{b7} {}",
                    app.name,
                    crate::core::window_identity::exe_key(&app.exe)
                )
            })
            .collect();
        let labels: Vec<&str> = options.iter().map(String::as_str).collect();
        let current = view.apps.iter().position(|app| {
            app.exe.eq_ignore_ascii_case(&draft.pin.exe) && app.app_id == draft.pin.app_id
        });
        if let Some(chosen) = choice::dropdown_sized(
            ui,
            "quick-jump-app",
            &labels,
            current,
            "",
            vec2(inner.width(), 32.0),
        ) {
            if let Some(app) = view.apps.get(chosen) {
                draft.pin.exe = app.exe.clone();
                draft.pin.app_id = app.app_id.clone();
                draft.pin.app_name = app.name.clone();
            }
        }
        field_label(ui, "Title contains", &tokens);
        input::text(
            ui,
            "quick-jump-title",
            &mut draft.pin.title,
            input::Face::Text,
            inner.width(),
        );
        let matches = candidates(&draft.pin, &view.windows);
        let (line, tone) = check_line(matches.len());
        let colour = match tone {
            CheckTone::Success => tokens.success,
            CheckTone::Warning => tokens.warning,
            CheckTone::Quiet => tokens.text_disabled,
        };
        caption(ui, &line, colour);
        if let Some(first) = matches.first() {
            let place = first.place();
            let line = if place.is_empty() {
                first.shown.clone()
            } else {
                format!("{} \u{b7} {place}", first.shown)
            };
            caption(ui, &line, tokens.text_secondary);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            toggle::toggle(ui, &mut draft.pin.start);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("If it is not open, start {}", draft.pin.app_name))
                    .font(theme::regular(14.0))
                    .color(tokens.text_primary),
            );
        });
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 32.0), inner.max);
        ui.scope_builder(UiBuilder::new().max_rect(buttons), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button::button(ui, "Done", Kind::Primary).clicked() {
                    done = true;
                }
                ui.add_space(8.0);
                if button::button(ui, "Remove pin", Kind::Danger).clicked() {
                    remove = true;
                }
            });
        });
    });
    let enter = ui.ctx().input(|i| i.key_pressed(Key::Enter));
    if done || enter {
        out.push(Outcome::Send(Request::Update {
            slot,
            pin: draft.pin.clone(),
        }));
        state.details = None;
    } else if remove {
        out.push(Outcome::Send(Request::Remove(slot)));
        state.details = None;
    }
}

fn field_label(ui: &mut Ui, text: &str, tokens: &Tokens) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .font(theme::regular(12.0))
            .color(tokens.text_secondary),
    );
}

fn caption(ui: &mut Ui, text: &str, colour: Color32) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::regular(12.0))
            .color(colour),
    );
}

/// The window Quick access draws on, for tests that check nothing is
/// drawn on a real one.
#[cfg(test)]
fn window_of(state: &ListUi) -> windows_sys::Win32::Foundation::HWND {
    state.window as windows_sys::Win32::Foundation::HWND
}

#[cfg(test)]
mod tests {
    use super::super::matching::slot_views;
    use super::super::matching::tests::window;
    use super::super::pins::tests::list;
    use super::super::pins::Slots;
    use super::*;
    use crate::core::config::ThemeName;

    fn view() -> View {
        let slots = list(&["Claude", "Code"]);
        let live = vec![window(1, "Claude", "Claude")];
        View {
            slots: slot_views(&slots, &live, 1),
            pins: slots.iter().map(|(_, pin)| pin.clone()).collect(),
            windows: live,
            apps: vec![super::super::shared::App {
                exe: r"C:\Apps\Claude.exe".to_string(),
                app_id: None,
                name: "Claude".to_string(),
            }],
            jump_keys: "Win+`".to_string(),
            pin_keys: String::new(),
        }
    }

    /// Draws one frame per entry of `frames`, each with its events, then
    /// one more in each theme with none.
    fn frames(view: &View, state: &mut ListUi, frames: Vec<Vec<egui::Event>>) -> Vec<Outcome> {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut out = Vec::new();
        let themes = ThemeName::ALL.into_iter().map(|name| (name, Vec::new()));
        let steps = frames
            .into_iter()
            .map(|events| (ThemeName::Classic, events))
            .chain(themes);
        let size = window_size(view.slots.len(), true);
        for (name, events) in steps {
            theme::apply(&ctx, name, crate::core::config::ThemeChoice::Dark);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| grid::contents(ui, view, state, &mut out));
            output.textures_delta.clear();
        }
        out
    }

    fn draw(view: &View, state: &mut ListUi, events: Vec<egui::Event>) -> Vec<Outcome> {
        frames(view, state, vec![Vec::new(), events])
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
    fn quick_access_and_its_details_draw_in_every_theme() {
        let view = view();
        let mut state = ListUi::default();
        let first = draw(&view, &mut state, Vec::new());
        assert!(first.is_empty(), "{first:?}");
        state.details = draft(&view, 0);
        assert!(state.details.is_some());
        draw(&view, &mut state, Vec::new());
        assert!(window_of(&state).is_null(), "no real window is touched");
        let out = draw(&view, &mut state, vec![key(Key::Escape)]);
        assert!(out.is_empty());
        assert!(state.details.is_none(), "Esc drops the details first");
    }

    fn click(at: egui::Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![
            Vec::new(),
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
        ]
    }

    #[test]
    fn a_click_on_the_selected_card_opens_its_details() {
        let view = view();
        let mut state = ListUi::default();
        // The first card, which starts selected.
        frames(&view, &mut state, click(pos2(150.0, 150.0)));
        assert_eq!(state.details.as_ref().map(|d| d.slot), Some(0));
    }

    #[test]
    fn keys_in_quick_access_become_requests() {
        let view = view();
        let mut state = ListUi::default();
        let out = draw(&view, &mut state, vec![key(Key::Delete)]);
        assert!(out.contains(&Outcome::Send(Request::Remove(0))));
        let mut state = ListUi::default();
        let out = draw(&view, &mut state, vec![key(Key::Enter)]);
        assert!(out.contains(&Outcome::Send(Request::Jump(0))));
        assert!(out.contains(&Outcome::Close));
        let mut state = ListUi::default();
        let out = draw(&view, &mut state, vec![key(Key::Num2)]);
        assert!(out.contains(&Outcome::Send(Request::Jump(1))));
        let mut state = ListUi::default();
        let out = draw(&view, &mut state, vec![key(Key::ArrowRight)]);
        assert!(out.is_empty());
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn with_no_pins_one_plain_line_shows_and_esc_closes() {
        let mut empty = view();
        empty.slots = slot_views(&Slots::default(), &[], 0);
        empty.pins.clear();
        let mut state = ListUi::default();
        let out = draw(&empty, &mut state, vec![key(Key::Escape)]);
        assert!(out.contains(&Outcome::Close));
    }
}
